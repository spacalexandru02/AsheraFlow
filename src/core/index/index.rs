use std::collections::{HashMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use sha1::{Digest, Sha1};

use crate::errors::error::Error;
use crate::core::lockfile::Lockfile;
use crate::core::index::entry::Entry;
use crate::core::index::checksum::Checksum;
use crate::core::index::checksum::CHECKSUM_SIZE;

const HEADER_FORMAT: &str = "DIRC";
const VERSION: u32 = 2;
const HEADER_SIZE: usize = 12;

pub struct Index {
    pathname: PathBuf,
    entries: HashMap<String, Entry>,
    keys: BTreeSet<String>,
    lockfile: Lockfile,
    changed: bool,
}

impl Index {
    pub fn new<P: AsRef<Path>>(pathname: P) -> Self {
        let mut index = Index {
            pathname: pathname.as_ref().to_path_buf(),
            entries: HashMap::new(),
            keys: BTreeSet::new(),
            lockfile: Lockfile::new(pathname),
            changed: false,
        };
        
        index.clear();
        index
    }
    
    fn clear(&mut self) {
        self.entries.clear();
        self.keys.clear();
        self.changed = false;
    }

    pub fn add(&mut self, pathname: &Path, oid: &str, stat: &fs::Metadata) -> Result<(), Error> {
        let entry = Entry::create(pathname, oid, stat);
        self.store_entry(entry);
        self.changed = true;
        Ok(())
    }
    
    fn store_entry(&mut self, entry: Entry) {
        let key = entry.get_path().to_string();
        self.keys.insert(key.clone());
        self.entries.insert(key, entry);
    }
    
    pub fn each_entry(&self) -> impl Iterator<Item = &Entry> {
        self.keys.iter().map(move |key| &self.entries[key])
    }
    
    pub fn load_for_update(&mut self) -> Result<bool, Error> {
        let acquired = self.lockfile.hold_for_update()
            .map_err(|e| Error::Generic(format!("Lock error: {:?}", e)))?;
        
        if acquired {
            self.load()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    // Load the index without acquiring a lock (for read-only operations)
    pub fn load(&mut self) -> Result<(), Error> {
        self.clear();
        
        // Try to open the index file
        let file = match File::open(&self.pathname) {
            Ok(file) => file,
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    // It's ok if the index doesn't exist yet
                    return Ok(());
                }
                return Err(Error::IO(e));
            }
        };
        
        let mut reader = file;
        let mut checksum = Checksum::new();
        
        // Read header
        let mut header_data = vec![0; HEADER_SIZE];
        reader.read_exact(&mut header_data)?;
        checksum.update(&header_data);
        
        // Parse header: signature (4 bytes), version (4 bytes), entry count (4 bytes)
        let signature = String::from_utf8_lossy(&header_data[0..4]).to_string();
        let version = u32::from_be_bytes([header_data[4], header_data[5], header_data[6], header_data[7]]);
        let count = u32::from_be_bytes([header_data[8], header_data[9], header_data[10], header_data[11]]);
        
        if signature != HEADER_FORMAT {
            return Err(Error::Generic(format!(
                "Signature: expected '{}' but found '{}'",
                HEADER_FORMAT, signature
            )));
        }
        
        if version != VERSION {
            return Err(Error::Generic(format!(
                "Version: expected '{}' but found '{}'",
                VERSION, version
            )));
        }
        
        // Read entries
        self.read_entries(&mut reader, &mut checksum, count)?;
        
        // Verify checksum
        let mut stored_checksum = vec![0; CHECKSUM_SIZE];
        reader.read_exact(&mut stored_checksum)?;
        checksum.verify(&stored_checksum)?;
        
        Ok(())
    }
    
    fn read_entries(&mut self, reader: &mut impl Read, checksum: &mut Checksum, count: u32) -> Result<(), Error> {
        const ENTRY_MIN_SIZE: usize = 64;  // Minimum size of an entry
        const ENTRY_BLOCK: usize = 8;      // Entries are padded to 8-byte blocks
        
        for _ in 0..count {
            // Read the minimum entry size first
            let mut entry_data = vec![0; ENTRY_MIN_SIZE];
            reader.read_exact(&mut entry_data)?;
            checksum.update(&entry_data);
            
            // Keep reading 8-byte blocks until we find a null terminator
            while entry_data[entry_data.len() - 1] != 0 {
                let mut block = vec![0; ENTRY_BLOCK];
                reader.read_exact(&mut block)?;
                checksum.update(&block);
                entry_data.extend_from_slice(&block);
            }
            
            // Parse the entry
            let entry = Entry::parse(&entry_data)?;
            
            // Store the entry
            self.store_entry(entry);
        }
        
        Ok(())
    }
    
    pub fn write_updates(&mut self) -> Result<bool, Error> {
        // If no changes were made, just release the lock and return
        if !self.changed {
            self.lockfile.rollback()
                .map_err(|e| Error::Generic(format!("Rollback error: {:?}", e)))?;
            return Ok(false);
        }
        
        // Initialize checksum
        let mut checksum = Checksum::new();
        
        // Generate header
        let entry_count = self.entries.len() as u32;
        let mut header = Vec::with_capacity(HEADER_SIZE);
        header.extend_from_slice(HEADER_FORMAT.as_bytes());
        header.extend_from_slice(&VERSION.to_be_bytes());
        header.extend_from_slice(&entry_count.to_be_bytes());
        
        // Update checksum with header
        checksum.update(&header);
        
        // Write header to lockfile
        self.lockfile.write(&String::from_utf8_lossy(&header))
            .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        
        // Write entries in sorted order
        for key in &self.keys {
            let entry = &self.entries[key];
            let bytes = entry.to_bytes();
            
            // Update checksum with entry data
            checksum.update(&bytes);
            
            // Write entry data to lockfile
            self.lockfile.write(&String::from_utf8_lossy(&bytes))
                .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        }
        
        // Get the final checksum
        let digest = checksum.finalize();
        
        // Write checksum
        self.lockfile.write(&String::from_utf8_lossy(&digest))
            .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        
        // Commit the changes
        self.lockfile.commit_ref()
            .map_err(|e| Error::Generic(format!("Commit error: {:?}", e)))?;
        
        // Reset the changed flag
        self.changed = false;
        
        Ok(true)
    }
}