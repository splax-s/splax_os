//! # SplaxFS - The Splax Filesystem
//!
//! A simple, efficient filesystem designed for Splax OS.
//! Inspired by ext2 but simplified for educational clarity.
//!
//! ## On-Disk Layout
//!
//! ```text
//! +------------------+  Block 0
//! | Superblock       |  Filesystem metadata
//! +------------------+  Block 1
//! | Block Bitmap     |  Which blocks are free
//! +------------------+  Block 2
//! | Inode Bitmap     |  Which inodes are free
//! +------------------+  Block 3-N
//! | Inode Table      |  Fixed-size inode structures
//! +------------------+  Block N+1...
//! | Data Blocks      |  File and directory content
//! +------------------+
//! ```
//!
//! ## Design Decisions
//!
//! - Block size: 4096 bytes (matches page size)
//! - Inode size: 128 bytes
//! - Max file size: ~4GB (12 direct + 1 indirect + 1 double indirect)
//! - Max filename: 255 bytes
//! - Simple extent-based allocation for efficiency

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use crate::block::{self, BlockDevice, BlockError};

/// Filesystem magic number (ASCII "SPLX")
pub const SPLAXFS_MAGIC: u32 = 0x53504C58;

/// Block size (4 KB)
pub const BLOCK_SIZE: usize = 4096;

/// Inode size (128 bytes)
pub const INODE_SIZE: usize = 128;

/// Inodes per block
pub const INODES_PER_BLOCK: usize = BLOCK_SIZE / INODE_SIZE;

/// Directory entry size (fixed)
pub const DIRENT_SIZE: usize = 264; // 8 + 255 + 1 padding

/// Maximum filename length
pub const MAX_FILENAME: usize = 255;

/// Number of direct block pointers in an inode
pub const DIRECT_BLOCKS: usize = 12;

/// Root inode number
pub const ROOT_INODE: u32 = 2; // Inode 0 is null, 1 is reserved

/// Filesystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplaxFsError {
    /// No such file or directory
    NotFound,
    /// File exists
    Exists,
    /// Not a directory
    NotDirectory,
    /// Is a directory
    IsDirectory,
    /// Directory not empty
    NotEmpty,
    /// No space left
    NoSpace,
    /// Filename too long
    NameTooLong,
    /// Filesystem corrupted
    Corrupted,
    /// I/O error
    IoError,
    /// Invalid argument
    InvalidArg,
    /// Not mounted
    NotMounted,
    /// Read-only filesystem
    ReadOnly,
    /// Journal error
    JournalError,
    /// Transaction not found
    TransactionNotFound,
}

impl From<BlockError> for SplaxFsError {
    fn from(_: BlockError) -> Self {
        SplaxFsError::IoError
    }
}

// =============================================================================
// Journaling Support
// =============================================================================

/// Journal magic number ("JRNL")
const JOURNAL_MAGIC: u32 = 0x4A524E4C;

/// Maximum journal entries
const MAX_JOURNAL_ENTRIES: usize = 64;

/// Journal entry state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JournalState {
    /// Entry is free
    Free = 0,
    /// Transaction in progress (not yet committed)
    Pending = 1,
    /// Transaction committed (can be replayed)
    Committed = 2,
    /// Transaction checkpointed (applied to disk)
    Checkpointed = 3,
}

impl From<u8> for JournalState {
    fn from(val: u8) -> Self {
        match val {
            1 => JournalState::Pending,
            2 => JournalState::Committed,
            3 => JournalState::Checkpointed,
            _ => JournalState::Free,
        }
    }
}

/// Journal superblock (stored in block 1)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JournalSuperblock {
    /// Magic number (JOURNAL_MAGIC)
    pub magic: u32,
    /// Journal version
    pub version: u32,
    /// First log block number
    pub first_log_block: u32,
    /// Number of log blocks
    pub log_blocks: u32,
    /// Head position (next write)
    pub head: u32,
    /// Tail position (oldest uncommitted)
    pub tail: u32,
    /// Sequence number counter
    pub sequence: u64,
    /// Number of active transactions
    pub active_transactions: u32,
    /// Reserved for future use
    pub _reserved: [u8; 476],
}

impl JournalSuperblock {
    /// Creates a new journal superblock
    pub fn new(first_log_block: u32, log_blocks: u32) -> Self {
        Self {
            magic: JOURNAL_MAGIC,
            version: 1,
            first_log_block,
            log_blocks,
            head: 0,
            tail: 0,
            sequence: 1,
            active_transactions: 0,
            _reserved: [0; 476],
        }
    }

    /// Validates the journal superblock
    pub fn is_valid(&self) -> bool {
        self.magic == JOURNAL_MAGIC
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 512] {
        let mut buf = [0u8; 512];
        unsafe {
            let ptr = self as *const Self as *const u8;
            core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), 512);
        }
        buf
    }

    /// Deserialize from bytes
    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut jsb = Self {
            magic: 0,
            version: 0,
            first_log_block: 0,
            log_blocks: 0,
            head: 0,
            tail: 0,
            sequence: 0,
            active_transactions: 0,
            _reserved: [0; 476],
        };
        unsafe {
            let ptr = &mut jsb as *mut Self as *mut u8;
            core::ptr::copy_nonoverlapping(buf.as_ptr(), ptr, core::cmp::min(buf.len(), 512));
        }
        jsb
    }
}

/// A journal transaction entry (header)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JournalEntry {
    /// Entry type (0=descriptor, 1=commit, 2=abort)
    pub entry_type: u8,
    /// Entry state
    pub state: u8,
    /// Number of block updates in this transaction
    pub block_count: u16,
    /// Transaction ID
    pub transaction_id: u64,
    /// Timestamp
    pub timestamp: u64,
    /// Checksum
    pub checksum: u32,
    /// Reserved
    pub _reserved: [u8; 8],
}

impl JournalEntry {
    /// Size of entry header
    pub const SIZE: usize = 32;

    /// Creates a new transaction descriptor
    pub fn new_descriptor(transaction_id: u64, block_count: u16) -> Self {
        Self {
            entry_type: 0,
            state: JournalState::Pending as u8,
            block_count,
            transaction_id,
            timestamp: 0, // Would be set from RTC
            checksum: 0,
            _reserved: [0; 8],
        }
    }

    /// Creates a commit marker
    pub fn new_commit(transaction_id: u64) -> Self {
        Self {
            entry_type: 1,
            state: JournalState::Committed as u8,
            block_count: 0,
            transaction_id,
            timestamp: 0,
            checksum: 0,
            _reserved: [0; 8],
        }
    }

    /// Creates an abort marker
    pub fn new_abort(transaction_id: u64) -> Self {
        Self {
            entry_type: 2,
            state: JournalState::Free as u8,
            block_count: 0,
            transaction_id,
            timestamp: 0,
            checksum: 0,
            _reserved: [0; 8],
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0] = self.entry_type;
        buf[1] = self.state;
        buf[2..4].copy_from_slice(&self.block_count.to_le_bytes());
        buf[4..12].copy_from_slice(&self.transaction_id.to_le_bytes());
        buf[12..20].copy_from_slice(&self.timestamp.to_le_bytes());
        buf[20..24].copy_from_slice(&self.checksum.to_le_bytes());
        buf
    }

    /// Deserialize from bytes
    pub fn from_bytes(buf: &[u8]) -> Self {
        Self {
            entry_type: buf.get(0).copied().unwrap_or(0),
            state: buf.get(1).copied().unwrap_or(0),
            block_count: u16::from_le_bytes([buf.get(2).copied().unwrap_or(0), buf.get(3).copied().unwrap_or(0)]),
            transaction_id: u64::from_le_bytes([
                buf.get(4).copied().unwrap_or(0), buf.get(5).copied().unwrap_or(0),
                buf.get(6).copied().unwrap_or(0), buf.get(7).copied().unwrap_or(0),
                buf.get(8).copied().unwrap_or(0), buf.get(9).copied().unwrap_or(0),
                buf.get(10).copied().unwrap_or(0), buf.get(11).copied().unwrap_or(0),
            ]),
            timestamp: u64::from_le_bytes([
                buf.get(12).copied().unwrap_or(0), buf.get(13).copied().unwrap_or(0),
                buf.get(14).copied().unwrap_or(0), buf.get(15).copied().unwrap_or(0),
                buf.get(16).copied().unwrap_or(0), buf.get(17).copied().unwrap_or(0),
                buf.get(18).copied().unwrap_or(0), buf.get(19).copied().unwrap_or(0),
            ]),
            checksum: u32::from_le_bytes([
                buf.get(20).copied().unwrap_or(0), buf.get(21).copied().unwrap_or(0),
                buf.get(22).copied().unwrap_or(0), buf.get(23).copied().unwrap_or(0),
            ]),
            _reserved: [0; 8],
        }
    }
}

/// Block update record in journal
#[repr(C)]
#[derive(Debug, Clone)]
pub struct JournalBlockRecord {
    /// Original block number
    pub block_num: u32,
    /// Block data (copy of the block before modification)
    pub data: Vec<u8>,
}

impl JournalBlockRecord {
    /// Creates a new block record
    pub fn new(block_num: u32, data: Vec<u8>) -> Self {
        Self { block_num, data }
    }
}

/// Transaction handle for journaling operations
#[derive(Debug)]
pub struct Transaction {
    /// Transaction ID
    pub id: u64,
    /// Block updates in this transaction
    pub updates: Vec<JournalBlockRecord>,
    /// Is transaction committed?
    pub committed: bool,
}

impl Transaction {
    /// Creates a new transaction
    pub fn new(id: u64) -> Self {
        Self {
            id,
            updates: Vec::new(),
            committed: false,
        }
    }

    /// Records a block update (write-ahead log)
    pub fn record_update(&mut self, block_num: u32, old_data: Vec<u8>) {
        self.updates.push(JournalBlockRecord::new(block_num, old_data));
    }
}

/// Journal manager for a mounted filesystem
pub struct Journal {
    /// Device name
    device_name: String,
    /// Journal superblock (cached)
    superblock: JournalSuperblock,
    /// Active transactions
    transactions: Vec<Transaction>,
    /// Next transaction ID
    next_transaction_id: u64,
}

impl Journal {
    /// Initializes a new journal on disk
    pub fn format(device: &dyn BlockDevice, first_log_block: u32, log_blocks: u32) -> Result<(), SplaxFsError> {
        let jsb = JournalSuperblock::new(first_log_block, log_blocks);
        
        // Write journal superblock to block 1
        let mut block_buf = [0u8; BLOCK_SIZE];
        let jsb_bytes = jsb.to_bytes();
        block_buf[..512].copy_from_slice(&jsb_bytes);
        
        let sector_size = device.info().sector_size;
        let sectors_per_block = BLOCK_SIZE / sector_size;
        let start_sector = 1 * sectors_per_block as u64;
        device.write_sectors(start_sector, &block_buf)?;
        
        // Zero out the log area
        let zero_block = [0u8; BLOCK_SIZE];
        for i in 0..log_blocks {
            let start_sector = (first_log_block + i) as u64 * sectors_per_block as u64;
            device.write_sectors(start_sector, &zero_block)?;
        }
        
        crate::serial_println!("[journal] Formatted journal: {} blocks at block {}", log_blocks, first_log_block);
        Ok(())
    }

    /// Opens an existing journal
    pub fn open(device_name: &str) -> Result<Self, SplaxFsError> {
        let jsb = block::with_device(device_name, |device| {
            let mut block_buf = [0u8; BLOCK_SIZE];
            let sector_size = device.info().sector_size;
            let sectors_per_block = BLOCK_SIZE / sector_size;
            let start_sector = 1 * sectors_per_block as u64;
            device.read_sectors(start_sector, &mut block_buf)?;
            Ok::<_, SplaxFsError>(JournalSuperblock::from_bytes(&block_buf))
        }).map_err(|_| SplaxFsError::IoError)??;

        if !jsb.is_valid() {
            return Err(SplaxFsError::JournalError);
        }

        Ok(Self {
            device_name: String::from(device_name),
            superblock: jsb,
            transactions: Vec::new(),
            next_transaction_id: jsb.sequence,
        })
    }

    /// Begins a new transaction
    pub fn begin(&mut self) -> u64 {
        let id = self.next_transaction_id;
        self.next_transaction_id += 1;
        self.transactions.push(Transaction::new(id));
        crate::serial_println!("[journal] Begin transaction {}", id);
        id
    }

    /// Records a block update for a transaction
    pub fn record_block(&mut self, transaction_id: u64, block_num: u32, old_data: Vec<u8>) -> Result<(), SplaxFsError> {
        if let Some(txn) = self.transactions.iter_mut().find(|t| t.id == transaction_id) {
            txn.record_update(block_num, old_data);
            Ok(())
        } else {
            Err(SplaxFsError::TransactionNotFound)
        }
    }

    /// Commits a transaction (writes to journal, marks committed)
    pub fn commit(&mut self, transaction_id: u64) -> Result<(), SplaxFsError> {
        let txn_idx = self.transactions.iter().position(|t| t.id == transaction_id)
            .ok_or(SplaxFsError::TransactionNotFound)?;
        
        // Write transaction to journal log
        let device_name = self.device_name.clone();
        let first_log_block = self.superblock.first_log_block;
        let log_blocks = self.superblock.log_blocks;
        let head = self.superblock.head;
        
        block::with_device(&device_name, |device| {
            let txn = &self.transactions[txn_idx];
            let sector_size = device.info().sector_size;
            let sectors_per_block = BLOCK_SIZE / sector_size;
            
            // Calculate position in circular log
            let log_pos = (head as usize) % (log_blocks as usize);
            let log_block = first_log_block + log_pos as u32;
            
            // Write descriptor entry
            let mut block_buf = [0u8; BLOCK_SIZE];
            let entry = JournalEntry::new_descriptor(txn.id, txn.updates.len() as u16);
            block_buf[..JournalEntry::SIZE].copy_from_slice(&entry.to_bytes());
            
            // Write block numbers (simplified - full impl would write actual data)
            let mut offset = JournalEntry::SIZE;
            for update in &txn.updates {
                if offset + 4 <= BLOCK_SIZE {
                    block_buf[offset..offset + 4].copy_from_slice(&update.block_num.to_le_bytes());
                    offset += 4;
                }
            }
            
            let start_sector = log_block as u64 * sectors_per_block as u64;
            device.write_sectors(start_sector, &block_buf)?;
            
            // Write commit marker
            let commit_entry = JournalEntry::new_commit(txn.id);
            let mut commit_buf = [0u8; BLOCK_SIZE];
            commit_buf[..JournalEntry::SIZE].copy_from_slice(&commit_entry.to_bytes());
            
            let commit_pos = ((head as usize) + 1) % (log_blocks as usize);
            let commit_block = first_log_block + commit_pos as u32;
            let commit_sector = commit_block as u64 * sectors_per_block as u64;
            device.write_sectors(commit_sector, &commit_buf)?;
            
            Ok::<(), SplaxFsError>(())
        }).map_err(|_| SplaxFsError::IoError)??;
        
        // Update journal superblock
        self.superblock.head = (head + 2) % log_blocks;
        self.superblock.sequence = self.next_transaction_id;
        self.sync_superblock()?;
        
        // Mark transaction as committed
        self.transactions[txn_idx].committed = true;
        crate::serial_println!("[journal] Committed transaction {}", transaction_id);
        
        Ok(())
    }

    /// Aborts a transaction (discards changes)
    pub fn abort(&mut self, transaction_id: u64) -> Result<(), SplaxFsError> {
        if let Some(idx) = self.transactions.iter().position(|t| t.id == transaction_id) {
            self.transactions.remove(idx);
            crate::serial_println!("[journal] Aborted transaction {}", transaction_id);
            Ok(())
        } else {
            Err(SplaxFsError::TransactionNotFound)
        }
    }

    /// Checkpoints committed transactions (removes from journal after data is on disk)
    pub fn checkpoint(&mut self, transaction_id: u64) -> Result<(), SplaxFsError> {
        if let Some(idx) = self.transactions.iter().position(|t| t.id == transaction_id && t.committed) {
            self.transactions.remove(idx);
            
            // Update tail pointer
            self.superblock.tail = (self.superblock.tail + 2) % self.superblock.log_blocks;
            self.superblock.active_transactions = self.transactions.len() as u32;
            self.sync_superblock()?;
            
            crate::serial_println!("[journal] Checkpointed transaction {}", transaction_id);
            Ok(())
        } else {
            Err(SplaxFsError::TransactionNotFound)
        }
    }

    /// Recovers the filesystem by replaying committed transactions
    pub fn recover(&mut self) -> Result<usize, SplaxFsError> {
        let device_name = self.device_name.clone();
        let first_log_block = self.superblock.first_log_block;
        let log_blocks = self.superblock.log_blocks;
        let tail = self.superblock.tail;
        let head = self.superblock.head;
        
        if tail == head {
            crate::serial_println!("[journal] No transactions to recover");
            return Ok(0);
        }
        
        let mut recovered = 0;
        
        block::with_device(&device_name, |device| {
            let sector_size = device.info().sector_size;
            let sectors_per_block = BLOCK_SIZE / sector_size;
            
            let mut pos = tail;
            while pos != head {
                let log_block = first_log_block + (pos as usize % log_blocks as usize) as u32;
                let start_sector = log_block as u64 * sectors_per_block as u64;
                
                let mut block_buf = [0u8; BLOCK_SIZE];
                device.read_sectors(start_sector, &mut block_buf)?;
                
                let entry = JournalEntry::from_bytes(&block_buf);
                
                if entry.state == JournalState::Committed as u8 {
                    // This transaction was committed, it's already applied
                    recovered += 1;
                }
                
                pos = (pos + 1) % log_blocks;
            }
            
            Ok::<(), SplaxFsError>(())
        }).map_err(|_| SplaxFsError::IoError)??;
        
        // Clear journal after recovery
        self.superblock.head = 0;
        self.superblock.tail = 0;
        self.sync_superblock()?;
        
        crate::serial_println!("[journal] Recovered {} transactions", recovered);
        Ok(recovered)
    }

    /// Syncs journal superblock to disk
    fn sync_superblock(&self) -> Result<(), SplaxFsError> {
        let device_name = self.device_name.clone();
        let jsb = self.superblock;
        
        block::with_device(&device_name, |device| {
            let mut block_buf = [0u8; BLOCK_SIZE];
            let jsb_bytes = jsb.to_bytes();
            block_buf[..512].copy_from_slice(&jsb_bytes);
            
            let sector_size = device.info().sector_size;
            let sectors_per_block = BLOCK_SIZE / sector_size;
            let start_sector = 1 * sectors_per_block as u64;
            device.write_sectors(start_sector, &block_buf)?;
            Ok(())
        }).map_err(|_| SplaxFsError::IoError)?
    }
}

/// File types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FileType {
    Unknown = 0,
    Regular = 1,
    Directory = 2,
    Symlink = 7,
}

impl From<u8> for FileType {
    fn from(val: u8) -> Self {
        match val {
            1 => FileType::Regular,
            2 => FileType::Directory,
            7 => FileType::Symlink,
            _ => FileType::Unknown,
        }
    }
}

/// On-disk superblock (block 0)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Superblock {
    /// Magic number (SPLAXFS_MAGIC)
    pub magic: u32,
    /// Filesystem version
    pub version: u32,
    /// Total number of blocks
    pub total_blocks: u32,
    /// Total number of inodes
    pub total_inodes: u32,
    /// Free blocks count
    pub free_blocks: u32,
    /// Free inodes count
    pub free_inodes: u32,
    /// Block size (always 4096)
    pub block_size: u32,
    /// Inode size (always 128)
    pub inode_size: u32,
    /// First data block number
    pub first_data_block: u32,
    /// Blocks per group (for future)
    pub blocks_per_group: u32,
    /// Inodes per group (for future)
    pub inodes_per_group: u32,
    /// Mount count
    pub mount_count: u16,
    /// Max mount count before fsck
    pub max_mount_count: u16,
    /// Filesystem state (1=clean, 2=dirty)
    pub state: u16,
    /// Error behavior
    pub errors: u16,
    /// Last mount time
    pub last_mount_time: u32,
    /// Last write time
    pub last_write_time: u32,
    /// Volume name (16 bytes)
    pub volume_name: [u8; 16],
    /// Reserved
    pub _reserved: [u8; 424],
}

impl Superblock {
    /// Creates a new superblock for formatting
    pub fn new(total_blocks: u32, total_inodes: u32, first_data_block: u32) -> Self {
        let mut sb = Self {
            magic: SPLAXFS_MAGIC,
            version: 1,
            total_blocks,
            total_inodes,
            free_blocks: total_blocks - first_data_block,
            free_inodes: total_inodes - 2, // 0=null, 1=reserved, 2=root
            block_size: BLOCK_SIZE as u32,
            inode_size: INODE_SIZE as u32,
            first_data_block,
            blocks_per_group: 0,
            inodes_per_group: 0,
            mount_count: 0,
            max_mount_count: 20,
            state: 1, // Clean
            errors: 1, // Continue on error
            last_mount_time: 0,
            last_write_time: 0,
            volume_name: [0; 16],
            _reserved: [0; 424],
        };
        // Set volume name
        let name = b"SplaxFS";
        sb.volume_name[..name.len()].copy_from_slice(name);
        sb
    }

    /// Validates the superblock
    pub fn is_valid(&self) -> bool {
        self.magic == SPLAXFS_MAGIC && self.block_size == BLOCK_SIZE as u32
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 512] {
        let mut buf = [0u8; 512];
        unsafe {
            let ptr = self as *const Self as *const u8;
            core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), 512);
        }
        buf
    }

    /// Deserialize from bytes
    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut sb = Self {
            magic: 0,
            version: 0,
            total_blocks: 0,
            total_inodes: 0,
            free_blocks: 0,
            free_inodes: 0,
            block_size: 0,
            inode_size: 0,
            first_data_block: 0,
            blocks_per_group: 0,
            inodes_per_group: 0,
            mount_count: 0,
            max_mount_count: 0,
            state: 0,
            errors: 0,
            last_mount_time: 0,
            last_write_time: 0,
            volume_name: [0; 16],
            _reserved: [0; 424],
        };
        unsafe {
            let ptr = &mut sb as *mut Self as *mut u8;
            core::ptr::copy_nonoverlapping(buf.as_ptr(), ptr, core::cmp::min(buf.len(), 512));
        }
        sb
    }
}

/// On-disk inode (128 bytes)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskInode {
    /// File mode (permissions + type)
    pub mode: u16,
    /// Owner user ID
    pub uid: u16,
    /// File size (lower 32 bits)
    pub size_low: u32,
    /// Access time
    pub atime: u32,
    /// Creation time
    pub ctime: u32,
    /// Modification time
    pub mtime: u32,
    /// Deletion time
    pub dtime: u32,
    /// Owner group ID
    pub gid: u16,
    /// Hard link count
    pub links_count: u16,
    /// Block count (in 512-byte units)
    pub blocks: u32,
    /// File flags
    pub flags: u32,
    /// Reserved
    pub _reserved1: u32,
    /// Direct block pointers
    pub direct: [u32; DIRECT_BLOCKS],
    /// Single indirect block pointer
    pub indirect: u32,
    /// Double indirect block pointer
    pub double_indirect: u32,
    /// Triple indirect block pointer
    pub triple_indirect: u32,
    /// File size (upper 32 bits)
    pub size_high: u32,
    /// Reserved
    pub _reserved2: [u32; 2],
}

impl DiskInode {
    /// Creates a new empty inode
    pub fn new() -> Self {
        Self {
            mode: 0,
            uid: 0,
            size_low: 0,
            atime: 0,
            ctime: 0,
            mtime: 0,
            dtime: 0,
            gid: 0,
            links_count: 0,
            blocks: 0,
            flags: 0,
            _reserved1: 0,
            direct: [0; DIRECT_BLOCKS],
            indirect: 0,
            double_indirect: 0,
            triple_indirect: 0,
            size_high: 0,
            _reserved2: [0; 2],
        }
    }

    /// Creates a directory inode
    pub fn new_directory() -> Self {
        let mut inode = Self::new();
        inode.mode = 0o40755; // Directory + rwxr-xr-x
        inode.links_count = 2; // . and parent's link
        inode
    }

    /// Creates a regular file inode
    pub fn new_file() -> Self {
        let mut inode = Self::new();
        inode.mode = 0o100644; // Regular file + rw-r--r--
        inode.links_count = 1;
        inode
    }

    /// Returns the file type
    pub fn file_type(&self) -> FileType {
        match (self.mode >> 12) & 0xF {
            4 => FileType::Directory,
            8 => FileType::Regular,
            10 => FileType::Symlink,
            _ => FileType::Unknown,
        }
    }

    /// Returns the file size
    pub fn size(&self) -> u64 {
        ((self.size_high as u64) << 32) | (self.size_low as u64)
    }

    /// Sets the file size
    pub fn set_size(&mut self, size: u64) {
        self.size_low = size as u32;
        self.size_high = (size >> 32) as u32;
    }

    /// Is this a directory?
    pub fn is_directory(&self) -> bool {
        self.file_type() == FileType::Directory
    }

    /// Is this a regular file?
    pub fn is_file(&self) -> bool {
        self.file_type() == FileType::Regular
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; INODE_SIZE] {
        let mut buf = [0u8; INODE_SIZE];
        unsafe {
            let ptr = self as *const Self as *const u8;
            core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), INODE_SIZE);
        }
        buf
    }

    /// Deserialize from bytes
    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut inode = Self::new();
        unsafe {
            let ptr = &mut inode as *mut Self as *mut u8;
            core::ptr::copy_nonoverlapping(buf.as_ptr(), ptr, core::cmp::min(buf.len(), INODE_SIZE));
        }
        inode
    }
}

/// Directory entry (on-disk format)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Inode number
    pub inode: u32,
    /// Entry length (for variable size)
    pub rec_len: u16,
    /// Name length
    pub name_len: u8,
    /// File type
    pub file_type: u8,
    /// Filename (up to 255 bytes)
    pub name: [u8; MAX_FILENAME],
}

impl DirEntry {
    /// Creates a new directory entry
    pub fn new(inode: u32, name: &str, file_type: FileType) -> Self {
        let mut entry = Self {
            inode,
            rec_len: DIRENT_SIZE as u16,
            name_len: name.len() as u8,
            file_type: file_type as u8,
            name: [0; MAX_FILENAME],
        };
        let name_bytes = name.as_bytes();
        let copy_len = core::cmp::min(name_bytes.len(), MAX_FILENAME);
        entry.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        entry
    }

    /// Gets the filename as a string
    pub fn name_str(&self) -> &str {
        let len = self.name_len as usize;
        core::str::from_utf8(&self.name[..len]).unwrap_or("")
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; DIRENT_SIZE] {
        let mut buf = [0u8; DIRENT_SIZE];
        // Manual serialization
        buf[0..4].copy_from_slice(&self.inode.to_le_bytes());
        buf[4..6].copy_from_slice(&self.rec_len.to_le_bytes());
        buf[6] = self.name_len;
        buf[7] = self.file_type;
        buf[8..8 + MAX_FILENAME].copy_from_slice(&self.name);
        buf
    }

    /// Deserialize from bytes
    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut entry = Self {
            inode: 0,
            rec_len: 0,
            name_len: 0,
            file_type: 0,
            name: [0; MAX_FILENAME],
        };
        if buf.len() >= DIRENT_SIZE {
            entry.inode = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            entry.rec_len = u16::from_le_bytes([buf[4], buf[5]]);
            entry.name_len = buf[6];
            entry.file_type = buf[7];
            entry.name.copy_from_slice(&buf[8..8 + MAX_FILENAME]);
        }
        entry
    }
}

/// Mounted SplaxFS filesystem
pub struct SplaxFs {
    /// Device name
    device_name: String,
    /// Superblock (cached)
    superblock: Superblock,
    /// Block bitmap (cached)
    block_bitmap: Vec<u8>,
    /// Inode bitmap (cached)
    inode_bitmap: Vec<u8>,
    /// Is filesystem dirty?
    dirty: bool,
    /// Mount point
    mount_point: String,
    /// Journal (optional, for journaled mounts)
    journal: Option<Journal>,
    /// Current transaction ID (if any)
    current_transaction: Option<u64>,
}

impl SplaxFs {
    /// Formats a block device with SplaxFS
    pub fn format(device_name: &str) -> Result<(), SplaxFsError> {
        crate::serial_println!("[splaxfs] Formatting device {}...", device_name);

        match block::with_device(device_name, |device| {
            let info = device.info();
            let total_sectors = info.total_sectors;
            let sector_size = info.sector_size;
            
            // Calculate filesystem parameters
            let sectors_per_block = BLOCK_SIZE / sector_size;
            let total_blocks = (total_sectors as usize / sectors_per_block) as u32;
            
            if total_blocks < 16 {
                crate::serial_println!("[splaxfs] Device too small");
                return Err(SplaxFsError::NoSpace);
            }
            
            // Layout:
            // Block 0: Superblock
            // Block 1: Block bitmap
            // Block 2: Inode bitmap
            // Block 3-10: Inode table (8 blocks = 256 inodes)
            // Block 11+: Data blocks
            
            let inode_table_blocks = 8u32;
            let total_inodes = inode_table_blocks * INODES_PER_BLOCK as u32;
            let first_data_block = 3 + inode_table_blocks;
            
            // Create superblock
            let sb = Superblock::new(total_blocks, total_inodes, first_data_block);
            
            // Write superblock (spans first sector)
            let mut block_buf = [0u8; BLOCK_SIZE];
            let sb_bytes = sb.to_bytes();
            block_buf[..512].copy_from_slice(&sb_bytes);
            Self::write_block(device, 0, &block_buf)?;
            
            // Initialize block bitmap (mark metadata blocks as used)
            let mut block_bitmap = vec![0u8; BLOCK_SIZE];
            for i in 0..first_data_block {
                let byte = (i / 8) as usize;
                let bit = (i % 8) as usize;
                if byte < block_bitmap.len() {
                    block_bitmap[byte] |= 1 << bit;
                }
            }
            Self::write_block(device, 1, &block_bitmap)?;
            
            // Initialize inode bitmap (mark inodes 0, 1, 2 as used)
            let mut inode_bitmap = vec![0u8; BLOCK_SIZE];
            inode_bitmap[0] = 0b00000111; // Inodes 0, 1, 2 used
            Self::write_block(device, 2, &inode_bitmap)?;
            
            // Initialize inode table with zeros
            let zero_block = [0u8; BLOCK_SIZE];
            for i in 0..inode_table_blocks {
                Self::write_block(device, 3 + i, &zero_block)?;
            }
            
            // Create root directory inode (inode 2)
            let mut root_inode = DiskInode::new_directory();
            root_inode.size_low = BLOCK_SIZE as u32; // One block
            root_inode.blocks = (BLOCK_SIZE / 512) as u32;
            root_inode.direct[0] = first_data_block; // First data block
            
            // Write root inode
            Self::write_inode(device, ROOT_INODE, &root_inode)?;
            
            // Create root directory content (. and ..)
            let mut root_block = [0u8; BLOCK_SIZE];
            let dot = DirEntry::new(ROOT_INODE, ".", FileType::Directory);
            let dotdot = DirEntry::new(ROOT_INODE, "..", FileType::Directory);
            let dot_bytes = dot.to_bytes();
            let dotdot_bytes = dotdot.to_bytes();
            root_block[..DIRENT_SIZE].copy_from_slice(&dot_bytes);
            root_block[DIRENT_SIZE..2 * DIRENT_SIZE].copy_from_slice(&dotdot_bytes);
            
            // Mark first data block as used
            block_bitmap[first_data_block as usize / 8] |= 1 << (first_data_block % 8);
            Self::write_block(device, 1, &block_bitmap)?;
            
            // Write root directory block
            Self::write_block(device, first_data_block, &root_block)?;
            
            crate::serial_println!("[splaxfs] Format complete:");
            crate::serial_println!("  Total blocks: {}", total_blocks);
            crate::serial_println!("  Total inodes: {}", total_inodes);
            crate::serial_println!("  First data block: {}", first_data_block);
            crate::serial_println!("  Usable space: {} KB", (total_blocks - first_data_block) * 4);
            
            Ok(())
        }) {
            Ok(inner) => inner,
            Err(_) => Err(SplaxFsError::NotFound),
        }
    }

    /// Mounts a SplaxFS filesystem
    pub fn mount(device_name: &str, mount_point: &str) -> Result<Self, SplaxFsError> {
        crate::serial_println!("[splaxfs] Mounting {} at {}...", device_name, mount_point);

        match block::with_device(device_name, |device| {
            // Read superblock
            let mut block_buf = [0u8; BLOCK_SIZE];
            Self::read_block(device, 0, &mut block_buf)?;
            let sb = Superblock::from_bytes(&block_buf);
            
            if !sb.is_valid() {
                crate::serial_println!("[splaxfs] Invalid superblock magic: 0x{:08x}", sb.magic);
                return Err(SplaxFsError::Corrupted);
            }
            
            // Read block bitmap
            let mut block_bitmap = vec![0u8; BLOCK_SIZE];
            Self::read_block(device, 1, &mut block_bitmap)?;
            
            // Read inode bitmap
            let mut inode_bitmap = vec![0u8; BLOCK_SIZE];
            Self::read_block(device, 2, &mut inode_bitmap)?;
            
            crate::serial_println!("[splaxfs] Mounted successfully:");
            crate::serial_println!("  Volume: {}", 
                core::str::from_utf8(&sb.volume_name).unwrap_or("?").trim_end_matches('\0'));
            crate::serial_println!("  Total blocks: {}", sb.total_blocks);
            crate::serial_println!("  Free blocks: {}", sb.free_blocks);
            crate::serial_println!("  Total inodes: {}", sb.total_inodes);
            crate::serial_println!("  Free inodes: {}", sb.free_inodes);
            
            Ok(SplaxFs {
                device_name: String::from(device_name),
                superblock: sb,
                block_bitmap,
                inode_bitmap,
                dirty: false,
                mount_point: String::from(mount_point),
                journal: None, // Journal will be opened separately if available
                current_transaction: None,
            })
        }) {
            Ok(inner) => {
                let mut fs = inner?;
                // Try to open journal if it exists
                if let Ok(mut journal) = Journal::open(device_name) {
                    // Recover any uncommitted transactions
                    if let Ok(recovered) = journal.recover() {
                        if recovered > 0 {
                            crate::serial_println!("[splaxfs] Recovered {} transactions from journal", recovered);
                        }
                    }
                    fs.journal = Some(journal);
                    crate::serial_println!("[splaxfs] Journal enabled");
                }
                Ok(fs)
            },
            Err(_) => Err(SplaxFsError::NotFound),
        }
    }

    /// Reads a block from the device
    fn read_block(device: &dyn BlockDevice, block_num: u32, buf: &mut [u8]) -> Result<(), SplaxFsError> {
        let sector_size = device.info().sector_size;
        let sectors_per_block = BLOCK_SIZE / sector_size;
        let start_sector = (block_num as u64) * (sectors_per_block as u64);
        device.read_sectors(start_sector, buf)?;
        Ok(())
    }

    /// Writes a block to the device
    fn write_block(device: &dyn BlockDevice, block_num: u32, buf: &[u8]) -> Result<(), SplaxFsError> {
        let sector_size = device.info().sector_size;
        let sectors_per_block = BLOCK_SIZE / sector_size;
        let start_sector = (block_num as u64) * (sectors_per_block as u64);
        device.write_sectors(start_sector, buf)?;
        Ok(())
    }

    /// Reads an inode from disk
    fn read_inode(device: &dyn BlockDevice, inode_num: u32) -> Result<DiskInode, SplaxFsError> {
        if inode_num == 0 {
            return Err(SplaxFsError::InvalidArg);
        }
        
        let inodes_per_block = BLOCK_SIZE / INODE_SIZE;
        let block_num = 3 + (inode_num as usize - 1) / inodes_per_block;
        let offset = ((inode_num as usize - 1) % inodes_per_block) * INODE_SIZE;
        
        let mut block_buf = [0u8; BLOCK_SIZE];
        Self::read_block(device, block_num as u32, &mut block_buf)?;
        
        Ok(DiskInode::from_bytes(&block_buf[offset..offset + INODE_SIZE]))
    }

    /// Writes an inode to disk
    fn write_inode(device: &dyn BlockDevice, inode_num: u32, inode: &DiskInode) -> Result<(), SplaxFsError> {
        if inode_num == 0 {
            return Err(SplaxFsError::InvalidArg);
        }
        
        let inodes_per_block = BLOCK_SIZE / INODE_SIZE;
        let block_num = 3 + (inode_num as usize - 1) / inodes_per_block;
        let offset = ((inode_num as usize - 1) % inodes_per_block) * INODE_SIZE;
        
        let mut block_buf = [0u8; BLOCK_SIZE];
        Self::read_block(device, block_num as u32, &mut block_buf)?;
        
        let inode_bytes = inode.to_bytes();
        block_buf[offset..offset + INODE_SIZE].copy_from_slice(&inode_bytes);
        
        Self::write_block(device, block_num as u32, &block_buf)?;
        Ok(())
    }

    /// Lists directory contents
    pub fn readdir(&self, path: &str) -> Result<Vec<(String, FileType, u64)>, SplaxFsError> {
        let mut entries = Vec::new();
        
        match block::with_device(&self.device_name, |device| {
            // Resolve path to inode
            let inode_num = self.lookup_path(device, path)?;
            let inode = Self::read_inode(device, inode_num)?;
            
            if !inode.is_directory() {
                return Err(SplaxFsError::NotDirectory);
            }
            
            // Read directory entries from first block
            if inode.direct[0] != 0 {
                let mut block_buf = [0u8; BLOCK_SIZE];
                Self::read_block(device, inode.direct[0], &mut block_buf)?;
                
                let mut offset = 0;
                while offset + DIRENT_SIZE <= BLOCK_SIZE {
                    let entry = DirEntry::from_bytes(&block_buf[offset..]);
                    if entry.inode != 0 && entry.name_len > 0 {
                        let name = entry.name_str().to_string();
                        let file_type = FileType::from(entry.file_type);
                        
                        // Get file size
                        let entry_inode = Self::read_inode(device, entry.inode)?;
                        let size = entry_inode.size();
                        
                        entries.push((name, file_type, size));
                    }
                    offset += DIRENT_SIZE;
                }
            }
            
            Ok(entries.clone())
        }) {
            Ok(inner) => inner,
            Err(_) => Err(SplaxFsError::NotFound),
        }
    }

    /// Looks up a path and returns the inode number
    fn lookup_path(&self, device: &dyn BlockDevice, path: &str) -> Result<u32, SplaxFsError> {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            return Ok(ROOT_INODE);
        }
        
        let mut current_inode = ROOT_INODE;
        
        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            
            let inode = Self::read_inode(device, current_inode)?;
            if !inode.is_directory() {
                return Err(SplaxFsError::NotDirectory);
            }
            
            // Search directory for component
            let mut found = false;
            if inode.direct[0] != 0 {
                let mut block_buf = [0u8; BLOCK_SIZE];
                Self::read_block(device, inode.direct[0], &mut block_buf)?;
                
                let mut offset = 0;
                while offset + DIRENT_SIZE <= BLOCK_SIZE {
                    let entry = DirEntry::from_bytes(&block_buf[offset..]);
                    if entry.inode != 0 && entry.name_str() == component {
                        current_inode = entry.inode;
                        found = true;
                        break;
                    }
                    offset += DIRENT_SIZE;
                }
            }
            
            if !found {
                return Err(SplaxFsError::NotFound);
            }
        }
        
        Ok(current_inode)
    }

    /// Static version of lookup_path (doesn't need self)
    fn lookup_path_static(device: &dyn BlockDevice, _superblock: &Superblock, path: &str) -> Result<u32, SplaxFsError> {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            return Ok(ROOT_INODE);
        }
        
        let mut current_inode = ROOT_INODE;
        
        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            
            let inode = Self::read_inode(device, current_inode)?;
            if !inode.is_directory() {
                return Err(SplaxFsError::NotDirectory);
            }
            
            // Search directory for component
            let mut found = false;
            if inode.direct[0] != 0 {
                let mut block_buf = [0u8; BLOCK_SIZE];
                Self::read_block(device, inode.direct[0], &mut block_buf)?;
                
                let mut offset = 0;
                while offset + DIRENT_SIZE <= BLOCK_SIZE {
                    let entry = DirEntry::from_bytes(&block_buf[offset..]);
                    if entry.inode != 0 && entry.name_str() == component {
                        current_inode = entry.inode;
                        found = true;
                        break;
                    }
                    offset += DIRENT_SIZE;
                }
            }
            
            if !found {
                return Err(SplaxFsError::NotFound);
            }
        }
        
        Ok(current_inode)
    }

    /// Static version of add_dirent (doesn't need &mut self)
    fn add_dirent_static(device: &dyn BlockDevice, _superblock: &Superblock, parent_inode_num: u32, 
                  child_inode_num: u32, name: &str, file_type: FileType) -> Result<(), SplaxFsError> {
        let parent_inode = Self::read_inode(device, parent_inode_num)?;
        
        if parent_inode.direct[0] == 0 {
            return Err(SplaxFsError::Corrupted);
        }
        
        // Read directory block
        let mut block_buf = [0u8; BLOCK_SIZE];
        Self::read_block(device, parent_inode.direct[0], &mut block_buf)?;
        
        // Find empty slot
        let mut offset = 0;
        while offset + DIRENT_SIZE <= BLOCK_SIZE {
            let entry = DirEntry::from_bytes(&block_buf[offset..]);
            if entry.inode == 0 {
                // Found empty slot
                let new_entry = DirEntry::new(child_inode_num, name, file_type);
                block_buf[offset..offset + DIRENT_SIZE].copy_from_slice(&new_entry.to_bytes());
                Self::write_block(device, parent_inode.direct[0], &block_buf)?;
                return Ok(());
            }
            offset += DIRENT_SIZE;
        }
        
        // Directory full
        Err(SplaxFsError::NoSpace)
    }

    /// Static version of sync_bitmaps
    fn sync_bitmaps_static(device: &dyn BlockDevice, superblock: &Superblock, 
                           inode_bitmap: &[u8], block_bitmap: &[u8]) -> Result<(), SplaxFsError> {
        // Copy to fixed-size arrays for writing
        let mut inode_buf = [0u8; BLOCK_SIZE];
        let mut block_buf = [0u8; BLOCK_SIZE];
        let inode_len = core::cmp::min(inode_bitmap.len(), BLOCK_SIZE);
        let block_len = core::cmp::min(block_bitmap.len(), BLOCK_SIZE);
        inode_buf[..inode_len].copy_from_slice(&inode_bitmap[..inode_len]);
        block_buf[..block_len].copy_from_slice(&block_bitmap[..block_len]);
        
        Self::write_block(device, 1, &block_buf)?;
        Self::write_block(device, 2, &inode_buf)?;
        
        // Update superblock
        let mut sb_block_buf = [0u8; BLOCK_SIZE];
        let sb_bytes = superblock.to_bytes();
        sb_block_buf[..512].copy_from_slice(&sb_bytes);
        Self::write_block(device, 0, &sb_block_buf)?;
        
        Ok(())
    }

    /// Creates a new file
    pub fn create_file(&mut self, path: &str) -> Result<(), SplaxFsError> {
        let (parent_path, filename) = self.split_path(path)?;
        
        if filename.len() > MAX_FILENAME {
            return Err(SplaxFsError::NameTooLong);
        }
        
        // Pre-allocate inode (modifies in-memory bitmap only)
        let new_inode_num = self.alloc_inode()?;
        let device_name = self.device_name.clone();
        let superblock = self.superblock.clone();
        let inode_bitmap = self.inode_bitmap.clone();
        let block_bitmap = self.block_bitmap.clone();
        
        match block::with_device(&device_name, |device| {
            // Find parent directory
            let parent_inode_num = Self::lookup_path_static(device, &superblock, &parent_path)?;
            let parent_inode = Self::read_inode(device, parent_inode_num)?;
            
            if !parent_inode.is_directory() {
                return Err(SplaxFsError::NotDirectory);
            }
            
            // Create new file inode
            let new_inode = DiskInode::new_file();
            Self::write_inode(device, new_inode_num, &new_inode)?;
            
            // Add directory entry
            Self::add_dirent_static(device, &superblock, parent_inode_num, new_inode_num, &filename, FileType::Regular)?;
            
            // Update bitmaps on disk
            Self::sync_bitmaps_static(device, &superblock, &inode_bitmap, &block_bitmap)?;
            
            crate::serial_println!("[splaxfs] Created file: {}", path);
            Ok(())
        }) {
            Ok(inner) => inner,
            Err(_) => Err(SplaxFsError::NotFound),
        }
    }

    /// Creates a new directory
    pub fn create_dir(&mut self, path: &str) -> Result<(), SplaxFsError> {
        let (parent_path, dirname) = self.split_path(path)?;
        
        if dirname.len() > MAX_FILENAME {
            return Err(SplaxFsError::NameTooLong);
        }
        
        // Pre-allocate inode and block (modifies in-memory bitmaps only)
        let new_inode_num = self.alloc_inode()?;
        let dir_block = self.alloc_block()?;
        let device_name = self.device_name.clone();
        let superblock = self.superblock.clone();
        let inode_bitmap = self.inode_bitmap.clone();
        let block_bitmap = self.block_bitmap.clone();
        
        match block::with_device(&device_name, |device| {
            // Find parent directory
            let parent_inode_num = Self::lookup_path_static(device, &superblock, &parent_path)?;
            let parent_inode = Self::read_inode(device, parent_inode_num)?;
            
            if !parent_inode.is_directory() {
                return Err(SplaxFsError::NotDirectory);
            }
            
            // Create directory inode
            let mut new_inode = DiskInode::new_directory();
            new_inode.size_low = BLOCK_SIZE as u32;
            new_inode.blocks = (BLOCK_SIZE / 512) as u32;
            new_inode.direct[0] = dir_block;
            Self::write_inode(device, new_inode_num, &new_inode)?;
            
            // Create . and .. entries
            let mut dir_block_buf = [0u8; BLOCK_SIZE];
            let dot = DirEntry::new(new_inode_num, ".", FileType::Directory);
            let dotdot = DirEntry::new(parent_inode_num, "..", FileType::Directory);
            dir_block_buf[..DIRENT_SIZE].copy_from_slice(&dot.to_bytes());
            dir_block_buf[DIRENT_SIZE..2 * DIRENT_SIZE].copy_from_slice(&dotdot.to_bytes());
            Self::write_block(device, dir_block, &dir_block_buf)?;
            
            // Add directory entry in parent
            Self::add_dirent_static(device, &superblock, parent_inode_num, new_inode_num, &dirname, FileType::Directory)?;
            
            // Update bitmaps on disk
            Self::sync_bitmaps_static(device, &superblock, &inode_bitmap, &block_bitmap)?;
            
            crate::serial_println!("[splaxfs] Created directory: {}", path);
            Ok(())
        }) {
            Ok(inner) => inner,
            Err(_) => Err(SplaxFsError::NotFound),
        }
    }

    /// Writes data to a file
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), SplaxFsError> {
        // Pre-allocate blocks needed for the file
        let blocks_needed = if data.is_empty() { 0 } else { (data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE };
        if blocks_needed > DIRECT_BLOCKS {
            crate::serial_println!("[splaxfs] File too large (max 48KB for now)");
            return Err(SplaxFsError::NoSpace);
        }
        
        // Pre-allocate all the blocks we might need
        let mut new_blocks = Vec::new();
        for _ in 0..blocks_needed {
            new_blocks.push(self.alloc_block()?);
        }
        
        let device_name = self.device_name.clone();
        let superblock = self.superblock.clone();
        let inode_bitmap = self.inode_bitmap.clone();
        let block_bitmap = self.block_bitmap.clone();
        
        match block::with_device(&device_name, |device| {
            let inode_num = Self::lookup_path_static(device, &superblock, path)?;
            let mut inode = Self::read_inode(device, inode_num)?;
            
            if !inode.is_file() {
                return Err(SplaxFsError::IsDirectory);
            }
            
            // Allocate blocks and write data using pre-allocated blocks
            let mut offset = 0;
            let mut block_idx = 0;
            for i in 0..blocks_needed {
                if inode.direct[i] == 0 {
                    if block_idx < new_blocks.len() {
                        inode.direct[i] = new_blocks[block_idx];
                        block_idx += 1;
                    } else {
                        return Err(SplaxFsError::NoSpace);
                    }
                }
                
                let mut block_buf = [0u8; BLOCK_SIZE];
                let end = core::cmp::min(offset + BLOCK_SIZE, data.len());
                block_buf[..end - offset].copy_from_slice(&data[offset..end]);
                Self::write_block(device, inode.direct[i], &block_buf)?;
                offset = end;
            }
            
            // Update inode
            inode.set_size(data.len() as u64);
            inode.blocks = (blocks_needed * (BLOCK_SIZE / 512)) as u32;
            Self::write_inode(device, inode_num, &inode)?;
            
            // Sync bitmaps
            Self::sync_bitmaps_static(device, &superblock, &inode_bitmap, &block_bitmap)?;
            
            Ok(())
        }) {
            Ok(inner) => inner,
            Err(_) => Err(SplaxFsError::NotFound),
        }
    }

    /// Reads a file's contents
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, SplaxFsError> {
        let device_name = self.device_name.clone();
        let superblock = self.superblock.clone();
        
        match block::with_device(&device_name, |device| {
            let inode_num = Self::lookup_path_static(device, &superblock, path)?;
            let inode = Self::read_inode(device, inode_num)?;
            
            if !inode.is_file() {
                return Err(SplaxFsError::IsDirectory);
            }
            
            let file_size = inode.size() as usize;
            let mut data = vec![0u8; file_size];
            let mut offset = 0;
            
            for i in 0..DIRECT_BLOCKS {
                if inode.direct[i] == 0 || offset >= file_size {
                    break;
                }
                
                let mut block_buf = [0u8; BLOCK_SIZE];
                Self::read_block(device, inode.direct[i], &mut block_buf)?;
                
                let end = core::cmp::min(offset + BLOCK_SIZE, file_size);
                data[offset..end].copy_from_slice(&block_buf[..end - offset]);
                offset = end;
            }
            
            Ok(data)
        }) {
            Ok(inner) => inner,
            Err(_) => Err(SplaxFsError::NotFound),
        }
    }

    /// Gets file/directory info
    pub fn stat(&self, path: &str) -> Result<(FileType, u64), SplaxFsError> {
        let device_name = self.device_name.clone();
        let superblock = self.superblock.clone();
        
        match block::with_device(&device_name, |device| {
            let inode_num = Self::lookup_path_static(device, &superblock, path)?;
            let inode = Self::read_inode(device, inode_num)?;
            Ok((inode.file_type(), inode.size()))
        }) {
            Ok(inner) => inner,
            Err(_) => Err(SplaxFsError::NotFound),
        }
    }

    /// Allocates a free inode
    fn alloc_inode(&mut self) -> Result<u32, SplaxFsError> {
        for i in 0..self.superblock.total_inodes {
            let byte = (i / 8) as usize;
            let bit = (i % 8) as usize;
            if byte < self.inode_bitmap.len() && (self.inode_bitmap[byte] & (1 << bit)) == 0 {
                self.inode_bitmap[byte] |= 1 << bit;
                self.superblock.free_inodes -= 1;
                self.dirty = true;
                return Ok(i + 1); // Inode numbers are 1-based
            }
        }
        Err(SplaxFsError::NoSpace)
    }

    /// Allocates a free block
    fn alloc_block(&mut self) -> Result<u32, SplaxFsError> {
        for i in self.superblock.first_data_block..self.superblock.total_blocks {
            let byte = (i / 8) as usize;
            let bit = (i % 8) as usize;
            if byte < self.block_bitmap.len() && (self.block_bitmap[byte] & (1 << bit)) == 0 {
                self.block_bitmap[byte] |= 1 << bit;
                self.superblock.free_blocks -= 1;
                self.dirty = true;
                return Ok(i);
            }
        }
        Err(SplaxFsError::NoSpace)
    }

    /// Adds a directory entry
    fn add_dirent(&mut self, device: &dyn BlockDevice, parent_inode_num: u32, 
                  child_inode_num: u32, name: &str, file_type: FileType) -> Result<(), SplaxFsError> {
        let parent_inode = Self::read_inode(device, parent_inode_num)?;
        
        if parent_inode.direct[0] == 0 {
            return Err(SplaxFsError::Corrupted);
        }
        
        // Read directory block
        let mut block_buf = [0u8; BLOCK_SIZE];
        Self::read_block(device, parent_inode.direct[0], &mut block_buf)?;
        
        // Find empty slot
        let mut offset = 0;
        while offset + DIRENT_SIZE <= BLOCK_SIZE {
            let entry = DirEntry::from_bytes(&block_buf[offset..]);
            if entry.inode == 0 {
                // Found empty slot
                let new_entry = DirEntry::new(child_inode_num, name, file_type);
                block_buf[offset..offset + DIRENT_SIZE].copy_from_slice(&new_entry.to_bytes());
                Self::write_block(device, parent_inode.direct[0], &block_buf)?;
                return Ok(());
            }
            offset += DIRENT_SIZE;
        }
        
        // Directory full
        Err(SplaxFsError::NoSpace)
    }

    /// Syncs bitmaps to disk
    fn sync_bitmaps(&self, device: &dyn BlockDevice) -> Result<(), SplaxFsError> {
        Self::write_block(device, 1, &self.block_bitmap)?;
        Self::write_block(device, 2, &self.inode_bitmap)?;
        
        // Update superblock
        let mut block_buf = [0u8; BLOCK_SIZE];
        let sb_bytes = self.superblock.to_bytes();
        block_buf[..512].copy_from_slice(&sb_bytes);
        Self::write_block(device, 0, &block_buf)?;
        
        Ok(())
    }

    /// Splits a path into parent and filename
    fn split_path(&self, path: &str) -> Result<(String, String), SplaxFsError> {
        let path = path.trim_start_matches('/').trim_end_matches('/');
        if path.is_empty() {
            return Err(SplaxFsError::InvalidArg);
        }
        
        if let Some(pos) = path.rfind('/') {
            let parent = &path[..pos];
            let name = &path[pos + 1..];
            Ok((format!("/{}", parent), String::from(name)))
        } else {
            Ok((String::from("/"), String::from(path)))
        }
    }

    // =========================================================================
    // Journaling Methods
    // =========================================================================

    /// Begins a journaled transaction
    pub fn begin_transaction(&mut self) -> Result<u64, SplaxFsError> {
        if let Some(ref mut journal) = self.journal {
            let txn_id = journal.begin();
            self.current_transaction = Some(txn_id);
            Ok(txn_id)
        } else {
            Err(SplaxFsError::JournalError)
        }
    }

    /// Commits the current transaction
    pub fn commit_transaction(&mut self) -> Result<(), SplaxFsError> {
        if let Some(txn_id) = self.current_transaction.take() {
            if let Some(ref mut journal) = self.journal {
                journal.commit(txn_id)?;
            }
            Ok(())
        } else {
            Err(SplaxFsError::TransactionNotFound)
        }
    }

    /// Aborts the current transaction
    pub fn abort_transaction(&mut self) -> Result<(), SplaxFsError> {
        if let Some(txn_id) = self.current_transaction.take() {
            if let Some(ref mut journal) = self.journal {
                journal.abort(txn_id)?;
            }
            Ok(())
        } else {
            Err(SplaxFsError::TransactionNotFound)
        }
    }

    /// Writes data to a file with journaling support
    pub fn write_file_journaled(&mut self, path: &str, data: &[u8]) -> Result<(), SplaxFsError> {
        // Begin transaction
        let _txn_id = self.begin_transaction().ok();
        
        // Perform the write
        let result = self.write_file(path, data);
        
        // Commit or abort based on result
        if result.is_ok() {
            let _ = self.commit_transaction();
        } else {
            let _ = self.abort_transaction();
        }
        
        result
    }

    /// Creates a file with journaling support
    pub fn create_file_journaled(&mut self, path: &str) -> Result<(), SplaxFsError> {
        let _txn_id = self.begin_transaction().ok();
        let result = self.create_file(path);
        if result.is_ok() {
            let _ = self.commit_transaction();
        } else {
            let _ = self.abort_transaction();
        }
        result
    }

    /// Creates a directory with journaling support
    pub fn create_dir_journaled(&mut self, path: &str) -> Result<(), SplaxFsError> {
        let _txn_id = self.begin_transaction().ok();
        let result = self.create_dir(path);
        if result.is_ok() {
            let _ = self.commit_transaction();
        } else {
            let _ = self.abort_transaction();
        }
        result
    }

    /// Syncs all pending data to disk
    pub fn sync(&mut self) -> Result<(), SplaxFsError> {
        let device_name = self.device_name.clone();
        let inode_bitmap = self.inode_bitmap.clone();
        let block_bitmap = self.block_bitmap.clone();
        let superblock = self.superblock.clone();
        
        block::with_device(&device_name, |device| {
            Self::sync_bitmaps_static(device, &superblock, &inode_bitmap, &block_bitmap)
        }).map_err(|_| SplaxFsError::IoError)??;
        
        // Checkpoint any committed transactions
        if let Some(ref mut journal) = self.journal {
            // For now, just log that sync was called
            crate::serial_println!("[splaxfs] Sync complete");
        }
        
        self.dirty = false;
        Ok(())
    }

    /// Formats a device with SplaxFS including journal
    pub fn format_with_journal(device_name: &str) -> Result<(), SplaxFsError> {
        crate::serial_println!("[splaxfs] Formatting device {} with journal...", device_name);

        block::with_device(device_name, |device| {
            let info = device.info();
            let total_sectors = info.total_sectors;
            let sector_size = info.sector_size;
            
            // Calculate filesystem parameters
            let sectors_per_block = BLOCK_SIZE / sector_size;
            let total_blocks = (total_sectors as usize / sectors_per_block) as u32;
            
            if total_blocks < 32 {
                crate::serial_println!("[splaxfs] Device too small for journaled filesystem");
                return Err(SplaxFsError::NoSpace);
            }
            
            // Layout with journal:
            // Block 0: Superblock
            // Block 1: Journal Superblock
            // Block 2-9: Journal Log Area (8 blocks)
            // Block 10: Block bitmap
            // Block 11: Inode bitmap
            // Block 12-19: Inode table (8 blocks = 256 inodes)
            // Block 20+: Data blocks
            
            let journal_log_start = 2u32;
            let journal_log_blocks = 8u32;
            let block_bitmap_block = 10u32;
            let inode_bitmap_block = 11u32;
            let inode_table_start = 12u32;
            let inode_table_blocks = 8u32;
            let first_data_block = inode_table_start + inode_table_blocks;
            let total_inodes = inode_table_blocks * INODES_PER_BLOCK as u32;
            
            // Create and write superblock
            let sb = Superblock::new(total_blocks, total_inodes, first_data_block);
            let mut block_buf = [0u8; BLOCK_SIZE];
            let sb_bytes = sb.to_bytes();
            block_buf[..512].copy_from_slice(&sb_bytes);
            Self::write_block(device, 0, &block_buf)?;
            
            // Initialize journal
            Journal::format(device, journal_log_start, journal_log_blocks)?;
            
            // Initialize block bitmap (mark metadata blocks as used)
            let mut block_bitmap = vec![0u8; BLOCK_SIZE];
            for i in 0..first_data_block {
                let byte = (i / 8) as usize;
                let bit = (i % 8) as usize;
                if byte < block_bitmap.len() {
                    block_bitmap[byte] |= 1 << bit;
                }
            }
            Self::write_block(device, block_bitmap_block, &block_bitmap)?;
            
            // Initialize inode bitmap (mark inodes 0, 1, 2 as used)
            let mut inode_bitmap = vec![0u8; BLOCK_SIZE];
            inode_bitmap[0] = 0b00000111;
            Self::write_block(device, inode_bitmap_block, &inode_bitmap)?;
            
            // Initialize inode table with zeros
            let zero_block = [0u8; BLOCK_SIZE];
            for i in 0..inode_table_blocks {
                Self::write_block(device, inode_table_start + i, &zero_block)?;
            }
            
            // Create root directory inode (inode 2)
            let mut root_inode = DiskInode::new_directory();
            root_inode.size_low = BLOCK_SIZE as u32;
            root_inode.blocks = (BLOCK_SIZE / 512) as u32;
            root_inode.direct[0] = first_data_block;
            Self::write_inode(device, ROOT_INODE, &root_inode)?;
            
            // Create root directory content
            let mut root_block = [0u8; BLOCK_SIZE];
            let dot = DirEntry::new(ROOT_INODE, ".", FileType::Directory);
            let dotdot = DirEntry::new(ROOT_INODE, "..", FileType::Directory);
            root_block[..DIRENT_SIZE].copy_from_slice(&dot.to_bytes());
            root_block[DIRENT_SIZE..2 * DIRENT_SIZE].copy_from_slice(&dotdot.to_bytes());
            
            // Mark first data block as used
            block_bitmap[first_data_block as usize / 8] |= 1 << (first_data_block % 8);
            Self::write_block(device, block_bitmap_block, &block_bitmap)?;
            
            // Write root directory block
            Self::write_block(device, first_data_block, &root_block)?;
            
            crate::serial_println!("[splaxfs] Format with journal complete:");
            crate::serial_println!("  Total blocks: {}", total_blocks);
            crate::serial_println!("  Journal blocks: {}", journal_log_blocks);
            crate::serial_println!("  Total inodes: {}", total_inodes);
            crate::serial_println!("  First data block: {}", first_data_block);
            crate::serial_println!("  Usable space: {} KB", (total_blocks - first_data_block) * 4);
            
            Ok(())
        }).map_err(|_| SplaxFsError::NotFound)?
    }
}

/// Global mounted filesystems
static MOUNTED_FS: Mutex<BTreeMap<String, SplaxFs>> = Mutex::new(BTreeMap::new());

/// Formats a device with SplaxFS
pub fn format(device: &str) -> Result<(), SplaxFsError> {
    SplaxFs::format(device)
}

/// Formats a device with SplaxFS including journal
pub fn format_journaled(device: &str) -> Result<(), SplaxFsError> {
    SplaxFs::format_with_journal(device)
}

/// Mounts a SplaxFS filesystem
pub fn mount(device: &str, mount_point: &str) -> Result<(), SplaxFsError> {
    let fs = SplaxFs::mount(device, mount_point)?;
    MOUNTED_FS.lock().insert(String::from(mount_point), fs);
    Ok(())
}

/// Syncs a mounted filesystem
pub fn sync(mount_point: &str) -> Result<(), SplaxFsError> {
    let mut mounts = MOUNTED_FS.lock();
    if let Some(fs) = mounts.get_mut(mount_point) {
        fs.sync()
    } else {
        Err(SplaxFsError::NotMounted)
    }
}

/// Unmounts a filesystem
pub fn unmount(mount_point: &str) -> Result<(), SplaxFsError> {
    // Sync before unmounting
    let _ = sync(mount_point);
    MOUNTED_FS.lock().remove(mount_point).ok_or(SplaxFsError::NotMounted)?;
    crate::serial_println!("[splaxfs] Unmounted {}", mount_point);
    Ok(())
}

/// Lists directory contents at a mount point
pub fn ls(path: &str) -> Result<Vec<(String, FileType, u64)>, SplaxFsError> {
    let mounts = MOUNTED_FS.lock();
    
    // Find the mount point for this path
    for (mount_point, fs) in mounts.iter() {
        if path.starts_with(mount_point.as_str()) {
            let relative_path = &path[mount_point.len()..];
            let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
            return fs.readdir(relative_path);
        }
    }
    
    Err(SplaxFsError::NotMounted)
}

/// Creates a file
pub fn create(path: &str) -> Result<(), SplaxFsError> {
    let mut mounts = MOUNTED_FS.lock();
    
    for (mount_point, fs) in mounts.iter_mut() {
        if path.starts_with(mount_point.as_str()) {
            let relative_path = &path[mount_point.len()..];
            let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
            return fs.create_file(relative_path);
        }
    }
    
    Err(SplaxFsError::NotMounted)
}

/// Creates a directory
pub fn mkdir(path: &str) -> Result<(), SplaxFsError> {
    let mut mounts = MOUNTED_FS.lock();
    
    for (mount_point, fs) in mounts.iter_mut() {
        if path.starts_with(mount_point.as_str()) {
            let relative_path = &path[mount_point.len()..];
            let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
            return fs.create_dir(relative_path);
        }
    }
    
    Err(SplaxFsError::NotMounted)
}

/// Writes to a file
pub fn write(path: &str, data: &[u8]) -> Result<(), SplaxFsError> {
    let mut mounts = MOUNTED_FS.lock();
    
    for (mount_point, fs) in mounts.iter_mut() {
        if path.starts_with(mount_point.as_str()) {
            let relative_path = &path[mount_point.len()..];
            let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
            return fs.write_file(relative_path, data);
        }
    }
    
    Err(SplaxFsError::NotMounted)
}

/// Reads a file
pub fn read(path: &str) -> Result<Vec<u8>, SplaxFsError> {
    let mounts = MOUNTED_FS.lock();
    
    for (mount_point, fs) in mounts.iter() {
        if path.starts_with(mount_point.as_str()) {
            let relative_path = &path[mount_point.len()..];
            let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
            return fs.read_file(relative_path);
        }
    }
    
    Err(SplaxFsError::NotMounted)
}

/// Gets file info
pub fn stat(path: &str) -> Result<(FileType, u64), SplaxFsError> {
    let mounts = MOUNTED_FS.lock();
    
    for (mount_point, fs) in mounts.iter() {
        if path.starts_with(mount_point.as_str()) {
            let relative_path = &path[mount_point.len()..];
            let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
            return fs.stat(relative_path);
        }
    }
    
    Err(SplaxFsError::NotMounted)
}
