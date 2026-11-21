//! Process liveness for lock and staging ownership checks.

/// True when a process with this pid currently exists, or when the process
/// table cannot be queried.
///
/// Callers reclaim locks and staging directories on `false`, so only a
/// completed lookup may report a process as dead.
pub fn pid_is_alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = match System::new() {
        Ok(sys) => sys,
        Err(e) => {
            tracing::warn!("process table unavailable, treating pid {pid} as alive: {e}");
            return true;
        }
    };
    let pid = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(pid).is_some()
}
