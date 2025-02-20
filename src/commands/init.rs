use crate::core::repository::Repository;
use crate::errors::error::Error;
use crate::validators::path_validator::PathValidator;

pub struct InitCommand;

impl InitCommand {
    pub fn execute(path: &str) -> Result<(), Error> {
        PathValidator::validate(path)?;
        
        let repo = Repository::new(path)?;
        let git_path = repo.create_git_directory()?;
        
        for dir in &["objects", "refs"] {
            repo.create_directory(&git_path.join(dir))?;
        }

        println!("Initialized empty Ash repository in {}", git_path.display());
        Ok(())
    }
}