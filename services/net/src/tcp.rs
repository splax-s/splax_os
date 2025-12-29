//! # TCP State Machine
//!
//! Userspace TCP implementation for the S-NET service.
//! Moved from kernel to support microkernel architecture.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

/// TCP state machine states (RFC 793)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    /// Initial state
    Closed,
    /// Waiting for connection request
    Listen,
    /// SYN sent, waiting for SYN-ACK
    SynSent,
    /// SYN received, waiting for ACK
    SynReceived,
    /// Connection established
    Established,
    /// FIN sent, waiting for FIN
    FinWait1,
    /// Received ACK of FIN
    FinWait2,
    /// Both sides closing
    Closing,
    /// Waiting for enough time to pass
    TimeWait,
    /// Received FIN, waiting for close
    CloseWait,
    /// Last ACK sent
    LastAck,
}

/// TCP flags
#[derive(Debug, Clone, Copy)]
pub struct TcpFlags(pub u8);

impl TcpFlags {
    pub const FIN: u8 = 0x01;
    pub const SYN: u8 = 0x02;
    pub const RST: u8 = 0x04;
    pub const PSH: u8 = 0x08;
    pub const ACK: u8 = 0x10;
    pub const URG: u8 = 0x20;
    pub const ECE: u8 = 0x40;
    pub const CWR: u8 = 0x80;

    pub fn has(&self, flag: u8) -> bool {
        self.0 & flag != 0
    }
}

/// TCP segment
#[derive(Debug, Clone)]
pub struct TcpSegment {
    /// Source port
    pub src_port: u16,
    /// Destination port
    pub dst_port: u16,
    /// Sequence number
    pub seq_num: u32,
    /// Acknowledgment number
    pub ack_num: u32,
    /// Data offset (header length in 32-bit words)
    pub data_offset: u8,
    /// Flags
    pub flags: TcpFlags,
    /// Window size
    pub window: u16,
    /// Checksum
    pub checksum: u16,
    /// Urgent pointer
    pub urgent_ptr: u16,
    /// Options (if any)
    pub options: Vec<TcpOption>,
    /// Payload data
    pub payload: Vec<u8>,
}

/// TCP options
#[derive(Debug, Clone)]
pub enum TcpOption {
    /// End of options
    End,
    /// No operation
    Nop,
    /// Maximum segment size
    Mss(u16),
    /// Window scale
    WindowScale(u8),
    /// Selective ACK permitted
    SackPermitted,
    /// Selective ACK
    Sack(Vec<(u32, u32)>),
    /// Timestamp
    Timestamp { ts_val: u32, ts_ecr: u32 },
}

/// Transmission Control Block
pub struct Tcb {
    /// Connection state
    pub state: TcpState,
    
    // Send sequence variables
    /// Unacknowledged sequence number
    pub snd_una: u32,
    /// Next send sequence number
    pub snd_nxt: u32,
    /// Send window
    pub snd_wnd: u16,
    /// Initial send sequence number
    pub iss: u32,
    
    // Receive sequence variables
    /// Next expected receive sequence number
    pub rcv_nxt: u32,
    /// Receive window
    pub rcv_wnd: u16,
    /// Initial receive sequence number
    pub irs: u32,
    
    // Buffers
    /// Send buffer
    pub send_buffer: VecDeque<u8>,
    /// Receive buffer
    pub recv_buffer: VecDeque<u8>,
    /// Retransmission queue
    pub retx_queue: Vec<TcpSegment>,
    
    // Congestion control
    /// Congestion window
    pub cwnd: u32,
    /// Slow start threshold
    pub ssthresh: u32,
    
    // Timers (in ms)
    /// Retransmission timeout
    pub rto: u32,
    /// Round trip time estimate
    pub srtt: u32,
    /// RTT variance
    pub rttvar: u32,
    
    // Options
    /// Maximum segment size
    pub mss: u16,
    /// Window scale (shift count)
    pub window_scale: u8,
    /// SACK permitted
    pub sack_permitted: bool,
    /// Timestamps enabled
    pub timestamps: bool,
}

impl Default for Tcb {
    fn default() -> Self {
        Self {
            state: TcpState::Closed,
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 0,
            iss: 0,
            rcv_nxt: 0,
            rcv_wnd: 65535,
            irs: 0,
            send_buffer: VecDeque::new(),
            recv_buffer: VecDeque::new(),
            retx_queue: Vec::new(),
            cwnd: 1460 * 10, // Initial window (10 MSS)
            ssthresh: 65535,
            rto: 1000, // 1 second initial RTO
            srtt: 0,
            rttvar: 0,
            mss: 1460,
            window_scale: 0,
            sack_permitted: false,
            timestamps: false,
        }
    }
}

impl Tcb {
    /// Generates a new initial sequence number
    pub fn generate_iss() -> u32 {
        // In production: use secure random
        // For now: simple counter-based ISS
        static mut ISS_COUNTER: u32 = 0;
        unsafe {
            ISS_COUNTER = ISS_COUNTER.wrapping_add(64000);
            ISS_COUNTER
        }
    }

    /// Initiates active open (connect)
    pub fn active_open(&mut self) {
        self.iss = Self::generate_iss();
        self.snd_una = self.iss;
        self.snd_nxt = self.iss + 1;
        self.state = TcpState::SynSent;
    }

    /// Initiates passive open (listen)
    pub fn passive_open(&mut self) {
        self.state = TcpState::Listen;
    }

    /// Processes an incoming segment
    pub fn process_segment(&mut self, segment: &TcpSegment) -> Option<TcpSegment> {
        match self.state {
            TcpState::Closed => None,
            
            TcpState::Listen => {
                if segment.flags.has(TcpFlags::SYN) {
                    // Received SYN, send SYN-ACK
                    self.irs = segment.seq_num;
                    self.rcv_nxt = segment.seq_num + 1;
                    self.iss = Self::generate_iss();
                    self.snd_una = self.iss;
                    self.snd_nxt = self.iss + 1;
                    self.state = TcpState::SynReceived;
                    
                    Some(self.create_segment(TcpFlags::SYN | TcpFlags::ACK, &[]))
                } else {
                    None
                }
            }
            
            TcpState::SynSent => {
                if segment.flags.has(TcpFlags::SYN) && segment.flags.has(TcpFlags::ACK) {
                    // Received SYN-ACK
                    self.irs = segment.seq_num;
                    self.rcv_nxt = segment.seq_num + 1;
                    self.snd_una = segment.ack_num;
                    self.state = TcpState::Established;
                    
                    Some(self.create_segment(TcpFlags::ACK, &[]))
                } else {
                    None
                }
            }
            
            TcpState::SynReceived => {
                if segment.flags.has(TcpFlags::ACK) {
                    self.snd_una = segment.ack_num;
                    self.state = TcpState::Established;
                }
                None
            }
            
            TcpState::Established => {
                self.process_established(segment)
            }
            
            TcpState::FinWait1 => {
                if segment.flags.has(TcpFlags::ACK) {
                    self.state = TcpState::FinWait2;
                }
                if segment.flags.has(TcpFlags::FIN) {
                    self.rcv_nxt = segment.seq_num + 1;
                    self.state = TcpState::Closing;
                    Some(self.create_segment(TcpFlags::ACK, &[]))
                } else {
                    None
                }
            }
            
            TcpState::FinWait2 => {
                if segment.flags.has(TcpFlags::FIN) {
                    self.rcv_nxt = segment.seq_num + 1;
                    self.state = TcpState::TimeWait;
                    Some(self.create_segment(TcpFlags::ACK, &[]))
                } else {
                    None
                }
            }
            
            TcpState::CloseWait => {
                // Application closes, we send FIN
                None
            }
            
            TcpState::LastAck => {
                if segment.flags.has(TcpFlags::ACK) {
                    self.state = TcpState::Closed;
                }
                None
            }
            
            TcpState::Closing => {
                if segment.flags.has(TcpFlags::ACK) {
                    self.state = TcpState::TimeWait;
                }
                None
            }
            
            TcpState::TimeWait => {
                // Wait for 2*MSL then close
                None
            }
        }
    }

    /// Processes a segment in established state
    fn process_established(&mut self, segment: &TcpSegment) -> Option<TcpSegment> {
        // Check sequence number
        if !self.is_valid_sequence(segment) {
            return Some(self.create_segment(TcpFlags::ACK, &[]));
        }

        // Process ACK
        if segment.flags.has(TcpFlags::ACK) {
            self.process_ack(segment.ack_num);
        }

        // Process data
        if !segment.payload.is_empty() {
            self.recv_buffer.extend(&segment.payload);
            self.rcv_nxt = self.rcv_nxt.wrapping_add(segment.payload.len() as u32);
        }

        // Process FIN
        if segment.flags.has(TcpFlags::FIN) {
            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
            self.state = TcpState::CloseWait;
            return Some(self.create_segment(TcpFlags::ACK, &[]));
        }

        // Send ACK if we received data
        if !segment.payload.is_empty() {
            Some(self.create_segment(TcpFlags::ACK, &[]))
        } else {
            None
        }
    }

    /// Checks if sequence number is valid
    fn is_valid_sequence(&self, segment: &TcpSegment) -> bool {
        let seg_len = segment.payload.len() as u32;
        let seg_seq = segment.seq_num;
        let rcv_nxt = self.rcv_nxt;
        let rcv_wnd = self.rcv_wnd as u32;

        if seg_len == 0 {
            if rcv_wnd == 0 {
                seg_seq == rcv_nxt
            } else {
                rcv_nxt <= seg_seq && seg_seq < rcv_nxt.wrapping_add(rcv_wnd)
            }
        } else if rcv_wnd == 0 {
            false
        } else {
            let seg_end = seg_seq.wrapping_add(seg_len - 1);
            (rcv_nxt <= seg_seq && seg_seq < rcv_nxt.wrapping_add(rcv_wnd))
                || (rcv_nxt <= seg_end && seg_end < rcv_nxt.wrapping_add(rcv_wnd))
        }
    }

    /// Processes an ACK
    fn process_ack(&mut self, ack_num: u32) {
        if self.snd_una < ack_num && ack_num <= self.snd_nxt {
            // Valid ACK - update snd_una
            let acked_bytes = ack_num.wrapping_sub(self.snd_una);
            self.snd_una = ack_num;
            
            // Remove acked data from retx queue
            self.retx_queue.retain(|seg| {
                seg.seq_num.wrapping_add(seg.payload.len() as u32) > ack_num
            });
            
            // Update congestion window
            if self.cwnd < self.ssthresh {
                // Slow start
                self.cwnd += self.mss as u32;
            } else {
                // Congestion avoidance
                self.cwnd += (self.mss as u32 * self.mss as u32) / self.cwnd;
            }
            
            let _ = acked_bytes; // Use the variable
        }
    }

    /// Creates an outgoing segment
    pub fn create_segment(&self, flags: u8, data: &[u8]) -> TcpSegment {
        TcpSegment {
            src_port: 0, // Filled by caller
            dst_port: 0, // Filled by caller
            seq_num: self.snd_nxt,
            ack_num: self.rcv_nxt,
            data_offset: 5,
            flags: TcpFlags(flags),
            window: self.rcv_wnd,
            checksum: 0, // Calculated later
            urgent_ptr: 0,
            options: Vec::new(),
            payload: data.to_vec(),
        }
    }

    /// Initiates close
    pub fn close(&mut self) -> Option<TcpSegment> {
        match self.state {
            TcpState::Established => {
                self.state = TcpState::FinWait1;
                Some(self.create_segment(TcpFlags::FIN | TcpFlags::ACK, &[]))
            }
            TcpState::CloseWait => {
                self.state = TcpState::LastAck;
                Some(self.create_segment(TcpFlags::FIN | TcpFlags::ACK, &[]))
            }
            _ => None,
        }
    }

    /// Sends data
    pub fn send(&mut self, data: &[u8]) -> Result<usize, &'static str> {
        if self.state != TcpState::Established {
            return Err("Connection not established");
        }

        self.send_buffer.extend(data);
        Ok(data.len())
    }

    /// Receives data
    pub fn recv(&mut self, buf: &mut [u8]) -> usize {
        let len = buf.len().min(self.recv_buffer.len());
        for (i, byte) in self.recv_buffer.drain(..len).enumerate() {
            buf[i] = byte;
        }
        len
    }
}

/// TCP connection manager
pub struct TcpManager {
    /// Active connections: (local_port, remote_ip, remote_port) -> TCB
    connections: BTreeMap<(u16, u32, u16), Tcb>,
    /// Listening sockets: port -> backlog
    listeners: BTreeMap<u16, Vec<Tcb>>,
}

impl TcpManager {
    /// Creates a new TCP manager
    pub fn new() -> Self {
        Self {
            connections: BTreeMap::new(),
            listeners: BTreeMap::new(),
        }
    }

    /// Starts listening on a port
    pub fn listen(&mut self, port: u16, backlog: usize) {
        self.listeners.insert(port, Vec::with_capacity(backlog));
    }

    /// Initiates a connection
    pub fn connect(&mut self, local_port: u16, remote_ip: u32, remote_port: u16) -> Result<(), &'static str> {
        let mut tcb = Tcb::default();
        tcb.active_open();
        self.connections.insert((local_port, remote_ip, remote_port), tcb);
        Ok(())
    }

    /// Gets a connection
    pub fn get_connection(&mut self, local_port: u16, remote_ip: u32, remote_port: u16) -> Option<&mut Tcb> {
        self.connections.get_mut(&(local_port, remote_ip, remote_port))
    }
}

impl Default for TcpManager {
    fn default() -> Self {
        Self::new()
    }
}
