use std::fs;
use std::path::{Path, PathBuf};
use crate::errors::error::Error;

pub struct Workspace {
    root_path: PathBuf,
}

impl Workspace {
    const IGNORE: [&'static str; 3] = [".", "..", ".git"];

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
                if !Self::IGNORE.contains(&file_name.as_str()) {
                    Some(file_name)
                } else {
                    None
                }
            })
            .collect();

        Ok(files)
    }
}