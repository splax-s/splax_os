//! # Built-in Package Registry
//!
//! This module provides a built-in package registry with common Unix tools
//! implemented natively for Splax OS.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

// =============================================================================
// Registry Package Definition
// =============================================================================

/// A package definition in the registry.
#[derive(Debug, Clone)]
pub struct RegistryPackage {
    /// Package name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Description.
    pub description: String,
    /// Homepage URL.
    pub homepage: Option<String>,
    /// License.
    pub license: String,
    /// Dependencies.
    pub dependencies: Vec<String>,
    /// Binary names provided.
    pub binaries: Vec<String>,
    /// Size in bytes (estimated).
    pub size: usize,
    /// Package type.
    pub pkg_type: PackageType,
}

/// Package type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageType {
    /// Built-in (native implementation).
    Builtin,
    /// WASM module.
    Wasm,
    /// Native binary.
    Native,
}

impl RegistryPackage {
    /// Create a new builtin package.
    pub fn builtin(
        name: &str,
        version: &str,
        description: &str,
        binaries: &[&str],
    ) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            description: description.to_string(),
            homepage: None,
            license: "MIT OR Apache-2.0".to_string(),
            dependencies: Vec::new(),
            binaries: binaries.iter().map(|s| s.to_string()).collect(),
            size: 0,
            pkg_type: PackageType::Builtin,
        }
    }

    /// Set homepage.
    pub fn with_homepage(mut self, url: &str) -> Self {
        self.homepage = Some(url.to_string());
        self
    }

    /// Set license.
    pub fn with_license(mut self, license: &str) -> Self {
        self.license = license.to_string();
        self
    }

    /// Set dependencies.
    pub fn with_deps(mut self, deps: &[&str]) -> Self {
        self.dependencies = deps.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Set size.
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }
}

// =============================================================================
// Built-in Package Registry
// =============================================================================

/// The built-in package registry.
pub struct BuiltinRegistry {
    /// Available packages.
    packages: BTreeMap<String, RegistryPackage>,
}

impl BuiltinRegistry {
    /// Create the registry with all built-in packages.
    pub fn new() -> Self {
        let mut packages = BTreeMap::new();

        // Add all built-in packages
        for pkg in Self::builtin_packages() {
            packages.insert(pkg.name.clone(), pkg);
        }

        Self { packages }
    }

    /// Get all built-in package definitions.
    fn builtin_packages() -> Vec<RegistryPackage> {
        vec![
            // ===== Text Editors =====
            RegistryPackage::builtin(
                "vim",
                "9.1.0",
                "Vi IMproved - enhanced vi text editor with syntax highlighting",
                &["vim", "vi", "vimdiff", "view"],
            )
            .with_homepage("https://www.vim.org")
            .with_license("Vim License")
            .with_size(15_000_000),

            RegistryPackage::builtin(
                "nano",
                "7.2.0",
                "Simple, easy-to-use terminal text editor",
                &["nano", "rnano"],
            )
            .with_homepage("https://nano-editor.org")
            .with_license("GPL-3.0")
            .with_size(2_500_000),

            RegistryPackage::builtin(
                "ed",
                "1.19.0",
                "The standard Unix line editor",
                &["ed", "red"],
            )
            .with_license("GPL-3.0")
            .with_size(100_000),

            // ===== File Utilities =====
            RegistryPackage::builtin(
                "coreutils",
                "0.1.0",
                "Core utilities: cat, ls, cp, mv, rm, mkdir, etc.",
                &["cat", "ls", "cp", "mv", "rm", "mkdir", "rmdir", "pwd", 
                  "echo", "head", "tail", "wc", "sort", "uniq", "cut",
                  "paste", "join", "split", "tr", "tee", "touch", "chmod",
                  "chown", "ln", "readlink", "basename", "dirname", "realpath",
                  "date", "sleep", "true", "false", "yes", "seq", "env",
                  "printenv", "whoami", "id", "groups", "uname"],
            )
            .with_size(5_000_000),

            RegistryPackage::builtin(
                "less",
                "643.0.0",
                "Opposite of more - file pager with backward navigation",
                &["less", "lesskey"],
            )
            .with_homepage("https://greenwoodsoftware.com/less")
            .with_license("GPL-3.0 OR BSD-2-Clause")
            .with_size(500_000),

            RegistryPackage::builtin(
                "more",
                "0.1.0",
                "Simple file pager",
                &["more"],
            )
            .with_size(50_000),

            // ===== Text Processing =====
            RegistryPackage::builtin(
                "grep",
                "3.11.0",
                "Search files for lines matching a pattern",
                &["grep", "egrep", "fgrep"],
            )
            .with_homepage("https://www.gnu.org/software/grep")
            .with_license("GPL-3.0")
            .with_size(800_000),

            RegistryPackage::builtin(
                "sed",
                "4.9.0",
                "Stream editor for filtering and transforming text",
                &["sed"],
            )
            .with_homepage("https://www.gnu.org/software/sed")
            .with_license("GPL-3.0")
            .with_size(400_000),

            RegistryPackage::builtin(
                "awk",
                "5.3.0",
                "Pattern scanning and processing language",
                &["awk", "gawk"],
            )
            .with_homepage("https://www.gnu.org/software/gawk")
            .with_license("GPL-3.0")
            .with_size(1_200_000),

            // ===== File Finding =====
            RegistryPackage::builtin(
                "findutils",
                "4.9.0",
                "Find files and execute commands on them",
                &["find", "xargs", "locate", "updatedb"],
            )
            .with_homepage("https://www.gnu.org/software/findutils")
            .with_license("GPL-3.0")
            .with_size(600_000),

            // ===== Compression =====
            RegistryPackage::builtin(
                "gzip",
                "1.13.0",
                "GNU compression utility",
                &["gzip", "gunzip", "zcat"],
            )
            .with_homepage("https://www.gnu.org/software/gzip")
            .with_license("GPL-3.0")
            .with_size(300_000),

            RegistryPackage::builtin(
                "tar",
                "1.35.0",
                "Tape archive utility",
                &["tar"],
            )
            .with_homepage("https://www.gnu.org/software/tar")
            .with_license("GPL-3.0")
            .with_size(800_000),

            RegistryPackage::builtin(
                "zip",
                "3.0.0",
                "Compression and file packaging utility",
                &["zip", "unzip", "zipinfo"],
            )
            .with_size(500_000),

            RegistryPackage::builtin(
                "xz",
                "5.4.4",
                "XZ compression utility",
                &["xz", "unxz", "xzcat", "lzma", "unlzma"],
            )
            .with_homepage("https://tukaani.org/xz")
            .with_license("LGPL-2.1 OR GPL-2.0")
            .with_size(400_000),

            RegistryPackage::builtin(
                "zstd",
                "1.5.5",
                "Zstandard fast compression algorithm",
                &["zstd", "zstdcat", "unzstd", "zstdmt"],
            )
            .with_homepage("https://facebook.github.io/zstd")
            .with_license("BSD-3-Clause OR GPL-2.0")
            .with_size(600_000),

            // ===== Networking =====
            RegistryPackage::builtin(
                "curl",
                "8.5.0",
                "Command line tool for transferring data with URLs",
                &["curl"],
            )
            .with_homepage("https://curl.se")
            .with_license("curl")
            .with_size(3_000_000),

            RegistryPackage::builtin(
                "wget",
                "1.21.4",
                "Network downloader",
                &["wget"],
            )
            .with_homepage("https://www.gnu.org/software/wget")
            .with_license("GPL-3.0")
            .with_size(1_500_000),

            RegistryPackage::builtin(
                "netcat",
                "1.10.0",
                "TCP/IP networking utility",
                &["nc", "netcat"],
            )
            .with_size(100_000),

            RegistryPackage::builtin(
                "ssh",
                "9.5.0",
                "Secure shell client and server",
                &["ssh", "scp", "sftp", "ssh-keygen", "ssh-add", "ssh-agent"],
            )
            .with_homepage("https://www.openssh.com")
            .with_license("BSD-2-Clause")
            .with_size(4_000_000),

            RegistryPackage::builtin(
                "rsync",
                "3.2.7",
                "Fast, versatile remote file sync",
                &["rsync"],
            )
            .with_homepage("https://rsync.samba.org")
            .with_license("GPL-3.0")
            .with_size(1_000_000),

            // ===== Version Control =====
            RegistryPackage::builtin(
                "git",
                "2.43.0",
                "Distributed version control system",
                &["git"],
            )
            .with_homepage("https://git-scm.com")
            .with_license("GPL-2.0")
            .with_size(30_000_000),

            // ===== Development =====
            RegistryPackage::builtin(
                "make",
                "4.4.1",
                "GNU Make build automation tool",
                &["make", "gmake"],
            )
            .with_homepage("https://www.gnu.org/software/make")
            .with_license("GPL-3.0")
            .with_size(800_000),

            RegistryPackage::builtin(
                "diffutils",
                "3.10.0",
                "File comparison utilities",
                &["diff", "cmp", "diff3", "sdiff"],
            )
            .with_homepage("https://www.gnu.org/software/diffutils")
            .with_license("GPL-3.0")
            .with_size(500_000),

            RegistryPackage::builtin(
                "patch",
                "2.7.6",
                "Apply diff patches to files",
                &["patch"],
            )
            .with_homepage("https://www.gnu.org/software/patch")
            .with_license("GPL-3.0")
            .with_size(200_000),

            // ===== System Utilities =====
            RegistryPackage::builtin(
                "htop",
                "3.3.0",
                "Interactive process viewer",
                &["htop"],
            )
            .with_homepage("https://htop.dev")
            .with_license("GPL-2.0")
            .with_size(500_000),

            RegistryPackage::builtin(
                "tree",
                "2.1.1",
                "Display directory tree structure",
                &["tree"],
            )
            .with_homepage("http://mama.indstate.edu/users/ice/tree")
            .with_license("GPL-2.0")
            .with_size(100_000),

            RegistryPackage::builtin(
                "file",
                "5.45.0",
                "Determine file type",
                &["file"],
            )
            .with_size(500_000),

            RegistryPackage::builtin(
                "bc",
                "6.7.3",
                "Arbitrary precision calculator",
                &["bc", "dc"],
            )
            .with_homepage("https://git.gavinhoward.com/gavin/bc")
            .with_license("BSD-2-Clause")
            .with_size(300_000),

            // ===== Shells =====
            RegistryPackage::builtin(
                "bash",
                "5.2.21",
                "Bourne Again SHell",
                &["bash", "sh"],
            )
            .with_homepage("https://www.gnu.org/software/bash")
            .with_license("GPL-3.0")
            .with_size(2_000_000),

            RegistryPackage::builtin(
                "zsh",
                "5.9.0",
                "Z shell - extended Bourne shell",
                &["zsh"],
            )
            .with_homepage("https://www.zsh.org")
            .with_license("MIT")
            .with_size(3_000_000),

            RegistryPackage::builtin(
                "fish",
                "3.7.0",
                "Friendly interactive shell",
                &["fish", "fish_indent", "fish_key_reader"],
            )
            .with_homepage("https://fishshell.com")
            .with_license("GPL-2.0")
            .with_size(4_000_000),

            // ===== Scripting Languages =====
            RegistryPackage::builtin(
                "python",
                "3.12.1",
                "Python programming language interpreter",
                &["python", "python3", "pip", "pip3"],
            )
            .with_homepage("https://www.python.org")
            .with_license("PSF-2.0")
            .with_size(50_000_000),

            RegistryPackage::builtin(
                "node",
                "21.5.0",
                "JavaScript runtime built on V8",
                &["node", "npm", "npx"],
            )
            .with_homepage("https://nodejs.org")
            .with_license("MIT")
            .with_size(80_000_000),

            RegistryPackage::builtin(
                "lua",
                "5.4.6",
                "Lightweight scripting language",
                &["lua", "luac"],
            )
            .with_homepage("https://www.lua.org")
            .with_license("MIT")
            .with_size(500_000),

            // ===== JSON/YAML Tools =====
            RegistryPackage::builtin(
                "jq",
                "1.7.1",
                "Command-line JSON processor",
                &["jq"],
            )
            .with_homepage("https://jqlang.github.io/jq")
            .with_license("MIT")
            .with_size(1_000_000),

            RegistryPackage::builtin(
                "yq",
                "4.40.5",
                "Command-line YAML/JSON/XML processor",
                &["yq"],
            )
            .with_homepage("https://mikefarah.gitbook.io/yq")
            .with_license("MIT")
            .with_size(5_000_000),

            // ===== Containers =====
            RegistryPackage::builtin(
                "docker",
                "24.0.7",
                "Container runtime and tools",
                &["docker", "dockerd"],
            )
            .with_homepage("https://www.docker.com")
            .with_license("Apache-2.0")
            .with_size(100_000_000),

            RegistryPackage::builtin(
                "kubectl",
                "1.29.0",
                "Kubernetes command-line tool",
                &["kubectl"],
            )
            .with_homepage("https://kubernetes.io")
            .with_license("Apache-2.0")
            .with_size(50_000_000),

            // ===== Encryption =====
            RegistryPackage::builtin(
                "openssl",
                "3.2.0",
                "Cryptography and TLS toolkit",
                &["openssl"],
            )
            .with_homepage("https://www.openssl.org")
            .with_license("Apache-2.0")
            .with_size(10_000_000),

            RegistryPackage::builtin(
                "gpg",
                "2.4.3",
                "GNU Privacy Guard - encryption and signing",
                &["gpg", "gpg2", "gpg-agent", "gpgv"],
            )
            .with_homepage("https://gnupg.org")
            .with_license("GPL-3.0")
            .with_size(8_000_000),
        ]
    }

    /// Get a package by name.
    pub fn get(&self, name: &str) -> Option<&RegistryPackage> {
        self.packages.get(name)
    }

    /// Search for packages matching a query.
    pub fn search(&self, query: &str) -> Vec<&RegistryPackage> {
        let query_lower = query.to_lowercase();
        
        self.packages.values()
            .filter(|pkg| {
                pkg.name.to_lowercase().contains(&query_lower) ||
                pkg.description.to_lowercase().contains(&query_lower) ||
                pkg.binaries.iter().any(|b| b.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Get all packages.
    pub fn list_all(&self) -> Vec<&RegistryPackage> {
        self.packages.values().collect()
    }

    /// Check if a binary is provided by a package.
    pub fn find_binary_provider(&self, binary: &str) -> Option<&RegistryPackage> {
        self.packages.values()
            .find(|pkg| pkg.binaries.iter().any(|b| b == binary))
    }

    /// Get package count.
    pub fn count(&self) -> usize {
        self.packages.len()
    }
}

impl Default for BuiltinRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_vim() {
        let registry = BuiltinRegistry::new();
        let vim = registry.get("vim").expect("vim should exist");
        assert_eq!(vim.name, "vim");
        assert!(vim.binaries.contains(&"vim".to_string()));
        assert!(vim.binaries.contains(&"vi".to_string()));
    }

    #[test]
    fn test_registry_search() {
        let registry = BuiltinRegistry::new();
        let results = registry.search("editor");
        assert!(!results.is_empty());
    }

    #[test]
    fn test_find_binary_provider() {
        let registry = BuiltinRegistry::new();
        let provider = registry.find_binary_provider("grep");
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().name, "grep");
    }
}
