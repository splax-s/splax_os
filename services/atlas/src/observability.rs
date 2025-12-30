//! # Observability Infrastructure
//!
//! This module implements a comprehensive observability stack for Splax OS,
//! including metrics, tracing, logging, and health checks.
//!
//! ## Components
//!
//! - **Metrics**: Counter, gauge, histogram with Prometheus-compatible export
//! - **Tracing**: Distributed tracing with spans and context propagation
//! - **Logging**: Structured logging with levels and fields
//! - **Health**: Health checks and readiness probes

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

// =============================================================================
// Metrics
// =============================================================================

/// Metric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    /// Monotonically increasing counter.
    Counter,
    /// Value that can go up or down.
    Gauge,
    /// Distribution of values.
    Histogram,
    /// Summary with quantiles.
    Summary,
}

/// Metric labels.
#[derive(Debug, Clone, Default)]
pub struct Labels {
    labels: BTreeMap<String, String>,
}

impl Labels {
    /// Create new empty labels.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a label.
    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(String::from(key), String::from(value));
        self
    }

    /// Get label value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(|s| s.as_str())
    }

    /// Format labels for Prometheus.
    pub fn to_prometheus(&self) -> String {
        if self.labels.is_empty() {
            return String::new();
        }

        let parts: Vec<String> = self
            .labels
            .iter()
            .map(|(k, v)| alloc::format!("{}=\"{}\"", k, escape_label_value(v)))
            .collect();

        alloc::format!("{{{}}}", parts.join(","))
    }
}

/// Escape label value for Prometheus.
fn escape_label_value(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            _ => result.push(c),
        }
    }
    result
}

/// Counter metric (monotonically increasing).
pub struct Counter {
    name: String,
    help: String,
    value: AtomicU64,
    labels: Labels,
}

impl Counter {
    /// Create a new counter.
    pub fn new(name: &str, help: &str) -> Self {
        Self {
            name: String::from(name),
            help: String::from(help),
            value: AtomicU64::new(0),
            labels: Labels::new(),
        }
    }

    /// Create a counter with labels.
    pub fn with_labels(name: &str, help: &str, labels: Labels) -> Self {
        Self {
            name: String::from(name),
            help: String::from(help),
            value: AtomicU64::new(0),
            labels,
        }
    }

    /// Increment by 1.
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Add a value.
    pub fn add(&self, v: u64) {
        self.value.fetch_add(v, Ordering::Relaxed);
    }

    /// Get current value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset to zero.
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

/// Gauge metric (can go up or down).
pub struct Gauge {
    name: String,
    help: String,
    value: AtomicU64,
    labels: Labels,
}

impl Gauge {
    /// Create a new gauge.
    pub fn new(name: &str, help: &str) -> Self {
        Self {
            name: String::from(name),
            help: String::from(help),
            value: AtomicU64::new(0),
            labels: Labels::new(),
        }
    }

    /// Create a gauge with labels.
    pub fn with_labels(name: &str, help: &str, labels: Labels) -> Self {
        Self {
            name: String::from(name),
            help: String::from(help),
            value: AtomicU64::new(0),
            labels,
        }
    }

    /// Set the value.
    pub fn set(&self, v: f64) {
        self.value.store(v.to_bits(), Ordering::Relaxed);
    }

    /// Get current value.
    pub fn get(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }

    /// Increment by 1.
    pub fn inc(&self) {
        let current = self.get();
        self.set(current + 1.0);
    }

    /// Decrement by 1.
    pub fn dec(&self) {
        let current = self.get();
        self.set(current - 1.0);
    }

    /// Add a value.
    pub fn add(&self, v: f64) {
        let current = self.get();
        self.set(current + v);
    }

    /// Subtract a value.
    pub fn sub(&self, v: f64) {
        let current = self.get();
        self.set(current - v);
    }
}

/// Histogram bucket.
#[derive(Debug, Clone)]
pub struct HistogramBucket {
    /// Upper bound.
    pub le: f64,
    /// Count of observations.
    pub count: u64,
}

/// Histogram metric.
pub struct Histogram {
    name: String,
    help: String,
    buckets: Vec<f64>,
    counts: Vec<AtomicU64>,
    sum: AtomicU64,
    count: AtomicU64,
    labels: Labels,
}

impl Histogram {
    /// Create a new histogram with default buckets.
    pub fn new(name: &str, help: &str) -> Self {
        Self::with_buckets(
            name,
            help,
            vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
        )
    }

    /// Create a histogram with custom buckets.
    pub fn with_buckets(name: &str, help: &str, buckets: Vec<f64>) -> Self {
        let counts = buckets.iter().map(|_| AtomicU64::new(0)).collect();

        Self {
            name: String::from(name),
            help: String::from(help),
            buckets,
            counts,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
            labels: Labels::new(),
        }
    }

    /// Create linear buckets.
    pub fn linear_buckets(start: f64, width: f64, count: usize) -> Vec<f64> {
        (0..count).map(|i| start + width * i as f64).collect()
    }

    /// Create exponential buckets.
    pub fn exponential_buckets(start: f64, factor: f64, count: usize) -> Vec<f64> {
        let mut buckets = Vec::with_capacity(count);
        let mut current = start;
        for _ in 0..count {
            buckets.push(current);
            current *= factor;
        }
        buckets
    }

    /// Observe a value.
    pub fn observe(&self, v: f64) {
        // Find bucket and increment
        for (i, &bound) in self.buckets.iter().enumerate() {
            if v <= bound {
                self.counts[i].fetch_add(1, Ordering::Relaxed);
            }
        }

        // Update sum and count
        let sum_bits = self.sum.load(Ordering::Relaxed);
        let current_sum = f64::from_bits(sum_bits);
        self.sum.store((current_sum + v).to_bits(), Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get observation count.
    pub fn get_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get sum of observations.
    pub fn get_sum(&self) -> f64 {
        f64::from_bits(self.sum.load(Ordering::Relaxed))
    }

    /// Get bucket counts.
    pub fn get_buckets(&self) -> Vec<HistogramBucket> {
        self.buckets
            .iter()
            .zip(self.counts.iter())
            .map(|(&le, count)| HistogramBucket {
                le,
                count: count.load(Ordering::Relaxed),
            })
            .collect()
    }
}

/// Metrics registry.
pub struct MetricsRegistry {
    counters: BTreeMap<String, Counter>,
    gauges: BTreeMap<String, Gauge>,
    histograms: BTreeMap<String, Histogram>,
    prefix: String,
}

impl MetricsRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self {
            counters: BTreeMap::new(),
            gauges: BTreeMap::new(),
            histograms: BTreeMap::new(),
            prefix: String::new(),
        }
    }

    /// Create a registry with a prefix.
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            counters: BTreeMap::new(),
            gauges: BTreeMap::new(),
            histograms: BTreeMap::new(),
            prefix: String::from(prefix),
        }
    }

    fn full_name(&self, name: &str) -> String {
        if self.prefix.is_empty() {
            String::from(name)
        } else {
            alloc::format!("{}_{}", self.prefix, name)
        }
    }

    /// Register a counter.
    pub fn register_counter(&mut self, name: &str, help: &str) -> &Counter {
        let full_name = self.full_name(name);
        self.counters
            .entry(full_name.clone())
            .or_insert_with(|| Counter::new(&full_name, help))
    }

    /// Register a gauge.
    pub fn register_gauge(&mut self, name: &str, help: &str) -> &Gauge {
        let full_name = self.full_name(name);
        self.gauges
            .entry(full_name.clone())
            .or_insert_with(|| Gauge::new(&full_name, help))
    }

    /// Register a histogram.
    pub fn register_histogram(&mut self, name: &str, help: &str) -> &Histogram {
        let full_name = self.full_name(name);
        self.histograms
            .entry(full_name.clone())
            .or_insert_with(|| Histogram::new(&full_name, help))
    }

    /// Export metrics in Prometheus text format.
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();

        // Export counters
        for counter in self.counters.values() {
            output.push_str(&alloc::format!("# HELP {} {}\n", counter.name, counter.help));
            output.push_str(&alloc::format!("# TYPE {} counter\n", counter.name));
            output.push_str(&alloc::format!(
                "{}{} {}\n",
                counter.name,
                counter.labels.to_prometheus(),
                counter.get()
            ));
        }

        // Export gauges
        for gauge in self.gauges.values() {
            output.push_str(&alloc::format!("# HELP {} {}\n", gauge.name, gauge.help));
            output.push_str(&alloc::format!("# TYPE {} gauge\n", gauge.name));
            output.push_str(&alloc::format!(
                "{}{} {}\n",
                gauge.name,
                gauge.labels.to_prometheus(),
                gauge.get()
            ));
        }

        // Export histograms
        for histogram in self.histograms.values() {
            output.push_str(&alloc::format!("# HELP {} {}\n", histogram.name, histogram.help));
            output.push_str(&alloc::format!("# TYPE {} histogram\n", histogram.name));

            let mut cumulative = 0u64;
            for bucket in histogram.get_buckets() {
                cumulative += bucket.count;
                output.push_str(&alloc::format!(
                    "{}_bucket{{le=\"{}\"}} {}\n",
                    histogram.name, bucket.le, cumulative
                ));
            }

            output.push_str(&alloc::format!(
                "{}_bucket{{le=\"+Inf\"}} {}\n",
                histogram.name,
                histogram.get_count()
            ));
            output.push_str(&alloc::format!(
                "{}_sum {} \n",
                histogram.name,
                histogram.get_sum()
            ));
            output.push_str(&alloc::format!(
                "{}_count {}\n",
                histogram.name,
                histogram.get_count()
            ));
        }

        output
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Distributed Tracing
// =============================================================================

/// Trace ID (128-bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId([u8; 16]);

impl TraceId {
    /// Generate a new random trace ID.
    pub fn new_random(random: &mut dyn FnMut() -> u64) -> Self {
        let high = random();
        let low = random();
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&high.to_be_bytes());
        id[8..].copy_from_slice(&low.to_be_bytes());
        Self(id)
    }

    /// Convert to hex string.
    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(32);
        for byte in &self.0 {
            hex.push(HEX_CHARS[(byte >> 4) as usize]);
            hex.push(HEX_CHARS[(byte & 0x0f) as usize]);
        }
        hex
    }
}

/// Span ID (64-bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId([u8; 8]);

impl SpanId {
    /// Generate a new random span ID.
    pub fn new_random(random: &mut dyn FnMut() -> u64) -> Self {
        Self(random().to_be_bytes())
    }

    /// Convert to hex string.
    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(16);
        for byte in &self.0 {
            hex.push(HEX_CHARS[(byte >> 4) as usize]);
            hex.push(HEX_CHARS[(byte & 0x0f) as usize]);
        }
        hex
    }
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Trace context for propagation.
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// Trace ID.
    pub trace_id: TraceId,
    /// Parent span ID.
    pub parent_span_id: Option<SpanId>,
    /// Current span ID.
    pub span_id: SpanId,
    /// Trace flags.
    pub flags: TraceFlags,
    /// Trace state (vendor-specific).
    pub trace_state: String,
}

/// Trace flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct TraceFlags(u8);

impl TraceFlags {
    /// Sampled flag.
    pub const SAMPLED: u8 = 0x01;

    /// Check if sampled.
    pub fn is_sampled(&self) -> bool {
        self.0 & Self::SAMPLED != 0
    }

    /// Set sampled flag.
    pub fn set_sampled(&mut self, sampled: bool) {
        if sampled {
            self.0 |= Self::SAMPLED;
        } else {
            self.0 &= !Self::SAMPLED;
        }
    }
}

/// Span kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// Internal operation.
    Internal,
    /// Server handling a request.
    Server,
    /// Client making a request.
    Client,
    /// Producer sending a message.
    Producer,
    /// Consumer receiving a message.
    Consumer,
}

/// Span status.
#[derive(Debug, Clone)]
pub enum SpanStatus {
    /// Unset status.
    Unset,
    /// Operation completed successfully.
    Ok,
    /// Operation failed with error.
    Error(String),
}

/// Span event.
#[derive(Debug, Clone)]
pub struct SpanEvent {
    /// Event name.
    pub name: String,
    /// Timestamp (nanoseconds since epoch).
    pub timestamp: u64,
    /// Event attributes.
    pub attributes: BTreeMap<String, AttributeValue>,
}

/// Span link.
#[derive(Debug, Clone)]
pub struct SpanLink {
    /// Linked trace context.
    pub context: TraceContext,
    /// Link attributes.
    pub attributes: BTreeMap<String, AttributeValue>,
}

/// Attribute value types.
#[derive(Debug, Clone)]
pub enum AttributeValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    StringArray(Vec<String>),
    IntArray(Vec<i64>),
    FloatArray(Vec<f64>),
    BoolArray(Vec<bool>),
}

/// A span in a trace.
#[derive(Debug)]
pub struct Span {
    /// Span name.
    pub name: String,
    /// Trace context.
    pub context: TraceContext,
    /// Span kind.
    pub kind: SpanKind,
    /// Start timestamp.
    pub start_time: u64,
    /// End timestamp.
    pub end_time: Option<u64>,
    /// Status.
    pub status: SpanStatus,
    /// Attributes.
    pub attributes: BTreeMap<String, AttributeValue>,
    /// Events.
    pub events: Vec<SpanEvent>,
    /// Links.
    pub links: Vec<SpanLink>,
}

impl Span {
    /// Create a new span.
    pub fn new(
        name: &str,
        trace_id: TraceId,
        span_id: SpanId,
        parent_span_id: Option<SpanId>,
    ) -> Self {
        Self {
            name: String::from(name),
            context: TraceContext {
                trace_id,
                parent_span_id,
                span_id,
                flags: TraceFlags::default(),
                trace_state: String::new(),
            },
            kind: SpanKind::Internal,
            start_time: 0,
            end_time: None,
            status: SpanStatus::Unset,
            attributes: BTreeMap::new(),
            events: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Set span kind.
    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    /// Add an attribute.
    pub fn set_attribute(&mut self, key: &str, value: AttributeValue) {
        self.attributes.insert(String::from(key), value);
    }

    /// Add an event.
    pub fn add_event(&mut self, name: &str, timestamp: u64) {
        self.events.push(SpanEvent {
            name: String::from(name),
            timestamp,
            attributes: BTreeMap::new(),
        });
    }

    /// Set status.
    pub fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    /// End the span.
    pub fn end(&mut self, timestamp: u64) {
        self.end_time = Some(timestamp);
    }

    /// Get duration in nanoseconds.
    pub fn duration(&self) -> Option<u64> {
        self.end_time.map(|end| end - self.start_time)
    }
}

/// Tracer for creating spans.
pub struct Tracer {
    /// Service name.
    service_name: String,
    /// Random state.
    random_state: u64,
    /// Completed spans.
    completed_spans: Vec<Span>,
}

impl Tracer {
    /// Create a new tracer.
    pub fn new(service_name: &str) -> Self {
        Self {
            service_name: String::from(service_name),
            random_state: 0x12345678_9ABCDEF0,
            completed_spans: Vec::new(),
        }
    }

    fn random(&mut self) -> u64 {
        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 7;
        self.random_state ^= self.random_state << 17;
        self.random_state
    }

    /// Start a new root span.
    pub fn start_span(&mut self, name: &str) -> Span {
        let trace_id = TraceId::new_random(&mut || self.random());
        let span_id = SpanId::new_random(&mut || self.random());
        Span::new(name, trace_id, span_id, None)
    }

    /// Start a child span.
    pub fn start_child_span(&mut self, name: &str, parent: &Span) -> Span {
        let span_id = SpanId::new_random(&mut || self.random());
        Span::new(
            name,
            parent.context.trace_id,
            span_id,
            Some(parent.context.span_id),
        )
    }

    /// Record a completed span.
    pub fn record(&mut self, span: Span) {
        self.completed_spans.push(span);
    }

    /// Export spans (clears recorded spans).
    pub fn export(&mut self) -> Vec<Span> {
        core::mem::take(&mut self.completed_spans)
    }
}

// =============================================================================
// Structured Logging
// =============================================================================

/// Log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
}

impl LogLevel {
    /// Convert to string.
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Fatal => "FATAL",
        }
    }
}

/// Log record.
#[derive(Debug, Clone)]
pub struct LogRecord {
    /// Timestamp (nanoseconds since epoch).
    pub timestamp: u64,
    /// Log level.
    pub level: LogLevel,
    /// Message.
    pub message: String,
    /// Target/module.
    pub target: String,
    /// Structured fields.
    pub fields: BTreeMap<String, String>,
    /// Trace context (if available).
    pub trace_id: Option<TraceId>,
    /// Span context (if available).
    pub span_id: Option<SpanId>,
}

/// Logger configuration.
#[derive(Debug, Clone)]
pub struct LoggerConfig {
    /// Minimum log level.
    pub level: LogLevel,
    /// Include timestamps.
    pub include_timestamp: bool,
    /// Include target.
    pub include_target: bool,
    /// Output format.
    pub format: LogFormat,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            include_timestamp: true,
            include_target: true,
            format: LogFormat::Text,
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Human-readable text.
    Text,
    /// JSON format.
    Json,
    /// Logfmt format.
    Logfmt,
}

/// Logger.
pub struct Logger {
    config: LoggerConfig,
    records: Vec<LogRecord>,
}

impl Logger {
    /// Create a new logger.
    pub fn new(config: LoggerConfig) -> Self {
        Self {
            config,
            records: Vec::new(),
        }
    }

    /// Check if level is enabled.
    pub fn enabled(&self, level: LogLevel) -> bool {
        level >= self.config.level
    }

    /// Log a message.
    pub fn log(&mut self, level: LogLevel, target: &str, message: &str) {
        if !self.enabled(level) {
            return;
        }

        self.records.push(LogRecord {
            timestamp: 0, // Would be current time
            level,
            message: String::from(message),
            target: String::from(target),
            fields: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        });
    }

    /// Log with fields.
    pub fn log_with_fields(
        &mut self,
        level: LogLevel,
        target: &str,
        message: &str,
        fields: BTreeMap<String, String>,
    ) {
        if !self.enabled(level) {
            return;
        }

        self.records.push(LogRecord {
            timestamp: 0,
            level,
            message: String::from(message),
            target: String::from(target),
            fields,
            trace_id: None,
            span_id: None,
        });
    }

    /// Format a log record.
    pub fn format(&self, record: &LogRecord) -> String {
        match self.config.format {
            LogFormat::Text => self.format_text(record),
            LogFormat::Json => self.format_json(record),
            LogFormat::Logfmt => self.format_logfmt(record),
        }
    }

    fn format_text(&self, record: &LogRecord) -> String {
        let mut output = String::new();

        if self.config.include_timestamp {
            output.push_str(&alloc::format!("[{}] ", record.timestamp));
        }

        output.push_str(&alloc::format!("{} ", record.level.as_str()));

        if self.config.include_target {
            output.push_str(&alloc::format!("[{}] ", record.target));
        }

        output.push_str(&record.message);

        for (key, value) in &record.fields {
            output.push_str(&alloc::format!(" {}={}", key, value));
        }

        output
    }

    fn format_json(&self, record: &LogRecord) -> String {
        let mut output = String::from("{");
        output.push_str(&alloc::format!("\"timestamp\":{},", record.timestamp));
        output.push_str(&alloc::format!("\"level\":\"{}\",", record.level.as_str()));
        output.push_str(&alloc::format!("\"target\":\"{}\",", record.target));
        output.push_str(&alloc::format!(
            "\"message\":\"{}\"",
            escape_json(&record.message)
        ));

        for (key, value) in &record.fields {
            output.push_str(&alloc::format!(",\"{}\":\"{}\"", key, escape_json(value)));
        }

        output.push('}');
        output
    }

    fn format_logfmt(&self, record: &LogRecord) -> String {
        let mut output = String::new();
        output.push_str(&alloc::format!("ts={} ", record.timestamp));
        output.push_str(&alloc::format!("level={} ", record.level.as_str()));
        output.push_str(&alloc::format!("target={} ", record.target));
        output.push_str(&alloc::format!("msg=\"{}\"", escape_logfmt(&record.message)));

        for (key, value) in &record.fields {
            output.push_str(&alloc::format!(" {}=\"{}\"", key, escape_logfmt(value)));
        }

        output
    }

    /// Export log records.
    pub fn export(&mut self) -> Vec<LogRecord> {
        core::mem::take(&mut self.records)
    }

    /// Convenience methods.
    pub fn trace(&mut self, target: &str, message: &str) {
        self.log(LogLevel::Trace, target, message);
    }

    pub fn debug(&mut self, target: &str, message: &str) {
        self.log(LogLevel::Debug, target, message);
    }

    pub fn info(&mut self, target: &str, message: &str) {
        self.log(LogLevel::Info, target, message);
    }

    pub fn warn(&mut self, target: &str, message: &str) {
        self.log(LogLevel::Warn, target, message);
    }

    pub fn error(&mut self, target: &str, message: &str) {
        self.log(LogLevel::Error, target, message);
    }

    pub fn fatal(&mut self, target: &str, message: &str) {
        self.log(LogLevel::Fatal, target, message);
    }
}

fn escape_json(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(c),
        }
    }
    result
}

fn escape_logfmt(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// =============================================================================
// Health Checks
// =============================================================================

/// Health check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthResult {
    /// Component is healthy.
    Healthy,
    /// Component is degraded but functional.
    Degraded,
    /// Component is unhealthy.
    Unhealthy,
}

/// Health check details.
#[derive(Debug, Clone)]
pub struct HealthCheck {
    /// Component name.
    pub name: String,
    /// Result.
    pub result: HealthResult,
    /// Message.
    pub message: Option<String>,
    /// Duration of check (microseconds).
    pub duration_us: u64,
    /// Last check timestamp.
    pub last_check: u64,
}

/// Health checker.
pub struct HealthChecker {
    checks: BTreeMap<String, HealthCheck>,
}

impl HealthChecker {
    /// Create a new health checker.
    pub fn new() -> Self {
        Self {
            checks: BTreeMap::new(),
        }
    }

    /// Register a health check.
    pub fn register(&mut self, name: &str) {
        self.checks.insert(
            String::from(name),
            HealthCheck {
                name: String::from(name),
                result: HealthResult::Healthy,
                message: None,
                duration_us: 0,
                last_check: 0,
            },
        );
    }

    /// Update a health check result.
    pub fn update(
        &mut self,
        name: &str,
        result: HealthResult,
        message: Option<&str>,
        duration_us: u64,
    ) {
        if let Some(check) = self.checks.get_mut(name) {
            check.result = result;
            check.message = message.map(String::from);
            check.duration_us = duration_us;
            check.last_check = 0; // Would be current time
        }
    }

    /// Get overall health status.
    pub fn overall_status(&self) -> HealthResult {
        let mut has_degraded = false;

        for check in self.checks.values() {
            match check.result {
                HealthResult::Unhealthy => return HealthResult::Unhealthy,
                HealthResult::Degraded => has_degraded = true,
                HealthResult::Healthy => {}
            }
        }

        if has_degraded {
            HealthResult::Degraded
        } else {
            HealthResult::Healthy
        }
    }

    /// Get all health checks.
    pub fn get_checks(&self) -> &BTreeMap<String, HealthCheck> {
        &self.checks
    }

    /// Format as JSON.
    pub fn to_json(&self) -> String {
        let status = match self.overall_status() {
            HealthResult::Healthy => "healthy",
            HealthResult::Degraded => "degraded",
            HealthResult::Unhealthy => "unhealthy",
        };

        let mut output = alloc::format!("{{\"status\":\"{}\",\"checks\":{{", status);

        let checks: Vec<String> = self
            .checks
            .values()
            .map(|check| {
                let result = match check.result {
                    HealthResult::Healthy => "healthy",
                    HealthResult::Degraded => "degraded",
                    HealthResult::Unhealthy => "unhealthy",
                };
                let message = check
                    .message
                    .as_ref()
                    .map(|m| alloc::format!(",\"message\":\"{}\"", escape_json(m)))
                    .unwrap_or_default();

                alloc::format!(
                    "\"{}\":{{\"status\":\"{}\",\"duration_us\":{}{}}}",
                    check.name, result, check.duration_us, message
                )
            })
            .collect();

        output.push_str(&checks.join(","));
        output.push_str("}}");

        output
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Observability Stack
// =============================================================================

/// Complete observability stack.
pub struct Observability {
    /// Metrics registry.
    pub metrics: MetricsRegistry,
    /// Tracer.
    pub tracer: Tracer,
    /// Logger.
    pub logger: Logger,
    /// Health checker.
    pub health: HealthChecker,
}

impl Observability {
    /// Create a new observability stack.
    pub fn new(service_name: &str) -> Self {
        Self {
            metrics: MetricsRegistry::with_prefix(service_name),
            tracer: Tracer::new(service_name),
            logger: Logger::new(LoggerConfig::default()),
            health: HealthChecker::new(),
        }
    }

    /// Initialize default metrics.
    pub fn init_default_metrics(&mut self) {
        self.metrics.register_counter("requests_total", "Total number of requests");
        self.metrics.register_counter("errors_total", "Total number of errors");
        self.metrics.register_gauge("active_connections", "Number of active connections");
        self.metrics.register_histogram("request_duration_seconds", "Request duration in seconds");
    }

    /// Initialize default health checks.
    pub fn init_default_health_checks(&mut self) {
        self.health.register("memory");
        self.health.register("disk");
        self.health.register("network");
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let counter = Counter::new("test_counter", "A test counter");
        assert_eq!(counter.get(), 0);

        counter.inc();
        assert_eq!(counter.get(), 1);

        counter.add(5);
        assert_eq!(counter.get(), 6);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new("test_gauge", "A test gauge");
        assert_eq!(gauge.get(), 0.0);

        gauge.set(42.5);
        assert_eq!(gauge.get(), 42.5);

        gauge.inc();
        assert_eq!(gauge.get(), 43.5);

        gauge.dec();
        assert_eq!(gauge.get(), 42.5);
    }

    #[test]
    fn test_histogram() {
        let hist = Histogram::with_buckets("test_hist", "A test histogram", vec![1.0, 5.0, 10.0]);

        hist.observe(0.5);
        hist.observe(2.0);
        hist.observe(7.0);
        hist.observe(15.0);

        assert_eq!(hist.get_count(), 4);

        let buckets = hist.get_buckets();
        assert_eq!(buckets[0].count, 1); // <= 1.0
        assert_eq!(buckets[1].count, 1); // <= 5.0 (only 2.0, 0.5 already counted)
        assert_eq!(buckets[2].count, 1); // <= 10.0 (only 7.0)
    }

    #[test]
    fn test_labels() {
        let labels = Labels::new()
            .with("method", "GET")
            .with("status", "200");

        assert_eq!(labels.get("method"), Some("GET"));
        assert_eq!(labels.to_prometheus(), "{method=\"GET\",status=\"200\"}");
    }

    #[test]
    fn test_tracer() {
        let mut tracer = Tracer::new("test-service");

        let mut span = tracer.start_span("operation");
        span.set_attribute("key", AttributeValue::String(String::from("value")));
        span.add_event("checkpoint", 1000);
        span.end(2000);

        tracer.record(span);

        let spans = tracer.export();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "operation");
    }

    #[test]
    fn test_logger() {
        let config = LoggerConfig {
            level: LogLevel::Debug,
            ..Default::default()
        };
        let mut logger = Logger::new(config);

        logger.info("test", "Hello, world!");
        logger.debug("test", "Debug message");
        logger.trace("test", "Trace message"); // Should be ignored

        let records = logger.export();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_health_checker() {
        let mut health = HealthChecker::new();

        health.register("database");
        health.register("cache");

        health.update("database", HealthResult::Healthy, None, 100);
        health.update("cache", HealthResult::Healthy, None, 50);

        assert_eq!(health.overall_status(), HealthResult::Healthy);

        health.update("cache", HealthResult::Degraded, Some("High latency"), 500);
        assert_eq!(health.overall_status(), HealthResult::Degraded);

        health.update("database", HealthResult::Unhealthy, Some("Connection failed"), 0);
        assert_eq!(health.overall_status(), HealthResult::Unhealthy);
    }

    #[test]
    fn test_prometheus_export() {
        let mut registry = MetricsRegistry::with_prefix("myapp");

        registry.register_counter("requests", "Total requests").add(100);
        registry.register_gauge("temperature", "Current temperature").set(23.5);

        let output = registry.export_prometheus();
        assert!(output.contains("myapp_requests"));
        assert!(output.contains("myapp_temperature"));
        assert!(output.contains("100"));
    }
}
