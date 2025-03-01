use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use crate::errors::error::Error;

#[derive(Debug)]
pub enum LockError {
    MissingParent(String),
    NoPermission(String),
    StaleLock(String),
    LockDenied(String),
}

pub struct Lockfile {
    file_path: PathBuf,
    lock_path: PathBuf,
    lock: Option<File>,
}

impl Lockfile {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let file_path = path.as_ref().to_path_buf();
        let lock_path = file_path.with_extension("lock");
        Lockfile {
            file_path,
            lock_path,
            lock: None,
        }
    }

    pub fn hold_for_update(&mut self) -> Result<bool, LockError> {
        if self.lock.is_some() {
            return Ok(true);
        }

        let dir = self.lock_path.parent().ok_or_else(|| {
            LockError::MissingParent(format!("Parent directory does not exist: {:?}", self.lock_path))
        })?;
        fs::create_dir_all(dir).map_err(|e| match e.kind() {
            io::ErrorKind::PermissionDenied => LockError::NoPermission(e.to_string()),
            _ => LockError::MissingParent(e.to_string()),
        })?;

        match OpenOptions::new()
            .write(true)
            .create_new(true) // O_CREAT | O_EXCL
            .open(&self.lock_path)
        {
            Ok(file) => {
                self.lock = Some(file);
                Ok(true)
            }
            Err(e) => match e.kind() {
                io::ErrorKind::AlreadyExists => Ok(false),
                io::ErrorKind::PermissionDenied => Err(LockError::NoPermission(e.to_string())),
                _ => Err(LockError::MissingParent(e.to_string())),
            },
        }
    }

    pub fn write(&mut self, data: &str) -> Result<(), LockError> {
        let lock = self.lock.as_mut().ok_or_else(|| {
            LockError::StaleLock("Not holding lock on file".into())
        })?;
        
        lock.write_all(data.as_bytes())
            .map_err(|e| LockError::StaleLock(e.to_string()))?;
        Ok(())
    }

    pub fn commit(mut self) -> Result<(), LockError> {
        let lock = self.lock.take().ok_or_else(|| {
            LockError::StaleLock("Not holding lock on file".into())
        })?;

        // Închide fișierul înainte de rename (necesar pe Windows)
        drop(lock);
        
        fs::rename(&self.lock_path, &self.file_path)
            .map_err(|e| LockError::StaleLock(e.to_string()))?;
        
        Ok(())
    }
}