use std::path::Path;
use crate::errors::error::Error;

pub struct PathValidator;

impl PathValidator {
    pub fn validate(path: &str) -> Result<(), Error> {
        if path.is_empty() {
            return Err(Error::InvalidPath("Path cannot be empty".to_string()));
        }

        let path = Path::new(path);
        if !path.exists() {
            return Err(Error::InvalidPath(format!("Path '{}' does not exist", path.display())));
        }

        if !path.is_dir() {
            return Err(Error::InvalidPath(format!("'{}' is not a directory", path.display())));
        }

        Ok(())
    }
}