//! Single-instance guard: a named mutex in the user session.

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

const BASE_NAME: &str = "Local\\TransClip-Daemon";

/// Holds the mutex until dropped.
pub struct InstanceGuard(HANDLE);

/// Outcome of trying to become the daemon.
pub enum Instance {
    /// We are the only daemon.
    First(InstanceGuard),
    /// Another daemon already runs.
    Already,
}

fn mutex_name() -> String {
    // A data-dir override (tests, portable use) gets its own mutex so it cannot collide with
    // the real daemon of the same user.
    match std::env::var_os(crate::paths::DATA_DIR_ENV) {
        Some(dir) => {
            let mut h: u32 = 0x811c_9dc5;
            for b in dir.to_string_lossy().bytes() {
                h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
            }
            format!("{BASE_NAME}-{h:08x}")
        }
        None => BASE_NAME.to_string(),
    }
}

/// Try to become the one daemon of this user session.
pub fn acquire() -> anyhow::Result<Instance> {
    let name = HSTRING::from(mutex_name());
    // SAFETY: `name` is a valid NUL-terminated wide string that outlives the call; the
    // returned handle is owned by `InstanceGuard` and closed exactly once in `Drop`.
    unsafe {
        let handle = CreateMutexW(None, false, &name)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return Ok(Instance::Already);
        }
        Ok(Instance::First(InstanceGuard(handle)))
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed only here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
