use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use sha1::{Digest, Sha1};

use crate::errors::error::Error;
use crate::core::lockfile::Lockfile;
use crate::core::index::entry::Entry;

pub mod entry;

const HEADER_FORMAT: &str = "DIRC";
const VERSION: u32 = 2;

pub struct Index {
    pathname: PathBuf,
    entries: HashMap<String, Entry>,
    lockfile: Lockfile,
}

impl Index {
    pub fn new<P: AsRef<Path>>(pathname: P) -> Self {
        Index {
            pathname: pathname.as_ref().to_path_buf(),
            entries: HashMap::new(),
            lockfile: Lockfile::new(pathname),
        }
    }

    pub fn add(&mut self, pathname: &Path, oid: &str, stat: &fs::Metadata) -> Result<(), Error> {
        let entry = Entry::create(pathname, oid, stat);
        self.entries.insert(pathname.to_string_lossy().to_string(), entry);
        Ok(())
    }

    pub fn write_updates(&mut self) -> Result<bool, Error> {
        // Acquire lock
        let acquired = self.lockfile.hold_for_update()
            .map_err(|e| Error::Generic(format!("Lock error: {:?}", e)))?;
        
        if !acquired {
            return Ok(false);
        }

        // Begin writing (initialize hash)
        let mut hasher = Sha1::new();
        
        // Write header
        let entry_count = self.entries.len() as u32;
        let header = format!("{}{:08x}{:08x}", HEADER_FORMAT, VERSION, entry_count);
        
        self.lockfile.write(&header)
            .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        hasher.update(header.as_bytes());
        
        // Write entries
        for (_key, entry) in &self.entries {
            let bytes = entry.to_bytes();
            self.lockfile.write(&String::from_utf8_lossy(&bytes))
                .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
            hasher.update(&bytes);
        }
        
        // Finish by writing digest and committing
        let digest = hasher.finalize();
        let digest_str = format!("{:x}", digest);
        
        self.lockfile.write(&digest_str)
            .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        
        // Use the commit_ref method which doesn't consume self
        self.lockfile.commit_ref()
            .map_err(|e| Error::Generic(format!("Commit error: {:?}", e)))?;
        
        Ok(true)
    }

    // We've moved these methods directly into write_updates to avoid borrow checker issues
}