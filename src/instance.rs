use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Holds an OS-level exclusive advisory lock on `.instance.lock` for the lifetime of the process.
/// Automatically released by the OS kernel when dropped or if the process terminates.
#[derive(Debug)]
pub(crate) struct SingleInstanceLock {
    _file: File,
}

/// Outcome of attempting to acquire the single-instance lock.
#[derive(Debug)]
pub(crate) enum AcquireResult {
    Acquired(SingleInstanceLock),
    AlreadyRunning { pid: Option<u32> },
}

impl SingleInstanceLock {
    /// Attempts to acquire an exclusive non-blocking lock on `lock_path`.
    pub(crate) fn acquire(lock_path: &Path) -> std::io::Result<AcquireResult> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;

        #[cfg(windows)]
        let lock_result = lock_instance_file(&file);
        #[cfg(not(windows))]
        let lock_result = file.try_lock();

        match lock_result {
            Ok(()) => {
                // Best-effort: record our PID for diagnostics
                let _ = file.set_len(0);
                let _ = file.seek(SeekFrom::Start(0));
                let _ = writeln!(file, "{}", std::process::id());
                // Pad so a single-byte lock region beyond the PID stays valid on Windows.
                let _ = file.set_len(1025);
                let _ = file.flush();
                Ok(AcquireResult::Acquired(Self { _file: file }))
            }
            Err(TryLockError::WouldBlock) => {
                let mut contents = String::new();
                let pid = if file.read_to_string(&mut contents).is_ok() {
                    contents
                        .lines()
                        .next()
                        .and_then(|line| line.trim().trim_matches('\0').parse::<u32>().ok())
                } else {
                    None
                };
                Ok(AcquireResult::AlreadyRunning { pid })
            }
            Err(TryLockError::Error(err)) => Err(err),
        }
    }
}

#[cfg(windows)]
fn lock_instance_file(file: &std::fs::File) -> Result<(), TryLockError> {
    use std::os::windows::io::AsRawHandle;

    // Ensure the lock region exists beyond the PID text.
    let _ = file.set_len(1025);

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        h_event: *mut std::ffi::c_void,
    }

    extern "system" {
        fn LockFileEx(
            h_file: *mut std::ffi::c_void,
            dw_flags: u32,
            dw_reserved: u32,
            n_number_of_bytes_to_lock_low: u32,
            n_number_of_bytes_to_lock_high: u32,
            lp_overlapped: *mut Overlapped,
        ) -> i32;
    }

    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 2;
    const LOCKFILE_FAIL_IMMEDIATELY: u32 = 1;
    const ERROR_LOCK_VIOLATION: i32 = 33;

    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 1024,
        offset_high: 0,
        h_event: std::ptr::null_mut(),
    };

    let result = unsafe {
        LockFileEx(
            file.as_raw_handle() as *mut std::ffi::c_void,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    };

    if result != 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(ERROR_LOCK_VIOLATION) {
            Err(TryLockError::WouldBlock)
        } else {
            Err(TryLockError::Error(err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_single_instance_lock_prevents_duplicate_and_releases_on_drop() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join(".instance.lock");

        let first = match SingleInstanceLock::acquire(&lock_path).unwrap() {
            AcquireResult::Acquired(lock) => lock,
            AcquireResult::AlreadyRunning { .. } => panic!("Expected first acquire to succeed"),
        };

        match SingleInstanceLock::acquire(&lock_path).unwrap() {
            AcquireResult::AlreadyRunning { pid } => {
                assert_eq!(pid, Some(std::process::id()));
            }
            AcquireResult::Acquired(_) => {
                panic!("Expected second acquire to report AlreadyRunning")
            }
        }

        drop(first);

        match SingleInstanceLock::acquire(&lock_path).unwrap() {
            AcquireResult::Acquired(_) => {}
            AcquireResult::AlreadyRunning { .. } => {
                panic!("Expected acquire after drop to succeed")
            }
        }
    }
}
