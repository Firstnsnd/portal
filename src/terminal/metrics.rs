//! # System Metrics
//!
//! Data model for per-target system metrics (CPU, memory, network, load)
//! collected periodically and displayed in the terminal metrics drawer.

use std::collections::VecDeque;

/// Number of history samples to retain (5 s × 60 = 5 min).
pub const HISTORY_CAP: usize = 60;

/// One sample of system-level metrics for a target.
#[derive(Clone, Debug)]
pub struct SystemMetrics {
    pub cpu_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub load_1: f32,
    pub net_rx_bytes_per_sec: u64,
    pub net_tx_bytes_per_sec: u64,
}

/// Rolling history ring buffers + latest snapshot.
///
/// Updated by a background collection task (local: sysinfo; remote: SSH exec).
/// The UI drawer reads the `latest` and history buffers for sparkline rendering.
pub struct MetricsSnapshot {
    pub latest: Option<SystemMetrics>,
    pub cpu_history: VecDeque<f32>,
    pub mem_history: VecDeque<f32>,
    pub net_rx_history: VecDeque<f32>,
    pub net_tx_history: VecDeque<f32>,
    pub load_history: VecDeque<f32>,

    // Differentials for computing per-second rates (CPU / network)
    pub(crate) prev_cpu_total: Option<u64>,
    pub(crate) prev_cpu_idle: Option<u64>,
    pub(crate) prev_net_rx: Option<u64>,
    pub(crate) prev_net_tx: Option<u64>,
}

impl MetricsSnapshot {
    pub fn new() -> Self {
        Self {
            latest: None,
            cpu_history: VecDeque::with_capacity(HISTORY_CAP),
            mem_history: VecDeque::with_capacity(HISTORY_CAP),
            net_rx_history: VecDeque::with_capacity(HISTORY_CAP),
            net_tx_history: VecDeque::with_capacity(HISTORY_CAP),
            load_history: VecDeque::with_capacity(HISTORY_CAP),
            prev_cpu_total: None,
            prev_cpu_idle: None,
            prev_net_rx: None,
            prev_net_tx: None,
        }
    }

    /// Append a new sample; `latest` is updated, history rings updated.
    pub fn push(&mut self, m: SystemMetrics) {
        self.latest = Some(m.clone());
        let mem_pct = (m.mem_used_bytes as f64 / m.mem_total_bytes.max(1) as f64 * 100.0) as f32;
        push_capped(&mut self.cpu_history, m.cpu_percent);
        push_capped(&mut self.mem_history, mem_pct);
        push_capped(&mut self.net_rx_history, m.net_rx_bytes_per_sec as f32);
        push_capped(&mut self.net_tx_history, m.net_tx_bytes_per_sec as f32);
        push_capped(&mut self.load_history, m.load_1);
    }
}

fn push_capped<T>(q: &mut VecDeque<T>, v: T) {
    if q.len() >= HISTORY_CAP {
        q.pop_front();
    }
    q.push_back(v);
}

// ── Remote parsing (Linux /proc) ──────────────────────────────────

/// Parse the combined `/proc` command output for a remote Linux target.
///
/// The remote command emits sections separated by `=X=` markers:
/// `=M=` meminfo, `=N=` net/dev, `=L=` loadavg, `=C=` /proc/stat.
///
/// Applies differentials stored in `snap` to compute CPU% and network rate,
/// then calls `snap.push(...)`.
pub fn parse_remote(output: &str, snap: &mut MetricsSnapshot) -> Option<SystemMetrics> {
    let mut mem_total: Option<u64> = None;
    let mut mem_avail: Option<u64> = None;
    let mut net_rx: Option<u64> = None; // cumulative bytes (all non-lo ifaces)
    let mut net_tx: Option<u64> = None;
    let mut load_1: Option<f32> = None;
    let mut cpu_total: Option<u64> = None;
    let mut cpu_idle: Option<u64> = None;

    let mut section = "";
    for line in output.lines() {
        let line = line.trim();
        if line == "=M=" { section = "M"; continue; }
        if line == "=N=" { section = "N"; continue; }
        if line == "=L=" { section = "L"; continue; }
        if line == "=C=" { section = "C"; continue; }

        match section {
            "M" => {
                if line.starts_with("MemTotal:") {
                    mem_total = parse_kb_value(line);
                } else if line.starts_with("MemAvailable:") {
                    mem_avail = parse_kb_value(line);
                }
            }
            "N" => {
                if line.starts_with("Inter-") || line.starts_with(" face") || line.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 10 {
                    let name = parts[0].trim_end_matches(':');
                    if name != "lo" {
                        if let (Ok(r), Ok(t)) = (parts[1].parse::<u64>(), parts[9].parse::<u64>()) {
                            *net_rx.get_or_insert(0) += r;
                            *net_tx.get_or_insert(0) += t;
                        }
                    }
                }
            }
            "L" => {
                let p: Vec<&str> = line.split_whitespace().collect();
                if p.len() >= 3 {
                    load_1 = p[0].parse::<f32>().ok();
                }
            }
            "C" => {
                if line.starts_with("cpu ") {
                    let vals: Vec<u64> = line.split_whitespace()
                        .skip(1)
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    if vals.len() >= 4 {
                        let idle = vals[3] + vals.get(4).copied().unwrap_or(0);
                        let total: u64 = vals.iter().sum();
                        cpu_total = Some(total);
                        cpu_idle = Some(idle);
                    }
                }
            }
            _ => {}
        }
    }

    let mem_used = mem_total.and_then(|t| mem_avail.map(|a| t.saturating_sub(a)));
    let cpu_pct = match (cpu_total, cpu_idle, snap.prev_cpu_total, snap.prev_cpu_idle) {
        (Some(t), Some(i), Some(pt), Some(pi)) if t > pt => {
            let dt = t - pt;
            let di = i.saturating_sub(pi);
            ((dt.saturating_sub(di)) as f64 / dt as f64 * 100.0) as f32
        }
        _ => 0.0,
    };
    let net_rx_rate = delta_rate(net_rx, snap.prev_net_rx);
    let net_tx_rate = delta_rate(net_tx, snap.prev_net_tx);

    snap.prev_cpu_total = cpu_total;
    snap.prev_cpu_idle = cpu_idle;
    snap.prev_net_rx = net_rx;
    snap.prev_net_tx = net_tx;

    let m = SystemMetrics {
        cpu_percent: cpu_pct,
        mem_used_bytes: mem_used.map_or(0, |u| u * 1024),
        mem_total_bytes: mem_total.map_or(0, |u| u * 1024),
        load_1: load_1.unwrap_or(0.0),
        net_rx_bytes_per_sec: net_rx_rate,
        net_tx_bytes_per_sec: net_tx_rate,
    };
    snap.push(m.clone());
    Some(m)
}

fn parse_kb_value(line: &str) -> Option<u64> {
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
}

fn delta_rate(current: Option<u64>, prev: Option<u64>) -> u64 {
    match (current, prev) {
        (Some(c), Some(p)) if c >= p => ((c - p) as f64 / 5.0) as u64,
        _ => 0,
    }
}
