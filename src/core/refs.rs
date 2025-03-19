// src/core/refs.rs
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use regex::Regex;
use crate::errors::error::Error;
use crate::core::lockfile::Lockfile;

pub struct Refs {
    pathname: PathBuf,
    refs_path: PathBuf,
    heads_path: PathBuf,
}

impl Refs {
    pub fn new<P: AsRef<Path>>(pathname: P) -> Self {
        let path = pathname.as_ref().to_path_buf();
        let refs_path = path.join("refs");
        let heads_path = refs_path.join("heads");
        
        Refs {
            pathname: path,
            refs_path,
            heads_path,
        }
    }

    pub fn read_head(&self) -> Result<Option<String>, Error> {
        let head_path = self.pathname.join("HEAD");
        if !head_path.exists() {
            return Ok(None);
        }
        
        self.read_ref_file(&head_path)
    }

    pub fn update_head(&self, oid: &str) -> Result<(), Error> {
        let head_path = self.pathname.join("HEAD");
        self.update_ref_file(&head_path, oid)
    }
    
    /// Create a new branch pointing to the specified commit OID
    pub fn create_branch(&self, branch_name: &str, oid: &str) -> Result<(), Error> {
        // Validate branch name using regex pattern for invalid names
        if !self.is_valid_branch_name(branch_name) {
            return Err(Error::Generic(format!(
                "'{}' is not a valid branch name.", branch_name
            )));
        }
        
        // Check if branch already exists
        let branch_path = self.heads_path.join(branch_name);
        if branch_path.exists() {
            return Err(Error::Generic(format!(
                "A branch named '{}' already exists.", branch_name
            )));
        }
        
        // Create the branch reference file
        self.update_ref_file(&branch_path, oid)
    }
    
    // Read a reference by name (branch, HEAD, etc.)
    pub fn read_ref(&self, name: &str) -> Result<Option<String>, Error> {
        // Check for HEAD alias
        if name == "@" || name == "HEAD" {
            return self.read_head();
        }
        
        // Look in multiple locations in order:
        // 1. Direct under .ash directory
        // 2. Under .ash/refs
        // 3. Under .ash/refs/heads (branches)
        let paths = [
            self.pathname.join(name),
            self.refs_path.join(name),
            self.heads_path.join(name),
        ];
        
        for path in &paths {
            if path.exists() {
                return self.read_ref_file(path);
            }
        }
        
        // Reference not found
        Ok(None)
    }
    
    // Helper to read a reference file
    fn read_ref_file(&self, path: &Path) -> Result<Option<String>, Error> {
        if !path.exists() {
            return Ok(None);
        }
        
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => return Ok(None),
        };
        
        let mut contents = String::new();
        match file.read_to_string(&mut contents) {
            Ok(_) => Ok(Some(contents.trim().to_string())),
            Err(_) => Ok(None),
        }
    }
    
    // Update a reference file with proper locking
    fn update_ref_file(&self, path: &Path, oid: &str) -> Result<(), Error> {
        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                Error::DirectoryCreation(format!(
                    "Failed to create directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        
        // Create a lockfile for safe writing
        let mut lockfile = Lockfile::new(path);
        
        // Acquire the lock
        let acquired = lockfile.hold_for_update()
            .map_err(|e| Error::Generic(format!("Lock error: {:?}", e)))?;
        
        if !acquired {
            return Err(Error::Generic(format!(
                "Could not acquire lock on '{}'", path.display()
            )));
        }
        
        // Write the OID with a newline
        lockfile.write(&format!("{}\n", oid))
            .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        
        // Commit the changes
        lockfile.commit_ref()
            .map_err(|e| Error::Generic(format!("Commit error: {:?}", e)))?;
        
        Ok(())
    }
    
    // Check if a branch name is valid (not matching the invalid patterns)
    fn is_valid_branch_name(&self, name: &str) -> bool {
        // Define invalid patterns for branch names
        lazy_static::lazy_static! {
            static ref INVALID_NAME: Regex = Regex::new(r"(?x)
                ^\.|              # starts with .
                /\.|              # contains a path component starting with .
                \.\.|             # contains ..
                ^/|               # starts with /
                /$|               # ends with /
                \.lock$|          # ends with .lock
                @\{|              # contains @{
                [\x00-\x20*:?\[\\\^~\x7f] # contains control chars or special chars
            ").unwrap();
        }
        
        // Empty names are invalid
        if name.is_empty() {
            return false;
        }
        
        // Check against the invalid patterns
        !INVALID_NAME.is_match(name)
    }
}