//! Process resource sampling (RFC 0138) — best-effort, Linux-only, never fabricated. Reads
//! `/proc/self/status`/`/proc/self/stat` directly (no new dependency for what's a diagnostic,
//! non-gating metric). Every field is `Option`: `None` on any other platform, or if `/proc`
//! doesn't have the expected shape — the report renders "n/a", never a fake number.

use std::time::Duration;

#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceSample {
    /// Resident set size, in KB, at the moment of sampling.
    pub rss_kb: Option<u64>,
    /// Process CPU time (user + system) consumed so far, cumulative since process start.
    pub cpu_time: Option<Duration>,
}

/// The difference `after - before`, saturating at zero rather than underflowing if a value
/// somehow decreased (it shouldn't, for either of these counters, but this is diagnostic data —
/// never panic over it).
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceDelta {
    pub rss_kb_end: Option<u64>,
    pub cpu_time: Option<Duration>,
}

impl ResourceDelta {
    pub fn between(before: ResourceSample, after: ResourceSample) -> Self {
        Self {
            rss_kb_end: after.rss_kb,
            cpu_time: match (before.cpu_time, after.cpu_time) {
                (Some(b), Some(a)) => Some(a.saturating_sub(b)),
                _ => None,
            },
        }
    }
}

#[cfg(target_os = "linux")]
pub fn sample() -> ResourceSample {
    ResourceSample {
        rss_kb: read_rss_kb(),
        cpu_time: read_cpu_time(),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn sample() -> ResourceSample {
    ResourceSample::default()
}

#[cfg(target_os = "linux")]
fn read_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

/// User `HZ` (`sysconf(_SC_CLK_TCK)`) — 100 on effectively every modern Linux (x86_64/arm64,
/// glibc or musl) this project targets. Hardcoded rather than pulling in `libc` for one syscall;
/// wrong only on an exotic kernel config, and this is a diagnostic metric, not a gated one.
#[cfg(target_os = "linux")]
const CLK_TCK: u64 = 100;

#[cfg(target_os = "linux")]
fn read_cpu_time() -> Option<Duration> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // Fields are space-separated, but field 2 (comm) is parenthesized and may itself contain
    // spaces — split on the last ')' and re-split what follows, the standard safe way to parse
    // this file.
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // After the comm field, index 0 is state (field 3); utime is field 14, stime is field 15 —
    // i.e. indices 11 and 12 in this post-comm, 0-based split.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(Duration::from_secs_f64(
        (utime + stime) as f64 / CLK_TCK as f64,
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn sample_reads_real_process_data() {
        let s = sample();
        assert!(s.rss_kb.unwrap_or(0) > 0, "expected a real RSS reading");
        // cpu_time may legitimately be zero on a fast test, but must at least parse.
        assert!(s.cpu_time.is_some());
    }

    #[test]
    fn delta_between_never_panics_on_equal_samples() {
        let s = sample();
        let d = ResourceDelta::between(s, s);
        assert_eq!(d.cpu_time, Some(Duration::ZERO));
    }
}
