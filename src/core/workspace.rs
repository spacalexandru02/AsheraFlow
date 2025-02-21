use std::fs;
use std::path::{Path, PathBuf};
use crate::errors::error::Error;

pub struct Workspace {
    root_path: PathBuf,
}

impl Workspace {
    const IGNORE: [&'static str; 3] = [".", "..", ".ash"];

    pub fn new(root_path: &Path) -> Self {
        Workspace {
            root_path: root_path.to_path_buf(),
        }
    }

    pub fn list_files(&self) -> Result<Vec<String>, Error> {
        let entries = fs::read_dir(&self.root_path).map_err(|e| {
            Error::Generic(format!("Failed to read directory '{}': {}", self.root_path.display(), e))
        })?;

        let files: Vec<String> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file_name = entry.file_name().into_string().ok()?;
                let file_path = self.root_path.join(&file_name);

                // Ignore directories and special files
                if !Self::IGNORE.contains(&file_name.as_str()) && file_path.is_file() {
                    Some(file_name)
                } else {
                    None
                }
            })
            .collect();

        Ok(files)
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, Error> {
        let file_path = self.root_path.join(path);
        if !file_path.is_file() {
            return Err(Error::Generic(format!("Path '{}' is not a file", file_path.display())));
        }
        fs::read(&file_path).map_err(|e| {
            Error::Generic(format!("Failed to read file '{}': {}", file_path.display(), e))
        })
    }
}