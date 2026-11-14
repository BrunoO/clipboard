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

        match file.try_lock() {
            Ok(()) => {
                // Best-effort: record our PID for diagnostics
                let _ = file.set_len(0);
                let _ = file.seek(SeekFrom::Start(0));
                let _ = writeln!(file, "{}", std::process::id());
                let _ = file.flush();
                Ok(AcquireResult::Acquired(Self { _file: file }))
            }
            Err(TryLockError::WouldBlock) => {
                let mut contents = String::new();
                let pid = if file.read_to_string(&mut contents).is_ok() {
                    contents.trim().parse::<u32>().ok()
                } else {
                    None
                };
                Ok(AcquireResult::AlreadyRunning { pid })
            }
            Err(TryLockError::Error(err)) => Err(err),
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
