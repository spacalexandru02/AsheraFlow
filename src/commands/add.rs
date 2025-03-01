use std::path::{Path, PathBuf};
use crate::core::workspace::Workspace;
use crate::core::database::Database;
use crate::core::blob::Blob;
use crate::core::index::Index;
use crate::errors::error::Error;

pub struct AddCommand;

impl AddCommand {
    pub fn execute(path_str: &str) -> Result<(), Error> {
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        let workspace = Workspace::new(root_path);
        let mut database = Database::new(git_path.join("objects"));
        let mut index = Index::new(git_path.join("index"));
        
        let path = PathBuf::from(path_str);
        
        // Validate path exists
        if !workspace.path_exists(&path)? {
            return Err(Error::InvalidPath(format!("Path '{}' does not exist", path.display())));
        }
        
        // Read file data
        let data = workspace.read_file(&path)?;
        
        // Get file metadata
        let stat = workspace.stat_file(&path)?;
        
        // Create blob and store it
        let mut blob = Blob::new(data);
        database.store(&mut blob)?;
        
        // Get OID
        let oid = blob.get_oid()
            .ok_or_else(|| Error::Generic("Blob OID not set after storage".into()))?;
        
        // Add to index
        index.add(&path, oid, &stat)?;
        
        // Write index updates
        match index.write_updates()? {
            true => println!("Added {} to index", path.display()),
            false => return Err(Error::Generic("Failed to update index".into())),
        }
        
        Ok(())
    }
}