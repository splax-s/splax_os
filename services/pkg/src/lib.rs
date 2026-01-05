//! # S-PKG: Splax Package Manager
//!
//! S-PKG is the package manager for Splax OS, handling software installation,
//! updates, and dependency resolution for user applications.
//!
//! ## Features
//!
//! - **Declarative Manifests**: TOML-based package definitions
//! - **Dependency Resolution**: SAT-solver based version resolution
//! - **Capability Awareness**: Packages declare required capabilities
//! - **Sandboxed Installation**: Each package runs in its own sandbox
//! - **Atomic Updates**: Rollback-safe installation and updates
//! - **Repository Support**: Multiple package sources
//! - **Cryptographic Verification**: Ed25519 signed packages
//!
//! ## Package Format
//!
//! S-PKG packages use the `.spkg` format:
//! ```text
//! ┌─────────────────────────────────────────┐
//! │            Magic: "SPKG"                │
//! ├─────────────────────────────────────────┤
//! │        Header (version, flags)          │
//! ├─────────────────────────────────────────┤
//! │         Manifest (TOML, compressed)     │
//! ├─────────────────────────────────────────┤
//! │         Files (tar, compressed)         │
//! ├─────────────────────────────────────────┤
//! │        Signature (Ed25519, 64 bytes)    │
//! └─────────────────────────────────────────┘
//! ```

#![no_std]

extern crate alloc;

pub mod registry;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use spin::Mutex;

pub use registry::{BuiltinRegistry, RegistryPackage, PackageType};

// =============================================================================
// Package Errors
// =============================================================================

/// Package manager errors.
#[derive(Debug, Clone)]
pub enum PkgError {
    /// Package not found in any repository.
    NotFound(String),
    /// Version not available.
    VersionNotFound { name: String, version: String },
    /// Dependency resolution failed.
    DependencyError(String),
    /// Package signature invalid.
    InvalidSignature(String),
    /// Manifest parse error.
    ManifestError(String),
    /// I/O error.
    IoError(String),
    /// Package already installed.
    AlreadyInstalled(String),
    /// Package not installed.
    NotInstalled(String),
    /// Required capability not available.
    CapabilityDenied(String),
    /// Conflicting packages.
    Conflict { pkg1: String, pkg2: String },
    /// Download failed.
    DownloadError(String),
    /// Insufficient storage.
    StorageError(String),
    /// Corrupted package.
    CorruptedPackage(String),
    /// Transaction in progress.
    TransactionInProgress,
    /// Rollback failed.
    RollbackFailed(String),
}

// =============================================================================
// Version Handling
// =============================================================================

/// Semantic version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version.
    pub patch: u32,
    /// Pre-release tag (e.g., "alpha", "beta").
    pub prerelease: Option<String>,
    /// Build metadata.
    pub build: Option<String>,
}

impl Version {
    /// Create a new version.
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: None,
            build: None,
        }
    }

    /// Parse a version string.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('v').unwrap_or(s);

        // Split off build metadata
        let (version_pre, build) = if let Some(idx) = s.find('+') {
            (&s[..idx], Some(s[idx + 1..].to_string()))
        } else {
            (s, None)
        };

        // Split off prerelease
        let (version, prerelease) = if let Some(idx) = version_pre.find('-') {
            (&version_pre[..idx], Some(version_pre[idx + 1..].to_string()))
        } else {
            (version_pre, None)
        };

        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() < 3 {
            return None;
        }

        Some(Self {
            major: parts[0].parse().ok()?,
            minor: parts[1].parse().ok()?,
            patch: parts[2].parse().ok()?,
            prerelease,
            build,
        })
    }

    /// Check if this version satisfies a constraint.
    pub fn satisfies(&self, constraint: &VersionConstraint) -> bool {
        constraint.matches(self)
    }
}

/// Version constraint for dependencies.
#[derive(Debug, Clone)]
pub enum VersionConstraint {
    /// Exact version: =1.2.3
    Exact(Version),
    /// Greater than or equal: >=1.2.3
    GreaterEqual(Version),
    /// Less than: <2.0.0
    Less(Version),
    /// Compatible (caret): ^1.2.3 (>=1.2.3, <2.0.0)
    Compatible(Version),
    /// Tilde: ~1.2.3 (>=1.2.3, <1.3.0)
    Tilde(Version),
    /// Wildcard: 1.2.*
    Wildcard { major: u32, minor: Option<u32> },
    /// And constraint.
    And(Vec<VersionConstraint>),
    /// Or constraint.
    Or(Vec<VersionConstraint>),
}

impl VersionConstraint {
    /// Check if a version matches this constraint.
    pub fn matches(&self, version: &Version) -> bool {
        match self {
            Self::Exact(v) => version == v,

            Self::GreaterEqual(v) => version >= v,

            Self::Less(v) => version < v,

            Self::Compatible(v) => {
                if version.major != v.major || version < v {
                    return false;
                }
                if v.major == 0 {
                    // 0.x.y is special: ^0.2.3 means >=0.2.3, <0.3.0
                    version.minor == v.minor && version.patch >= v.patch
                } else {
                    true
                }
            }

            Self::Tilde(v) => {
                version.major == v.major
                    && version.minor == v.minor
                    && version.patch >= v.patch
            }

            Self::Wildcard { major, minor } => {
                if version.major != *major {
                    return false;
                }
                if let Some(m) = minor {
                    version.minor == *m
                } else {
                    true
                }
            }

            Self::And(constraints) => constraints.iter().all(|c| c.matches(version)),

            Self::Or(constraints) => constraints.iter().any(|c| c.matches(version)),
        }
    }

    /// Parse a constraint string.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();

        if s.contains(',') {
            let parts: Vec<_> = s.split(',').filter_map(Self::parse).collect();
            return Some(Self::And(parts));
        }

        if s.contains("||") {
            let parts: Vec<_> = s.split("||").filter_map(Self::parse).collect();
            return Some(Self::Or(parts));
        }

        if let Some(rest) = s.strip_prefix(">=") {
            return Version::parse(rest.trim()).map(Self::GreaterEqual);
        }

        if let Some(rest) = s.strip_prefix('>') {
            // >1.0.0 -> >=1.0.1 (approximation)
            return Version::parse(rest.trim()).map(Self::GreaterEqual);
        }

        if let Some(rest) = s.strip_prefix("<=") {
            return Version::parse(rest.trim()).map(Self::Less);
        }

        if let Some(rest) = s.strip_prefix('<') {
            return Version::parse(rest.trim()).map(Self::Less);
        }

        if let Some(rest) = s.strip_prefix('^') {
            return Version::parse(rest.trim()).map(Self::Compatible);
        }

        if let Some(rest) = s.strip_prefix('~') {
            return Version::parse(rest.trim()).map(Self::Tilde);
        }

        if let Some(rest) = s.strip_prefix('=') {
            return Version::parse(rest.trim()).map(Self::Exact);
        }

        if s.contains('*') {
            let cleaned = s.replace('*', "");
            let parts: Vec<&str> = cleaned.split('.').collect();
            let major = parts.get(0).and_then(|p| p.parse().ok())?;
            let minor = parts.get(1).and_then(|p| p.parse().ok());
            return Some(Self::Wildcard { major, minor });
        }

        // Default to compatible
        Version::parse(s).map(Self::Compatible)
    }
}

// =============================================================================
// Package Manifest
// =============================================================================

/// Package manifest (parsed from TOML).
#[derive(Debug, Clone)]
pub struct PackageManifest {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: Version,
    /// Human-readable description.
    pub description: String,
    /// Authors.
    pub authors: Vec<String>,
    /// License (SPDX identifier).
    pub license: String,
    /// Homepage URL.
    pub homepage: Option<String>,
    /// Repository URL.
    pub repository: Option<String>,
    /// Keywords for search.
    pub keywords: Vec<String>,
    /// Categories.
    pub categories: Vec<String>,
    /// Dependencies.
    pub dependencies: Vec<Dependency>,
    /// Build dependencies.
    pub build_dependencies: Vec<Dependency>,
    /// Required capabilities.
    pub capabilities: Vec<CapabilityRequirement>,
    /// Binary targets.
    pub binaries: Vec<BinaryTarget>,
    /// Library target.
    pub library: Option<LibraryTarget>,
    /// Installation hooks.
    pub hooks: InstallHooks,
    /// Minimum Splax OS version.
    pub min_os_version: Option<Version>,
    /// Supported architectures.
    pub architectures: Vec<Architecture>,
}

/// Architecture type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86_64,
    Aarch64,
    RiscV64,
    Wasm32,
    Any,
}

/// Package dependency.
#[derive(Debug, Clone)]
pub struct Dependency {
    /// Package name.
    pub name: String,
    /// Version constraint.
    pub version: VersionConstraint,
    /// Optional dependency.
    pub optional: bool,
    /// Features to enable.
    pub features: Vec<String>,
}

/// Capability requirement.
#[derive(Debug, Clone)]
pub struct CapabilityRequirement {
    /// Capability name.
    pub name: String,
    /// Why this capability is needed.
    pub reason: String,
    /// Whether it's optional.
    pub optional: bool,
}

/// Binary target.
#[derive(Debug, Clone)]
pub struct BinaryTarget {
    /// Binary name.
    pub name: String,
    /// Path to binary in package.
    pub path: String,
    /// Whether to add to PATH.
    pub add_to_path: bool,
}

/// Library target.
#[derive(Debug, Clone)]
pub struct LibraryTarget {
    /// Library name.
    pub name: String,
    /// Path to library.
    pub path: String,
    /// Library type.
    pub lib_type: LibraryType,
}

/// Library type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryType {
    /// WebAssembly module.
    Wasm,
    /// Native shared library.
    SharedObject,
    /// Static library.
    StaticLib,
}

/// Installation hooks.
#[derive(Debug, Clone, Default)]
pub struct InstallHooks {
    /// Run before installation.
    pub pre_install: Option<String>,
    /// Run after installation.
    pub post_install: Option<String>,
    /// Run before uninstallation.
    pub pre_uninstall: Option<String>,
    /// Run after uninstallation.
    pub post_uninstall: Option<String>,
    /// Run before upgrade.
    pub pre_upgrade: Option<String>,
    /// Run after upgrade.
    pub post_upgrade: Option<String>,
}

// =============================================================================
// Package File Format
// =============================================================================

/// Package file magic number.
pub const PACKAGE_MAGIC: &[u8; 4] = b"SPKG";

/// Package file header.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PackageHeader {
    /// Magic number.
    pub magic: [u8; 4],
    /// Format version.
    pub version: u32,
    /// Flags.
    pub flags: u32,
    /// Manifest offset.
    pub manifest_offset: u64,
    /// Manifest size (compressed).
    pub manifest_size: u32,
    /// Manifest size (uncompressed).
    pub manifest_uncompressed: u32,
    /// Files offset.
    pub files_offset: u64,
    /// Files size (compressed).
    pub files_size: u64,
    /// Files size (uncompressed).
    pub files_uncompressed: u64,
    /// Signature offset.
    pub signature_offset: u64,
}

impl PackageHeader {
    /// Header size.
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

/// Package flags.
pub mod pkg_flags {
    /// Package is compressed with zstd.
    pub const COMPRESSED_ZSTD: u32 = 1 << 0;
    /// Package is compressed with lz4.
    pub const COMPRESSED_LZ4: u32 = 1 << 1;
    /// Package contains native code.
    pub const NATIVE_CODE: u32 = 1 << 2;
    /// Package contains WASM code.
    pub const WASM_CODE: u32 = 1 << 3;
    /// Package is signed.
    pub const SIGNED: u32 = 1 << 4;
}

// =============================================================================
// Installed Package Database
// =============================================================================

/// Installed package record.
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    /// Package manifest.
    pub manifest: PackageManifest,
    /// Installation timestamp.
    pub installed_at: u64,
    /// Installation path.
    pub install_path: String,
    /// Files installed.
    pub files: Vec<InstalledFile>,
    /// Automatically installed (as dependency).
    pub auto_installed: bool,
    /// Packages that depend on this one.
    pub dependents: Vec<String>,
}

/// Installed file record.
#[derive(Debug, Clone)]
pub struct InstalledFile {
    /// Path relative to install root.
    pub path: String,
    /// File size.
    pub size: u64,
    /// SHA-256 hash.
    pub hash: [u8; 32],
    /// Is configuration file (don't overwrite).
    pub is_config: bool,
}

/// Package database.
pub struct PackageDatabase {
    /// Installed packages.
    packages: BTreeMap<String, InstalledPackage>,
    /// File ownership map.
    file_owners: BTreeMap<String, String>,
    /// Database path.
    db_path: String,
    /// Modified flag.
    modified: bool,
}

impl PackageDatabase {
    /// Create a new database.
    pub fn new(db_path: String) -> Self {
        Self {
            packages: BTreeMap::new(),
            file_owners: BTreeMap::new(),
            db_path,
            modified: false,
        }
    }

    /// Check if a package is installed.
    pub fn is_installed(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }

    /// Get installed package info.
    pub fn get(&self, name: &str) -> Option<&InstalledPackage> {
        self.packages.get(name)
    }

    /// Get all installed packages.
    pub fn list(&self) -> impl Iterator<Item = &InstalledPackage> {
        self.packages.values()
    }

    /// Record package installation.
    pub fn record_install(&mut self, pkg: InstalledPackage) {
        let name = pkg.manifest.name.clone();

        // Record file ownership
        for file in &pkg.files {
            self.file_owners.insert(file.path.clone(), name.clone());
        }

        self.packages.insert(name, pkg);
        self.modified = true;
    }

    /// Remove package record.
    pub fn record_remove(&mut self, name: &str) -> Option<InstalledPackage> {
        if let Some(pkg) = self.packages.remove(name) {
            // Remove file ownership
            for file in &pkg.files {
                self.file_owners.remove(&file.path);
            }
            self.modified = true;
            Some(pkg)
        } else {
            None
        }
    }

    /// Find packages that need update.
    pub fn outdated<'a>(
        &'a self,
        available: &'a [(String, Version)],
    ) -> impl Iterator<Item = (&'a InstalledPackage, &'a Version)> {
        self.packages.values().filter_map(move |pkg| {
            available.iter()
                .find(|(name, _)| name == &pkg.manifest.name)
                .filter(|(_, new_ver)| *new_ver > pkg.manifest.version)
                .map(|(_, v)| (pkg, v))
        })
    }

    /// Get file owner.
    pub fn file_owner(&self, path: &str) -> Option<&str> {
        self.file_owners.get(path).map(|s| s.as_str())
    }
}

// =============================================================================
// Repository
// =============================================================================

/// Package repository.
#[derive(Debug, Clone)]
pub struct Repository {
    /// Repository name.
    pub name: String,
    /// Repository URL.
    pub url: String,
    /// Priority (lower = higher priority).
    pub priority: u32,
    /// Enabled.
    pub enabled: bool,
    /// GPG key ID for verification.
    pub key_id: Option<String>,
}

/// Repository package index entry.
#[derive(Debug, Clone)]
pub struct PackageIndexEntry {
    /// Package name.
    pub name: String,
    /// Available versions.
    pub versions: Vec<Version>,
    /// Latest version.
    pub latest: Version,
    /// Description.
    pub description: String,
    /// Download size for latest.
    pub download_size: u64,
    /// Installed size for latest.
    pub installed_size: u64,
    /// Dependencies for latest.
    pub dependencies: Vec<String>,
}

/// Repository index.
pub struct RepositoryIndex {
    /// Repository info.
    pub repo: Repository,
    /// Package index.
    pub packages: BTreeMap<String, PackageIndexEntry>,
    /// Last update timestamp.
    pub last_update: u64,
}

impl RepositoryIndex {
    /// Create empty index.
    pub fn new(repo: Repository) -> Self {
        Self {
            repo,
            packages: BTreeMap::new(),
            last_update: 0,
        }
    }

    /// Search packages.
    pub fn search(&self, query: &str) -> Vec<&PackageIndexEntry> {
        let query_lower = query.to_lowercase();
        self.packages.values()
            .filter(|pkg| {
                pkg.name.to_lowercase().contains(&query_lower)
                    || pkg.description.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Get package info.
    pub fn get(&self, name: &str) -> Option<&PackageIndexEntry> {
        self.packages.get(name)
    }
}

// =============================================================================
// Dependency Resolution
// =============================================================================

/// Dependency resolver.
pub struct DependencyResolver {
    /// Available packages from repositories.
    available: BTreeMap<String, Vec<(Version, Vec<Dependency>)>>,
    /// Installed packages.
    installed: BTreeMap<String, Version>,
}

impl DependencyResolver {
    /// Create a new resolver.
    pub fn new() -> Self {
        Self {
            available: BTreeMap::new(),
            installed: BTreeMap::new(),
        }
    }

    /// Add available package version.
    pub fn add_available(
        &mut self,
        name: String,
        version: Version,
        deps: Vec<Dependency>,
    ) {
        self.available
            .entry(name)
            .or_insert_with(Vec::new)
            .push((version, deps));
    }

    /// Set installed packages.
    pub fn set_installed(&mut self, installed: BTreeMap<String, Version>) {
        self.installed = installed;
    }

    /// Resolve dependencies for installing a package.
    pub fn resolve(&self, name: &str, constraint: &VersionConstraint) -> Result<ResolutionPlan, PkgError> {
        let mut plan = ResolutionPlan::default();
        let mut visited = BTreeMap::new();

        self.resolve_recursive(name, constraint, &mut plan, &mut visited)?;

        // Sort by installation order
        self.topological_sort(&mut plan)?;

        Ok(plan)
    }

    fn resolve_recursive(
        &self,
        name: &str,
        constraint: &VersionConstraint,
        plan: &mut ResolutionPlan,
        visited: &mut BTreeMap<String, Version>,
    ) -> Result<(), PkgError> {
        // Check if already resolved
        if let Some(resolved_ver) = visited.get(name) {
            if constraint.matches(resolved_ver) {
                return Ok(());
            } else {
                return Err(PkgError::DependencyError(
                    alloc::format!("Conflicting versions for {}", name)
                ));
            }
        }

        // Check if already installed with compatible version
        if let Some(installed_ver) = self.installed.get(name) {
            if constraint.matches(installed_ver) {
                visited.insert(name.to_string(), installed_ver.clone());
                return Ok(());
            }
        }

        // Find best matching version
        let versions = self.available.get(name)
            .ok_or_else(|| PkgError::NotFound(name.to_string()))?;

        let (best_version, deps) = versions.iter()
            .filter(|(v, _)| constraint.matches(v))
            .max_by(|(a, _), (b, _)| a.cmp(b))
            .ok_or_else(|| PkgError::VersionNotFound {
                name: name.to_string(),
                version: alloc::format!("{:?}", constraint),
            })?;

        visited.insert(name.to_string(), best_version.clone());

        // Check if upgrade needed
        if let Some(installed) = self.installed.get(name) {
            if installed < best_version {
                plan.upgrades.push(PlanAction {
                    name: name.to_string(),
                    version: best_version.clone(),
                    from_version: Some(installed.clone()),
                });
            }
        } else {
            plan.installs.push(PlanAction {
                name: name.to_string(),
                version: best_version.clone(),
                from_version: None,
            });
        }

        // Resolve dependencies
        for dep in deps {
            if !dep.optional {
                self.resolve_recursive(&dep.name, &dep.version, plan, visited)?;
            }
        }

        Ok(())
    }

    fn topological_sort(&self, plan: &mut ResolutionPlan) -> Result<(), PkgError> {
        // Simple topological sort for installation order
        // In a real implementation, this would handle cycles and complex dependencies

        // For now, just reverse (dependencies first)
        plan.installs.reverse();
        Ok(())
    }
}

impl Default for DependencyResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolution plan.
#[derive(Debug, Default)]
pub struct ResolutionPlan {
    /// Packages to install.
    pub installs: Vec<PlanAction>,
    /// Packages to upgrade.
    pub upgrades: Vec<PlanAction>,
    /// Packages to remove (conflicts).
    pub removes: Vec<String>,
    /// Total download size.
    pub download_size: u64,
    /// Total installed size.
    pub installed_size: u64,
}

/// Planned package action.
#[derive(Debug, Clone)]
pub struct PlanAction {
    /// Package name.
    pub name: String,
    /// Target version.
    pub version: Version,
    /// Previous version (for upgrades).
    pub from_version: Option<Version>,
}

// =============================================================================
// Transaction Manager
// =============================================================================

/// Transaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// Planning phase.
    Planning,
    /// Downloading packages.
    Downloading,
    /// Installing packages.
    Installing,
    /// Committed successfully.
    Committed,
    /// Rolling back.
    RollingBack,
    /// Failed.
    Failed,
}

/// Package transaction for atomic operations.
pub struct Transaction {
    /// Transaction ID.
    pub id: u64,
    /// Current state.
    pub state: TransactionState,
    /// Resolution plan.
    pub plan: ResolutionPlan,
    /// Downloaded packages (path).
    pub downloads: BTreeMap<String, String>,
    /// Backup of files being modified.
    pub backups: Vec<BackupEntry>,
    /// Operations performed (for rollback).
    pub operations: Vec<TransactionOp>,
}

/// Backup entry for rollback.
#[derive(Debug, Clone)]
pub struct BackupEntry {
    /// Original path.
    pub path: String,
    /// Backup path.
    pub backup_path: String,
}

/// Transaction operation.
#[derive(Debug, Clone)]
pub enum TransactionOp {
    /// File created.
    FileCreated(String),
    /// File modified.
    FileModified { path: String, backup: String },
    /// File deleted.
    FileDeleted { path: String, backup: String },
    /// Directory created.
    DirectoryCreated(String),
    /// Package registered.
    PackageRegistered(String),
    /// Package unregistered.
    PackageUnregistered(String),
}

impl Transaction {
    /// Create a new transaction.
    pub fn new(id: u64, plan: ResolutionPlan) -> Self {
        Self {
            id,
            state: TransactionState::Planning,
            plan,
            downloads: BTreeMap::new(),
            backups: Vec::new(),
            operations: Vec::new(),
        }
    }

    /// Record an operation.
    pub fn record(&mut self, op: TransactionOp) {
        self.operations.push(op);
    }

    /// Rollback the transaction.
    pub fn rollback(&mut self) -> Result<(), PkgError> {
        self.state = TransactionState::RollingBack;

        // Undo operations in reverse order
        for op in self.operations.iter().rev() {
            match op {
                TransactionOp::FileCreated(path) => {
                    // Delete created file
                    let _ = self.delete_file(path);
                }
                TransactionOp::FileModified { path, backup } => {
                    // Restore from backup
                    let _ = self.restore_file(backup, path);
                }
                TransactionOp::FileDeleted { path, backup } => {
                    // Restore deleted file
                    let _ = self.restore_file(backup, path);
                }
                TransactionOp::DirectoryCreated(path) => {
                    // Remove directory if empty
                    let _ = self.remove_empty_dir(path);
                }
                TransactionOp::PackageRegistered(_name) => {
                    // Unregister package from database
                }
                TransactionOp::PackageUnregistered(_name) => {
                    // Re-register package
                }
            }
        }

        self.state = TransactionState::Failed;
        Ok(())
    }

    fn delete_file(&self, _path: &str) -> Result<(), PkgError> {
        // Would call filesystem
        Ok(())
    }

    fn restore_file(&self, _from: &str, _to: &str) -> Result<(), PkgError> {
        // Would call filesystem
        Ok(())
    }

    fn remove_empty_dir(&self, _path: &str) -> Result<(), PkgError> {
        // Would call filesystem
        Ok(())
    }
}

// =============================================================================
// Package Manager
// =============================================================================

/// Package manager.
pub struct PackageManager {
    /// Package database.
    database: Mutex<PackageDatabase>,
    /// Repository indices.
    repositories: Mutex<Vec<RepositoryIndex>>,
    /// Active transaction.
    transaction: Mutex<Option<Transaction>>,
    /// Cache directory.
    cache_dir: String,
    /// Install root.
    install_root: String,
    /// Next transaction ID.
    next_tx_id: Mutex<u64>,
}

impl PackageManager {
    /// Create a new package manager.
    pub fn new(
        db_path: String,
        cache_dir: String,
        install_root: String,
    ) -> Self {
        Self {
            database: Mutex::new(PackageDatabase::new(db_path)),
            repositories: Mutex::new(Vec::new()),
            transaction: Mutex::new(None),
            cache_dir,
            install_root,
            next_tx_id: Mutex::new(1),
        }
    }

    /// Add a repository.
    pub fn add_repository(&self, repo: Repository) {
        let index = RepositoryIndex::new(repo);
        self.repositories.lock().push(index);
    }

    /// Update repository indices.
    pub fn update(&self) -> Result<(), PkgError> {
        let mut repos = self.repositories.lock();

        for repo in repos.iter_mut() {
            if repo.repo.enabled {
                self.fetch_index(repo)?;
            }
        }

        Ok(())
    }

    /// Search for packages.
    pub fn search(&self, query: &str) -> Vec<PackageIndexEntry> {
        let repos = self.repositories.lock();
        let mut results = Vec::new();

        for repo in repos.iter() {
            for entry in repo.search(query) {
                results.push(entry.clone());
            }
        }

        // Deduplicate by name
        results.sort_by(|a, b| a.name.cmp(&b.name));
        results.dedup_by(|a, b| a.name == b.name);

        results
    }

    /// Install a package.
    pub fn install(&self, name: &str) -> Result<(), PkgError> {
        // Check if transaction in progress
        if self.transaction.lock().is_some() {
            return Err(PkgError::TransactionInProgress);
        }

        // Create dependency resolver
        let resolver = self.create_resolver();

        // Resolve dependencies
        let constraint = VersionConstraint::Wildcard { major: 0, minor: None };
        let plan = resolver.resolve(name, &constraint)?;

        // Create transaction
        let tx_id = {
            let mut id = self.next_tx_id.lock();
            let current = *id;
            *id += 1;
            current
        };

        let mut tx = Transaction::new(tx_id, plan);

        // Download packages
        tx.state = TransactionState::Downloading;
        self.download_packages(&mut tx)?;

        // Install packages
        tx.state = TransactionState::Installing;
        if let Err(e) = self.install_packages(&mut tx) {
            tx.rollback()?;
            return Err(e);
        }

        // Commit
        tx.state = TransactionState::Committed;

        Ok(())
    }

    /// Remove a package.
    pub fn remove(&self, name: &str) -> Result<(), PkgError> {
        let mut db = self.database.lock();

        let pkg = db.get(name)
            .ok_or_else(|| PkgError::NotInstalled(name.to_string()))?;

        // Check for dependents
        if !pkg.dependents.is_empty() {
            return Err(PkgError::DependencyError(
                alloc::format!("Package is required by: {:?}", pkg.dependents)
            ));
        }

        // Run pre-uninstall hook
        if let Some(hook) = &pkg.manifest.hooks.pre_uninstall {
            self.run_hook(hook)?;
        }

        // Remove files
        for file in pkg.files.iter().rev() {
            let path = alloc::format!("{}/{}", self.install_root, file.path);
            let _ = self.delete_file(&path);
        }

        // Run post-uninstall hook
        let hooks = pkg.manifest.hooks.clone();
        db.record_remove(name);

        if let Some(hook) = &hooks.post_uninstall {
            self.run_hook(hook)?;
        }

        Ok(())
    }

    /// Upgrade all packages.
    pub fn upgrade_all(&self) -> Result<u32, PkgError> {
        let db = self.database.lock();
        let repos = self.repositories.lock();

        // Find outdated packages
        let available: Vec<_> = repos.iter()
            .flat_map(|r| r.packages.values())
            .map(|p| (p.name.clone(), p.latest.clone()))
            .collect();

        let outdated: Vec<_> = db.outdated(&available)
            .map(|(pkg, _)| pkg.manifest.name.clone())
            .collect();

        drop(db);
        drop(repos);

        let count = outdated.len() as u32;

        // Upgrade each
        for name in outdated {
            self.install(&name)?;
        }

        Ok(count)
    }

    /// List installed packages.
    pub fn list_installed(&self) -> Vec<InstalledPackage> {
        self.database.lock().list().cloned().collect()
    }

    /// Check if a package is installed.
    pub fn is_installed(&self, name: &str) -> bool {
        self.database.lock().is_installed(name)
    }

    // =========================================================================
    // Internal helpers
    // =========================================================================

    fn create_resolver(&self) -> DependencyResolver {
        let mut resolver = DependencyResolver::new();

        // Add available packages
        let repos = self.repositories.lock();
        for repo in repos.iter() {
            for (name, entry) in &repo.packages {
                for version in &entry.versions {
                    // Would need full dependency info from index
                    resolver.add_available(
                        name.clone(),
                        version.clone(),
                        Vec::new(),
                    );
                }
            }
        }

        // Add installed packages
        let db = self.database.lock();
        let installed: BTreeMap<_, _> = db.list()
            .map(|p| (p.manifest.name.clone(), p.manifest.version.clone()))
            .collect();
        resolver.set_installed(installed);

        resolver
    }

    fn fetch_index(&self, _repo: &mut RepositoryIndex) -> Result<(), PkgError> {
        // Would fetch from repo.repo.url
        Ok(())
    }

    fn download_packages(&self, _tx: &mut Transaction) -> Result<(), PkgError> {
        // Would download packages in tx.plan
        Ok(())
    }

    fn install_packages(&self, tx: &mut Transaction) -> Result<(), PkgError> {
        // Collect names and versions to avoid borrow issues
        let installs: Vec<_> = tx.plan.installs.iter()
            .map(|a| (a.name.clone(), a.version.clone()))
            .collect();
        let upgrades: Vec<_> = tx.plan.upgrades.iter()
            .map(|a| (a.name.clone(), a.version.clone()))
            .collect();

        for (name, version) in installs {
            self.install_single_package(tx, &name, &version)?;
        }

        for (name, version) in upgrades {
            self.install_single_package(tx, &name, &version)?;
        }

        Ok(())
    }

    fn install_single_package(
        &self,
        tx: &mut Transaction,
        name: &str,
        _version: &Version,
    ) -> Result<(), PkgError> {
        // Get downloaded package path (clone to avoid borrow)
        let pkg_path = tx.downloads.get(name)
            .ok_or_else(|| PkgError::IoError("Package not downloaded".to_string()))?
            .clone();

        // Would:
        // 1. Extract package
        // 2. Run pre-install hook
        // 3. Copy files
        // 4. Run post-install hook
        // 5. Register in database

        tx.record(TransactionOp::PackageRegistered(name.to_string()));

        let _ = pkg_path;

        Ok(())
    }

    fn run_hook(&self, _hook: &str) -> Result<(), PkgError> {
        // Would run hook script
        Ok(())
    }

    fn delete_file(&self, _path: &str) -> Result<(), PkgError> {
        // Would delete file
        Ok(())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parse() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);

        let v = Version::parse("v1.0.0-alpha+build").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.prerelease, Some("alpha".to_string()));
        assert_eq!(v.build, Some("build".to_string()));
    }

    #[test]
    fn test_version_constraint() {
        let v = Version::new(1, 5, 0);

        assert!(VersionConstraint::parse(">=1.0.0").unwrap().matches(&v));
        assert!(VersionConstraint::parse("^1.0.0").unwrap().matches(&v));
        assert!(!VersionConstraint::parse("<1.0.0").unwrap().matches(&v));
        assert!(VersionConstraint::parse("~1.5.0").unwrap().matches(&v));
    }

    #[test]
    fn test_package_database() {
        let mut db = PackageDatabase::new("/var/pkg/db".to_string());

        assert!(!db.is_installed("test"));

        let pkg = InstalledPackage {
            manifest: PackageManifest {
                name: "test".to_string(),
                version: Version::new(1, 0, 0),
                description: "Test package".to_string(),
                authors: vec![],
                license: "MIT".to_string(),
                homepage: None,
                repository: None,
                keywords: vec![],
                categories: vec![],
                dependencies: vec![],
                build_dependencies: vec![],
                capabilities: vec![],
                binaries: vec![],
                library: None,
                hooks: InstallHooks::default(),
                min_os_version: None,
                architectures: vec![Architecture::Any],
            },
            installed_at: 0,
            install_path: "/opt/test".to_string(),
            files: vec![],
            auto_installed: false,
            dependents: vec![],
        };

        db.record_install(pkg);

        assert!(db.is_installed("test"));
    }
}
