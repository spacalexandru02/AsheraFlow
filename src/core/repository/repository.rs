// src/core/repository.rs
use std::path::{Path, PathBuf};
use std::fs;
use crate::errors::error::Error;
use crate::core::database::database::Database;
use crate::core::refs::Refs;

pub struct Repository {
    pub path: PathBuf,
    pub database: Database,
    pub refs: Refs,
}

impl Repository {
    pub fn new(path: &str) -> Result<Self, Error> {
        let path_buf = PathBuf::from(path).canonicalize().map_err(|e| {
            Error::PathResolution(format!("Failed to resolve path '{}': {}", path, e))
        })?;
        
        let git_path = path_buf.join(".ash");
        
        
        let db_path = git_path.join("objects");
        
        Ok(Repository {
            path: path_buf.clone(),
            database: Database::new(db_path),
            refs: Refs::new(&git_path),
        })
    }

    pub fn create_git_directory(&self) -> Result<PathBuf, Error> {
        let git_path = self.path.join(".ash");
        self.create_directory(&git_path)?;
        Ok(git_path)
    }

    pub fn create_directory(&self, path: &Path) -> Result<(), Error> {
        fs::create_dir_all(path).map_err(|e| {
            Error::DirectoryCreation(format!(
                "Failed to create directory '{}': {}",
                path.display(),
                e
            ))
        })
    }
    
}