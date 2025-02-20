use std::path::{Path, PathBuf};
use std::fs;
use crate::errors::error::Error;

pub struct Repository {
    path: PathBuf,
}

impl Repository {
    pub fn new(path: &str) -> Result<Self, Error> {
        Ok(Repository {
            path: PathBuf::from(path).canonicalize().map_err(|e| {
                Error::PathResolution(format!("Failed to resolve path '{}': {}", path, e))
            })?
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