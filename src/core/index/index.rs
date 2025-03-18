// src/core/index/index.rs
use std::collections::{HashMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
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
    pub entries: HashMap<String, Entry>,
    pub keys: BTreeSet<String>,
    lockfile: Lockfile,
    pub changed: bool,
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
    
    // Getters
    pub fn get_pathname(&self) -> &PathBuf {
        &self.pathname
    }
    
    pub fn get_entry(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }
    
    pub fn get_entry_mut(&mut self, key: &str) -> Option<&mut Entry> {
        self.entries.get_mut(key)
    }
    
    pub fn get_keys(&self) -> &BTreeSet<String> {
        &self.keys
    }
    
    pub fn is_changed(&self) -> bool {
        self.changed
    }
    
    pub fn set_changed(&mut self, changed: bool) {
        self.changed = changed;
    }
    
    pub fn update_entry_stat(&mut self, path: &str, stat: &std::fs::Metadata) -> Result<(), Error> {
        if let Some(entry) = self.get_entry_mut(path) {
            entry.update_stat(stat);
            self.changed = true;
            Ok(())
        } else {
            Err(Error::Generic(format!("Entry not found for key: {}", path)))
        }
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
        
        // Verifică dimensiunea fișierului
        let file_size = match file.metadata() {
            Ok(metadata) => metadata.len(),
            Err(e) => return Err(Error::IO(e)),
        };
        
        if file_size < HEADER_SIZE as u64 {
            println!("Warning: Index file too small ({} bytes), initializing new index", file_size);
            return Ok(());
        }
        
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
            match reader.read_exact(&mut entry_data) {
                Ok(_) => {},
                Err(e) => {
                    println!("Warning: Could not read entry data: {}", e);
                    return Ok(());  // Abandon reading but don't fail
                }
            }
            checksum.update(&entry_data);
            
            // Keep reading 8-byte blocks until we find a null terminator or EOF
            let mut reached_end = false;
            while !reached_end && entry_data[entry_data.len() - 1] != 0 {
                let mut block = vec![0; ENTRY_BLOCK];
                match reader.read_exact(&mut block) {
                    Ok(_) => {
                        checksum.update(&block);
                        entry_data.extend_from_slice(&block);
                    },
                    Err(_) => {
                        reached_end = true;
                    }
                }
            }
            
            if reached_end {
                break;  // Stop reading entries if we hit EOF
            }
            
            // Parse the entry
            match Entry::parse(&entry_data) {
                Ok(entry) => self.store_entry(entry),
                Err(e) => println!("Warning: Could not parse entry: {}", e)
            }
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
        
        // Write header to lockfile using write_bytes instead of write
        self.lockfile.write_bytes(&header)
            .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        
        // Write entries in sorted order
        for key in &self.keys {
            let entry = &self.entries[key];
            let bytes = entry.to_bytes();
            
            // Update checksum with entry data
            checksum.update(&bytes);
            
            // Write entry data to lockfile
            self.lockfile.write_bytes(&bytes)
                .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        }
        
        // Get the final checksum
        let digest = checksum.finalize();
        
        // Write checksum
        self.lockfile.write_bytes(&digest)
            .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        
        // Commit the changes
        self.lockfile.commit_ref()
            .map_err(|e| Error::Generic(format!("Commit error: {:?}", e)))?;
        
        // Reset the changed flag
        self.changed = false;
        
        Ok(true)
    }


    pub fn rollback(&mut self) -> Result<(), Error> {
        self.changed = false;
        self.lockfile.rollback()
            .map_err(|e| Error::Lock(format!("Failed to release lock: {:?}", e)))
    }
    
    pub fn verify_integrity(&self) -> Result<bool, Error> {
        let file_path = &self.pathname;
        
        if !file_path.exists() {
            // An empty index is valid
            return Ok(true);
        }
        
        let file = match File::open(file_path) {
            Ok(f) => f,
            Err(e) => return Err(Error::IO(e)),
        };
        
        let mut reader = file;
        let mut checksum = Checksum::new();
        
        // Read and verify the header
        let mut header_data = vec![0; HEADER_SIZE];
        match reader.read_exact(&mut header_data) {
            Ok(_) => checksum.update(&header_data),
            Err(e) => return Err(Error::IO(e)),
        }
        
        // Parse header
        let signature = String::from_utf8_lossy(&header_data[0..4]).to_string();
        if signature != HEADER_FORMAT {
            return Err(Error::Generic(format!(
                "Invalid index signature: expected '{}', got '{}'",
                HEADER_FORMAT, signature
            )));
        }
        
        let version = u32::from_be_bytes([header_data[4], header_data[5], header_data[6], header_data[7]]);
        if version != VERSION {
            return Err(Error::Generic(format!(
                "Unsupported index version: expected {}, got {}",
                VERSION, version
            )));
        }
        
        let count = u32::from_be_bytes([header_data[8], header_data[9], header_data[10], header_data[11]]);
        
        // Read the entries
        let metadata = fs::metadata(file_path)?;
        let expected_size = HEADER_SIZE as u64 + (count as u64 * 62) + CHECKSUM_SIZE as u64;
        
        if metadata.len() < expected_size {
            return Err(Error::Generic(format!(
                "Index file too small: expected at least {} bytes, got {} bytes",
                expected_size, metadata.len()
            )));
        }
        
        // Skip the entries (we're just validating the checksum)
        let mut buffer = vec![0; metadata.len() as usize - HEADER_SIZE - CHECKSUM_SIZE];
        match reader.read_exact(&mut buffer) {
            Ok(_) => checksum.update(&buffer),
            Err(e) => return Err(Error::IO(e)),
        }
        
        // Verify the checksum
        let mut stored_checksum = vec![0; CHECKSUM_SIZE];
        match reader.read_exact(&mut stored_checksum) {
            Ok(_) => {},
            Err(e) => return Err(Error::IO(e)),
        }
        
        let calculated_checksum = checksum.finalize();
        
        if stored_checksum != calculated_checksum {
            return Err(Error::Generic(format!(
                "Index checksum mismatch. Index file may be corrupted."
            )));
        }
        
        Ok(true)
    }
    
    // Method to repair index if possible
    pub fn repair(&mut self) -> Result<bool, Error> {
        // Try to load the current index
        match self.load() {
            Ok(_) => {
                // If we can load it, it's probably fine, just rewrite it
                self.changed = true;
                self.write_updates()?;
                Ok(true)
            },
            Err(_) => {
                // If we can't load it, clear and recreate it
                self.clear();
                self.changed = true;
                
                // First ensure we have a lock
                self.lockfile.hold_for_update()
                    .map_err(|e| Error::Generic(format!("Lock error: {:?}", e)))?;
                
                // Write a clean index
                self.write_updates()?;
                
                Ok(false)
            }
        }
    }
    
    // Method to check for and remove stale locks
    pub fn check_stale_locks(&self) -> Result<bool, Error> {
        let lock_path = self.pathname.with_extension("lock");
        
        if lock_path.exists() {
            // Check if the lock file is stale (e.g., more than 1 hour old)
            if let Ok(metadata) = fs::metadata(&lock_path) {
                if let Ok(modified) = metadata.modified() {
                    let now = std::time::SystemTime::now();
                    if let Ok(duration) = now.duration_since(modified) {
                        if duration.as_secs() > 3600 {
                            // Lock is more than an hour old, probably stale
                            match fs::remove_file(&lock_path) {
                                Ok(_) => {
                                    println!("Removed stale lock file: {}", lock_path.display());
                                    return Ok(true);
                                },
                                Err(e) => {
                                    return Err(Error::IO(e));
                                }
                            }
                        }
                    }
                }
            }
            
            // Lock exists but doesn't appear stale
            return Ok(false);
        }
        
        // No lock file exists
        Ok(false)
    }
    
    // Metoda helper pentru a verifica dacă un fișier este indexat
    pub fn tracked(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }
    pub fn remove_entry(&mut self, path: &str) -> Result<(), Error> {
        if self.entries.remove(path).is_some() {
            self.keys.remove(path);
            self.changed = true;
            Ok(())
        } else {
            Err(Error::Generic(format!("Entry not found for path: {}", path)))
        }
    }
}