use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use crate::errors::error::Error;

use super::lockfile::Lockfile;

pub struct Refs {
    pathname: PathBuf,
}

impl Refs {
    pub fn new<P: AsRef<Path>>(pathname: P) -> Self {
        Refs {
            pathname: pathname.as_ref().to_path_buf(),
        }
    }

    pub fn read_head(&self) -> Result<Option<String>, Error> {
        let head_path = self.pathname.join("HEAD");
        if !head_path.exists() {
            return Ok(None);
        }
        
        let mut contents = String::new();
        File::open(head_path)?.read_to_string(&mut contents)?;
        Ok(Some(contents.trim().to_string()))
    }

    pub fn update_head(&self, oid: &str) -> Result<(), Error> {
        let head_path = self.pathname.join("HEAD");
        let mut lockfile = Lockfile::new(&head_path);
        
        // Acquire lock
        let acquired = lockfile.hold_for_update()
            .map_err(|e| Error::Generic(format!("Lock error: {:?}", e)))?;
        if !acquired {
            return Err(Error::Generic("Could not acquire lock on HEAD".into()));
        }
        
        // Write OID and newline
        lockfile.write(&format!("{}\n", oid))
            .map_err(|e| Error::Generic(format!("Write error: {:?}", e)))?;
        
        // Commit changes using the non-consuming method
        lockfile.commit_ref()
            .map_err(|e| Error::Generic(format!("Commit error: {:?}", e)))?;
        
        Ok(())
    }
}