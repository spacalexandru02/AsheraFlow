// src/core/repository/inspector.rs
use std::path::Path;
use crate::errors::error::Error;
use crate::core::database::blob::Blob;
use crate::core::database::entry::DatabaseEntry;
use crate::core::index::entry::Entry;
use crate::core::workspace::Workspace;
use crate::core::index::index::Index;
use crate::core::database::database::{Database, GitObject};

// Enum for change types in the repository
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum ChangeType {
    Untracked,
    Modified,
    Added,
    Deleted,
}

// The Inspector now takes separate references to the components it needs
// rather than a reference to the entire Repository
pub struct Inspector<'a> {
    workspace: &'a Workspace,
    index: &'a Index,
    database: &'a Database,
}

impl<'a> Inspector<'a> {
    // Updated constructor to take separate components
    pub fn new(workspace: &'a Workspace, index: &'a Index, database: &'a Database) -> Self {
        Inspector { 
            workspace,
            index,
            database,
        }
    }
    
    /// Check if a path is an untracked file or directory containing untracked files
    pub fn trackable_file(&self, path: &Path, stat: &std::fs::Metadata) -> Result<bool, Error> {
        // If it's a file, check if it's in the index
        if stat.is_file() {
            // If the file is not in the index, it's trackable
            return Ok(!self.index.tracked(&path.to_string_lossy().to_string()));
        }
        
        // If it's a directory, check if it contains any untracked files
        if stat.is_dir() {
            // Get all files in the directory
            return self.directory_contains_untracked(path);
        }
        
        // Not a file or directory (e.g., symlink), consider not trackable
        Ok(false)
    }
    
    /// Check if a directory contains any untracked files
    fn directory_contains_untracked(&self, dir_path: &Path) -> Result<bool, Error> {
        if !dir_path.is_dir() {
            return Ok(false);
        }
        
        // Get all entries in the directory
        match std::fs::read_dir(dir_path) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();
                            let file_name = entry.file_name();
                            
                            // Skip hidden files
                            if file_name.to_string_lossy().starts_with('.') {
                                continue;
                            }
                            
                            // Skip .ash directory
                            if file_name == ".ash" {
                                continue;
                            }
                            
                            // If it's a file not in the index, it's untracked
                            if path.is_file() {
                                let path_str = path.to_string_lossy().to_string();
                                if !self.index.tracked(&path_str) {
                                    return Ok(true);
                                }
                            } else if path.is_dir() {
                                // Recursively check subdirectories
                                if self.directory_contains_untracked(&path)? {
                                    return Ok(true);
                                }
                            }
                        },
                        Err(_) => continue,
                    }
                }
                Ok(false)
            },
            Err(e) => Err(Error::IO(e)),
        }
    }
    
    /// Compare an index entry to a file in the workspace
    pub fn compare_index_to_workspace(&self, 
                                     entry: Option<&Entry>, 
                                     stat: Option<&std::fs::Metadata>) -> Result<Option<ChangeType>, Error> {
        // File not in index but exists in workspace
        if entry.is_none() {
            return Ok(Some(ChangeType::Untracked));
        }
        
        let entry = entry.unwrap();
        
        // File in index but not in workspace
        if stat.is_none() {
            return Ok(Some(ChangeType::Deleted));
        }
        
        let stat = stat.unwrap();
        
        // Check file metadata (size, mode)
        if !entry.mode_match(stat) || entry.size as u64 != stat.len() {
            return Ok(Some(ChangeType::Modified));
        }
        
        // If timestamps match, assume content is the same
        if entry.time_match(stat) {
            return Ok(None);
        }
        
        // Timestamps don't match, check content
        let path = Path::new(&entry.path);
        let data = self.workspace.read_file(path)?;
        let blob = Blob::new(data);
        let oid = self.database.hash_file_data(&blob.to_bytes());
        
        if entry.oid != oid {
            Ok(Some(ChangeType::Modified))
        } else {
            // Content is the same despite timestamp difference
            Ok(None)
        }
    }
    
    /// Compare a tree entry to an index entry
    pub fn compare_tree_to_index(&self, item: Option<&DatabaseEntry>, entry: Option<&Entry>) -> Option<ChangeType> {
        // Neither exists
        if item.is_none() && entry.is_none() {
            return None;
        }
        
        // Item in tree but not in index
        if item.is_some() && entry.is_none() {
            return Some(ChangeType::Deleted);
        }
        
        // Item not in tree but in index
        if item.is_none() && entry.is_some() {
            return Some(ChangeType::Added);
        }
        
        // Both exist, compare mode and OID
        let item = item.unwrap();
        let entry = entry.unwrap();
        
        // Compare mode and object ID
        let mode_match = item.get_mode() == entry.mode_octal();
        let oid_match = item.get_oid() == entry.oid;
        
        if !mode_match || !oid_match {
            Some(ChangeType::Modified)
        } else {
            None
        }
    }
}