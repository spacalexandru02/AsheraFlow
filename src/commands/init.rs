/// Implements the 'init' command for AsheraFlow.
/// Handles repository initialization and directory setup.
use crate::core::repository::repository::Repository;
use crate::errors::error::Error;
use crate::validators::path_validator::PathValidator;
use crate::core::refs::Refs;
use std::fs;
use std::path::Path;

/// Main struct for the init command logic.
pub struct InitCommand;

/// Default branch name used for new repositories.
const DEFAULT_BRANCH: &str = "master";

impl InitCommand {
    /// Executes the init command, initializing a new repository at the specified path.
    /// Returns an error if the path is invalid or directory creation fails.
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

        // Initialize HEAD to point to master branch
        let refs = Refs::new(&git_path);
        let relative_path = format!("refs/heads/{}", DEFAULT_BRANCH);
        refs.set_head(&relative_path, &format!("ref: {}", relative_path))?;

        println!("Initialized empty Ash repository in {}", git_path.display());
        Ok(())
    }
}