use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

pub struct FileLock {
    _file: File,
}

impl FileLock {
    pub fn acquire(lock_path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_path)?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self { _file: file }),
            Err(_) => anyhow::bail!("Another nex operation is running. Try again later."),
        }
    }
}

// Lock released automatically when FileLock is dropped (file closed)
