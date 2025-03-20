// Modified src/commands/init.rs
use crate::core::repository::repository::Repository;
use crate::errors::error::Error;
use crate::validators::path_validator::PathValidator;
use std::fs;
use std::path::Path;

pub struct InitCommand;

impl InitCommand {
    pub fn execute(path: &str) -> Result<(), Error> {
        // Use the init-specific validator
        PathValidator::validate_for_init(path)?;
        
        // Create the directory if it doesn't exist
        let path_obj = Path::new(path);
        if !path_obj.exists() {
            fs::create_dir_all(path_obj).map_err(|e| {
                Error::DirectoryCreation(format!(
                    "Failed to create directory '{}': {}",
                    path_obj.display(),
                    e
                ))
            })?;
        }
        
        // Initialize the repository
        let repo = Repository::new(path)?;
        let git_path = repo.create_git_directory()?;
        
        for dir in &["objects", "refs", "refs/heads"] {
            repo.create_directory(&git_path.join(dir))?;
        }

        println!("Initialized empty Ash repository in {}", git_path.display());
        Ok(())
    }
}