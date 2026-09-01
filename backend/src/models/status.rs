//! Facts about the running instance that an operator wants at a glance.
//!
//! Everything here is either measured live when the status page is opened or
//! recorded as the process runs. Nothing is persisted: after a restart, uptime
//! starts again and the cleanup job reports that it has not run yet, which is
//! the honest answer.

use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Result of the last expired-share cleanup, for the maintenance view.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PurgeStatus {
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_cleared: usize,
    /// Set when the last run could not finish, so the console can say so
    /// instead of showing a reassuring zero.
    pub last_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct PurgeStatusHandle(Arc<RwLock<PurgeStatus>>);

impl PurgeStatusHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self) -> PurgeStatus {
        match self.0.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn record(&self, cleared: usize, error: Option<String>) {
        let next = PurgeStatus {
            last_run_at: Some(Utc::now()),
            last_cleared: cleared,
            last_error: error,
        };

        match self.0.write() {
            Ok(mut guard) => *guard = next,
            Err(poisoned) => *poisoned.into_inner() = next,
        }
    }
}

/// Whether a dependency answered, and how quickly.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyHealth {
    pub reachable: bool,
    pub latency_ms: u64,
    /// A short, non-leaking reason when it did not answer.
    pub detail: Option<String>,
}

/// Space on a filesystem, in the terms `df` reports it.
#[derive(Debug, Clone, Serialize)]
pub struct DiskUsage {
    /// The path measured, so the console can say what this is space *on*.
    pub path: String,
    pub total_bytes: u64,
    /// What an unprivileged process may still write. Smaller than the raw free
    /// figure, because filesystems reserve a slice for root — this is the
    /// number that matters to a process that is about to run out.
    pub available_bytes: u64,
    pub used_bytes: u64,
}

impl DiskUsage {
    pub fn used_percent(&self) -> u8 {
        let denominator = self.used_bytes.saturating_add(self.available_bytes);
        if denominator == 0 {
            return 0;
        }
        ((self.used_bytes as f64 / denominator as f64) * 100.0).round() as u8
    }
}

/// Free space on the filesystem at `path`.
///
/// Inside the container this is normally the Docker host's filesystem, which
/// is also where the Postgres and object-storage volumes live — so it is the
/// number an operator actually wants. It stops being that if those volumes
/// were bound to a different disk, which is why the path is reported alongside
/// it rather than presented as "your disk".
#[cfg(unix)]
pub fn disk_usage(path: &str) -> Option<DiskUsage> {
    use std::ffi::CString;

    let c_path = CString::new(path).ok()?;
    // SAFETY: `stat` is a plain C struct with no invariants, zeroed before the
    // call, and `c_path` is a valid NUL-terminated string that outlives it.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }

    // f_frsize is the fragment size the block counts are expressed in.
    let block = stat.f_frsize as u64;
    let total = (stat.f_blocks as u64).saturating_mul(block);
    let free = (stat.f_bfree as u64).saturating_mul(block);
    let available = (stat.f_bavail as u64).saturating_mul(block);

    Some(DiskUsage {
        path: path.to_string(),
        total_bytes: total,
        available_bytes: available,
        used_bytes: total.saturating_sub(free),
    })
}

#[cfg(not(unix))]
pub fn disk_usage(_path: &str) -> Option<DiskUsage> {
    None
}

/// Resident memory of this process, if the platform will say.
///
/// Linux only, which is where this runs in anger; a developer on macOS simply
/// sees nothing rather than a number that means something different.
pub fn process_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kib: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kib * 1024);
            }
        }
        None
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// What the database can tell about itself.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DatabaseStats {
    /// e.g. "16.4". Absent if the query failed.
    pub version: Option<String>,
    /// On-disk size of this database, as PostgreSQL accounts for it.
    pub size_bytes: Option<i64>,
}

/// What the object store holds.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BucketStats {
    pub object_count: u64,
    pub size_bytes: u64,
    /// True when the scan hit its page limit and stopped early, so the figures
    /// are a floor rather than a total.
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_reports_that_the_cleanup_has_not_run() {
        let status = PurgeStatusHandle::new().get();
        assert!(status.last_run_at.is_none());
        assert_eq!(status.last_cleared, 0);
    }

    #[test]
    fn a_recorded_run_replaces_the_previous_one() {
        let handle = PurgeStatusHandle::new();
        handle.record(3, None);
        handle.record(0, Some("storage was down".to_string()));

        let status = handle.get();
        assert_eq!(status.last_cleared, 0);
        assert_eq!(status.last_error.as_deref(), Some("storage was down"));
    }

    #[test]
    fn disk_use_is_measured_against_what_is_actually_writable() {
        // 20 used, 80 available — but 100 total, meaning 0 is reserved for
        // root. `df` calls that 20%, and so does this.
        let usage = DiskUsage {
            path: "/".to_string(),
            total_bytes: 100,
            available_bytes: 80,
            used_bytes: 20,
        };
        assert_eq!(usage.used_percent(), 20);
    }

    #[test]
    fn reserved_space_does_not_read_as_free() {
        // A filesystem with 5 bytes held back for root: 90 used, 5 available.
        // Reporting 90/100 would flatter it; what matters is 90 of the 95 a
        // normal process can reach.
        let usage = DiskUsage {
            path: "/data".to_string(),
            total_bytes: 100,
            available_bytes: 5,
            used_bytes: 90,
        };
        assert_eq!(usage.used_percent(), 95);
    }

    #[test]
    fn an_empty_filesystem_does_not_divide_by_zero() {
        let usage = DiskUsage {
            path: "/".to_string(),
            total_bytes: 0,
            available_bytes: 0,
            used_bytes: 0,
        };
        assert_eq!(usage.used_percent(), 0);
    }

    #[test]
    fn the_root_filesystem_can_be_measured() {
        // Every platform this runs on has a root filesystem with a non-zero
        // size; if this returns None the statvfs call is wrong.
        let usage = disk_usage("/").expect("root filesystem should be measurable");
        assert!(usage.total_bytes > 0, "{usage:?}");
        assert!(usage.used_bytes <= usage.total_bytes, "{usage:?}");
        assert_eq!(usage.path, "/");
    }

    #[test]
    fn a_path_that_does_not_exist_reports_nothing_rather_than_guessing() {
        assert!(disk_usage("/definitely/not/a/real/path/here").is_none());
    }
}
