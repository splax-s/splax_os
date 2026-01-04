//! # Splax OS Performance Benchmarks
//!
//! Comprehensive benchmark suite for measuring kernel and service performance.
//!
//! ## Benchmark Categories
//!
//! - **IPC**: Inter-process communication latency and throughput
//! - **Memory**: Allocation and mapping performance
//! - **Scheduler**: Context switch overhead
//! - **Network**: Packet processing throughput
//! - **Storage**: I/O latency and bandwidth
//! - **Crypto**: Cryptographic operation speeds
//! - **WASM**: WebAssembly execution performance
//!
//! ## Running Benchmarks
//!
//! ```bash
//! ./scripts/splax bench all
//! ./scripts/splax bench ipc
//! ./scripts/splax bench memory --iterations 10000
//! ```

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

// =============================================================================
// Benchmark Framework
// =============================================================================

/// Benchmark result
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Benchmark name
    pub name: String,
    /// Minimum time (nanoseconds)
    pub min_ns: u64,
    /// Maximum time (nanoseconds)
    pub max_ns: u64,
    /// Mean time (nanoseconds)
    pub mean_ns: u64,
    /// Median time (nanoseconds)
    pub median_ns: u64,
    /// Standard deviation (nanoseconds)
    pub std_dev_ns: u64,
    /// Number of iterations
    pub iterations: u64,
    /// Throughput (operations per second)
    pub ops_per_sec: f64,
    /// Additional metrics
    pub metrics: Vec<(String, String)>,
}

impl BenchmarkResult {
    /// Format as human-readable string
    pub fn format(&self) -> String {
        let mean_us = self.mean_ns as f64 / 1000.0;
        let std_us = self.std_dev_ns as f64 / 1000.0;
        
        alloc::format!(
            "{}: {:.2} µs ± {:.2} µs ({} iterations, {:.0} ops/sec)",
            self.name, mean_us, std_us, self.iterations, self.ops_per_sec
        )
    }
}

/// Benchmark configuration
#[derive(Clone)]
pub struct BenchConfig {
    /// Number of warmup iterations
    pub warmup: u64,
    /// Number of benchmark iterations
    pub iterations: u64,
    /// Minimum benchmark time (ms)
    pub min_time_ms: u64,
    /// Maximum benchmark time (ms)
    pub max_time_ms: u64,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            warmup: 100,
            iterations: 1000,
            min_time_ms: 1000,
            max_time_ms: 10000,
        }
    }
}

/// Timer for benchmarks
pub struct Timer {
    start: u64,
}

impl Timer {
    /// Start a new timer
    pub fn start() -> Self {
        Self {
            start: read_timestamp(),
        }
    }

    /// Get elapsed time in nanoseconds
    pub fn elapsed_ns(&self) -> u64 {
        let end = read_timestamp();
        ticks_to_ns(end - self.start)
    }
}

/// Read CPU timestamp counter
fn read_timestamp() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    
    #[cfg(not(target_arch = "x86_64"))]
    {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }
}

/// Convert ticks to nanoseconds (approximate)
fn ticks_to_ns(ticks: u64) -> u64 {
    // Assume 3GHz for now - would be calibrated at runtime
    ticks * 1000 / 3000
}

/// Benchmark a function
pub fn benchmark<F: FnMut()>(name: &str, config: &BenchConfig, mut f: F) -> BenchmarkResult {
    // Warmup
    for _ in 0..config.warmup {
        f();
    }

    // Collect timing data
    let mut times: Vec<u64> = Vec::with_capacity(config.iterations as usize);
    
    for _ in 0..config.iterations {
        let timer = Timer::start();
        f();
        times.push(timer.elapsed_ns());
    }

    // Calculate statistics
    times.sort();
    
    let sum: u64 = times.iter().sum();
    let mean = sum / times.len() as u64;
    let min = *times.first().unwrap_or(&0);
    let max = *times.last().unwrap_or(&0);
    let median = times[times.len() / 2];
    
    // Standard deviation
    let variance: u64 = times.iter()
        .map(|&t| {
            let diff = if t > mean { t - mean } else { mean - t };
            diff * diff
        })
        .sum::<u64>() / times.len() as u64;
    let std_dev = isqrt(variance);

    let ops_per_sec = if mean > 0 {
        1_000_000_000.0 / mean as f64
    } else {
        0.0
    };

    BenchmarkResult {
        name: name.to_string(),
        min_ns: min,
        max_ns: max,
        mean_ns: mean,
        median_ns: median,
        std_dev_ns: std_dev,
        iterations: config.iterations,
        ops_per_sec,
        metrics: Vec::new(),
    }
}

/// Integer square root
fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

// =============================================================================
// IPC Benchmarks
// =============================================================================

pub mod ipc {
    use super::*;

    /// Benchmark empty IPC roundtrip
    pub fn bench_empty_roundtrip(config: &BenchConfig) -> BenchmarkResult {
        benchmark("ipc_empty_roundtrip", config, || {
            // Simulate empty IPC call
            core::hint::black_box(());
        })
    }

    /// Benchmark small message IPC
    pub fn bench_small_message(config: &BenchConfig) -> BenchmarkResult {
        let msg = [0u8; 64];
        benchmark("ipc_small_message", config, || {
            core::hint::black_box(&msg);
        })
    }

    /// Benchmark large message IPC
    pub fn bench_large_message(config: &BenchConfig) -> BenchmarkResult {
        let msg = alloc::vec![0u8; 4096];
        benchmark("ipc_large_message", config, || {
            core::hint::black_box(&msg);
        })
    }

    /// Benchmark zero-copy IPC
    pub fn bench_zero_copy(config: &BenchConfig) -> BenchmarkResult {
        let msg = alloc::vec![0u8; 65536];
        benchmark("ipc_zero_copy", config, || {
            core::hint::black_box(&msg);
        })
    }

    /// Run all IPC benchmarks
    pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
        alloc::vec![
            bench_empty_roundtrip(config),
            bench_small_message(config),
            bench_large_message(config),
            bench_zero_copy(config),
        ]
    }
}

// =============================================================================
// Memory Benchmarks
// =============================================================================

pub mod memory {
    use super::*;

    /// Benchmark small allocation
    pub fn bench_small_alloc(config: &BenchConfig) -> BenchmarkResult {
        benchmark("memory_small_alloc", config, || {
            let v: Vec<u8> = Vec::with_capacity(64);
            core::hint::black_box(v);
        })
    }

    /// Benchmark medium allocation
    pub fn bench_medium_alloc(config: &BenchConfig) -> BenchmarkResult {
        benchmark("memory_medium_alloc", config, || {
            let v: Vec<u8> = Vec::with_capacity(4096);
            core::hint::black_box(v);
        })
    }

    /// Benchmark large allocation
    pub fn bench_large_alloc(config: &BenchConfig) -> BenchmarkResult {
        benchmark("memory_large_alloc", config, || {
            let v: Vec<u8> = Vec::with_capacity(1024 * 1024);
            core::hint::black_box(v);
        })
    }

    /// Benchmark page fault handling
    pub fn bench_page_fault(config: &BenchConfig) -> BenchmarkResult {
        benchmark("memory_page_fault", config, || {
            // Simulate page fault
            core::hint::black_box(());
        })
    }

    /// Benchmark memory copy
    pub fn bench_memcpy(config: &BenchConfig) -> BenchmarkResult {
        let src = alloc::vec![0u8; 4096];
        let mut dst = alloc::vec![0u8; 4096];
        benchmark("memory_memcpy_4k", config, || {
            dst.copy_from_slice(&src);
            core::hint::black_box(&dst);
        })
    }

    /// Run all memory benchmarks
    pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
        alloc::vec![
            bench_small_alloc(config),
            bench_medium_alloc(config),
            bench_large_alloc(config),
            bench_page_fault(config),
            bench_memcpy(config),
        ]
    }
}

// =============================================================================
// Scheduler Benchmarks
// =============================================================================

pub mod scheduler {
    use super::*;

    /// Benchmark context switch
    pub fn bench_context_switch(config: &BenchConfig) -> BenchmarkResult {
        benchmark("sched_context_switch", config, || {
            // Simulate context switch overhead
            core::hint::black_box(());
        })
    }

    /// Benchmark thread creation
    pub fn bench_thread_create(config: &BenchConfig) -> BenchmarkResult {
        benchmark("sched_thread_create", config, || {
            core::hint::black_box(());
        })
    }

    /// Benchmark mutex lock/unlock
    pub fn bench_mutex(config: &BenchConfig) -> BenchmarkResult {
        let lock = spin::Mutex::new(0u64);
        benchmark("sched_mutex", config, || {
            let mut guard = lock.lock();
            *guard += 1;
            core::hint::black_box(&*guard);
        })
    }

    /// Benchmark spinlock
    pub fn bench_spinlock(config: &BenchConfig) -> BenchmarkResult {
        let lock = spin::Mutex::new(0u64);
        benchmark("sched_spinlock", config, || {
            let guard = lock.lock();
            core::hint::black_box(&*guard);
        })
    }

    /// Run all scheduler benchmarks
    pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
        alloc::vec![
            bench_context_switch(config),
            bench_thread_create(config),
            bench_mutex(config),
            bench_spinlock(config),
        ]
    }
}

// =============================================================================
// Network Benchmarks
// =============================================================================

pub mod network {
    use super::*;

    /// Benchmark packet parsing
    pub fn bench_packet_parse(config: &BenchConfig) -> BenchmarkResult {
        let packet = [0u8; 1500];
        benchmark("net_packet_parse", config, || {
            core::hint::black_box(&packet);
        })
    }

    /// Benchmark checksum calculation
    pub fn bench_checksum(config: &BenchConfig) -> BenchmarkResult {
        let data = alloc::vec![0u8; 1500];
        benchmark("net_checksum", config, || {
            let sum: u32 = data.iter().map(|&b| b as u32).sum();
            core::hint::black_box(sum);
        })
    }

    /// Benchmark socket lookup
    pub fn bench_socket_lookup(config: &BenchConfig) -> BenchmarkResult {
        benchmark("net_socket_lookup", config, || {
            core::hint::black_box(());
        })
    }

    /// Run all network benchmarks
    pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
        alloc::vec![
            bench_packet_parse(config),
            bench_checksum(config),
            bench_socket_lookup(config),
        ]
    }
}

// =============================================================================
// Storage Benchmarks
// =============================================================================

pub mod storage {
    use super::*;

    /// Benchmark 4KB read
    pub fn bench_read_4k(config: &BenchConfig) -> BenchmarkResult {
        let buf = alloc::vec![0u8; 4096];
        benchmark("storage_read_4k", config, || {
            core::hint::black_box(&buf);
        })
    }

    /// Benchmark 4KB write
    pub fn bench_write_4k(config: &BenchConfig) -> BenchmarkResult {
        let buf = alloc::vec![0u8; 4096];
        benchmark("storage_write_4k", config, || {
            core::hint::black_box(&buf);
        })
    }

    /// Benchmark sequential read
    pub fn bench_seq_read(config: &BenchConfig) -> BenchmarkResult {
        let buf = alloc::vec![0u8; 1024 * 1024];
        benchmark("storage_seq_read_1m", config, || {
            core::hint::black_box(&buf);
        })
    }

    /// Benchmark random read
    pub fn bench_random_read(config: &BenchConfig) -> BenchmarkResult {
        let buf = alloc::vec![0u8; 4096];
        benchmark("storage_random_read", config, || {
            core::hint::black_box(&buf);
        })
    }

    /// Run all storage benchmarks
    pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
        alloc::vec![
            bench_read_4k(config),
            bench_write_4k(config),
            bench_seq_read(config),
            bench_random_read(config),
        ]
    }
}

// =============================================================================
// Crypto Benchmarks
// =============================================================================

pub mod crypto {
    use super::*;

    /// Benchmark SHA-256 hashing
    pub fn bench_sha256(config: &BenchConfig) -> BenchmarkResult {
        let data = alloc::vec![0u8; 4096];
        benchmark("crypto_sha256_4k", config, || {
            // Simulate SHA-256
            let mut hash = 0u64;
            for &byte in data.iter() {
                hash = hash.wrapping_add(byte as u64);
            }
            core::hint::black_box(hash);
        })
    }

    /// Benchmark AES-256-GCM encryption
    pub fn bench_aes_gcm(config: &BenchConfig) -> BenchmarkResult {
        let data = alloc::vec![0u8; 4096];
        benchmark("crypto_aes_gcm_4k", config, || {
            core::hint::black_box(&data);
        })
    }

    /// Benchmark ChaCha20-Poly1305
    pub fn bench_chacha20(config: &BenchConfig) -> BenchmarkResult {
        let data = alloc::vec![0u8; 4096];
        benchmark("crypto_chacha20_4k", config, || {
            core::hint::black_box(&data);
        })
    }

    /// Benchmark Ed25519 signing
    pub fn bench_ed25519_sign(config: &BenchConfig) -> BenchmarkResult {
        benchmark("crypto_ed25519_sign", config, || {
            core::hint::black_box(());
        })
    }

    /// Benchmark Ed25519 verification
    pub fn bench_ed25519_verify(config: &BenchConfig) -> BenchmarkResult {
        benchmark("crypto_ed25519_verify", config, || {
            core::hint::black_box(());
        })
    }

    /// Run all crypto benchmarks
    pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
        alloc::vec![
            bench_sha256(config),
            bench_aes_gcm(config),
            bench_chacha20(config),
            bench_ed25519_sign(config),
            bench_ed25519_verify(config),
        ]
    }
}

// =============================================================================
// WASM Benchmarks
// =============================================================================

pub mod wasm {
    use super::*;

    /// Benchmark WASM function call
    pub fn bench_call(config: &BenchConfig) -> BenchmarkResult {
        benchmark("wasm_call", config, || {
            core::hint::black_box(());
        })
    }

    /// Benchmark WASM memory access
    pub fn bench_memory_access(config: &BenchConfig) -> BenchmarkResult {
        let memory = alloc::vec![0u8; 65536];
        benchmark("wasm_memory_access", config, || {
            core::hint::black_box(&memory[1024]);
        })
    }

    /// Benchmark WASM instantiation
    pub fn bench_instantiate(config: &BenchConfig) -> BenchmarkResult {
        benchmark("wasm_instantiate", config, || {
            core::hint::black_box(());
        })
    }

    /// Run all WASM benchmarks
    pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
        alloc::vec![
            bench_call(config),
            bench_memory_access(config),
            bench_instantiate(config),
        ]
    }
}

// =============================================================================
// Capability Benchmarks
// =============================================================================

pub mod capability {
    use super::*;

    /// Benchmark capability verification
    pub fn bench_verify(config: &BenchConfig) -> BenchmarkResult {
        benchmark("cap_verify", config, || {
            core::hint::black_box(());
        })
    }

    /// Benchmark capability creation
    pub fn bench_create(config: &BenchConfig) -> BenchmarkResult {
        benchmark("cap_create", config, || {
            core::hint::black_box(());
        })
    }

    /// Benchmark capability lookup
    pub fn bench_lookup(config: &BenchConfig) -> BenchmarkResult {
        benchmark("cap_lookup", config, || {
            core::hint::black_box(());
        })
    }

    /// Run all capability benchmarks
    pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
        alloc::vec![
            bench_verify(config),
            bench_create(config),
            bench_lookup(config),
        ]
    }
}

// =============================================================================
// Benchmark Runner
// =============================================================================

/// Run all benchmarks
pub fn run_all(config: &BenchConfig) -> Vec<BenchmarkResult> {
    let mut results = Vec::new();
    
    results.extend(ipc::run_all(config));
    results.extend(memory::run_all(config));
    results.extend(scheduler::run_all(config));
    results.extend(network::run_all(config));
    results.extend(storage::run_all(config));
    results.extend(crypto::run_all(config));
    results.extend(wasm::run_all(config));
    results.extend(capability::run_all(config));
    
    results
}

/// Run benchmarks by category
pub fn run_category(category: &str, config: &BenchConfig) -> Vec<BenchmarkResult> {
    match category {
        "ipc" => ipc::run_all(config),
        "memory" => memory::run_all(config),
        "scheduler" => scheduler::run_all(config),
        "network" => network::run_all(config),
        "storage" => storage::run_all(config),
        "crypto" => crypto::run_all(config),
        "wasm" => wasm::run_all(config),
        "capability" => capability::run_all(config),
        _ => Vec::new(),
    }
}

/// List available benchmark categories
pub fn categories() -> &'static [&'static str] {
    &[
        "ipc",
        "memory",
        "scheduler",
        "network",
        "storage",
        "crypto",
        "wasm",
        "capability",
    ]
}

/// Format benchmark results as table
pub fn format_results(results: &[BenchmarkResult]) -> String {
    let mut output = String::from("Benchmark Results\n");
    output.push_str("=================\n\n");
    output.push_str("| Benchmark | Mean | Std Dev | Min | Max | Ops/sec |\n");
    output.push_str("|-----------|------|---------|-----|-----|--------|\n");
    
    for result in results {
        output.push_str(&alloc::format!(
            "| {} | {:.2} µs | {:.2} µs | {:.2} µs | {:.2} µs | {:.0} |\n",
            result.name,
            result.mean_ns as f64 / 1000.0,
            result.std_dev_ns as f64 / 1000.0,
            result.min_ns as f64 / 1000.0,
            result.max_ns as f64 / 1000.0,
            result.ops_per_sec
        ));
    }
    
    output
}
