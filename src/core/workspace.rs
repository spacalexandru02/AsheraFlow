use std::fs;
use std::path::{Path, PathBuf};
use crate::errors::error::Error;
use std::collections::HashSet;

pub struct Workspace {
    root_path: PathBuf,
}

impl Workspace {
    pub fn new(root_path: &Path) -> Self {
        Workspace {
            root_path: root_path.to_path_buf(),
        }
    }

    // Load ignore patterns from .ashignore
    fn load_ignore_patterns(&self) -> HashSet<String> {
        let mut patterns = HashSet::new();
        let ignore_path = self.root_path.join(".ashignore");
        
        // Always ignore .ash directory
        patterns.insert(".ash".to_string());
        patterns.insert(".ash/*".to_string());
        
        if ignore_path.exists() {
            if let Ok(content) = fs::read_to_string(ignore_path) {
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty() && !line.starts_with('#') {
                        patterns.insert(line.to_string());
                    }
                }
            }
        }
        
        patterns
    }
    
    // List files recursively, applying ignore patterns
    pub fn list_files(&self) -> Result<Vec<PathBuf>, Error> {
        let ignore_patterns = self.load_ignore_patterns();
        let mut files = Vec::new();
        self.list_files_recursive(&self.root_path, &mut files, &ignore_patterns)?;
        Ok(files)
    }

    // List files starting from a specific path (for add command)
    pub fn list_files_from(&self, start_path: &Path) -> Result<Vec<PathBuf>, Error> {
        let ignore_patterns = self.load_ignore_patterns();
        let mut files = Vec::new();
        
        // Convert to absolute path if it's not already
        let abs_start_path = if start_path.is_absolute() {
            start_path.to_path_buf()
        } else {
            self.root_path.join(start_path)
        };
        
        // Check if path exists
        if !abs_start_path.exists() {
            return Ok(Vec::new());
        }
        
        if abs_start_path.is_dir() {
            // For directories, we need to recursively list files
            for entry in fs::read_dir(&abs_start_path)? {
                let entry = entry?;
                let entry_path = entry.path();
                
                if entry_path.is_dir() {
                    // Recursively process subdirectories
                    let mut sub_files = self.list_files_from(&entry_path)?;
                    files.append(&mut sub_files);
                } else {
                    // For files, check if they should be ignored
                    if let Ok(rel_path) = entry_path.strip_prefix(&self.root_path) {
                        let rel_path_str = rel_path.to_string_lossy().to_string();
                        if !self.matches_any_pattern(&rel_path_str, &ignore_patterns) {
                            files.push(rel_path.to_path_buf());
                        }
                    }
                }
            }
        } else {
            // For individual files, add them directly if they're not ignored
            if let Ok(rel_path) = abs_start_path.strip_prefix(&self.root_path) {
                let rel_path_str = rel_path.to_string_lossy().to_string();
                if !self.matches_any_pattern(&rel_path_str, &ignore_patterns) {
                    files.push(rel_path.to_path_buf());
                }
            }
        }
        
        Ok(files)
    }

    fn list_files_recursive(
        &self,
        path: &Path,
        files: &mut Vec<PathBuf>,
        ignore_patterns: &HashSet<String>,
    ) -> Result<(), Error> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            
            // Get the path relative to the root for pattern matching
            let relative_path = match entry_path.strip_prefix(&self.root_path) {
                Ok(rel_path) => rel_path,
                Err(_) => continue, // Skip if we can't get relative path
            };
            
            // Convert to string for pattern matching
            let rel_path_str = relative_path.to_string_lossy().to_string();
            
            // Check if this path should be ignored
            if self.matches_any_pattern(&rel_path_str, ignore_patterns) {
                continue;
            }
            
            if entry_path.is_dir() {
                self.list_files_recursive(&entry_path, files, ignore_patterns)?;
            } else {
                files.push(relative_path.to_path_buf());
            }
        }
        Ok(())
    }
    
    // Check if a path matches any ignore pattern
    fn matches_any_pattern(&self, path: &str, patterns: &HashSet<String>) -> bool {
        for pattern in patterns {
            if self.matches_pattern(path, pattern) {
                return true;
            }
        }
        false
    }
    
    // Simple pattern matching for ignore files
    fn matches_pattern(&self, path: &str, pattern: &str) -> bool {
        // Exact match
        if path == pattern {
            return true;
        }
        
        // Check for directory patterns (ending with /)
        if pattern.ends_with('/') {
            let dir_pattern = &pattern[0..pattern.len()-1];
            if path.starts_with(dir_pattern) {
                return true;
            }
        }
        
        // Handle wildcard patterns
        if pattern.contains('*') {
            // Convert glob pattern to regex pattern
            let regex_pattern = pattern
                .replace(".", "\\.")
                .replace("*", ".*");
            
            if let Ok(re) = regex::Regex::new(&format!("^{}$", regex_pattern)) {
                return re.is_match(path);
            }
        }
        
        false
    }

    pub fn read_file(&self, path: &Path) -> Result<Vec<u8>, Error> {
        let file_path = self.root_path.join(path);
        fs::read(&file_path).map_err(Into::into)
    }
    
    // Get file metadata
    pub fn stat_file(&self, path: &Path) -> Result<fs::Metadata, Error> {
        let file_path = self.root_path.join(path);
        fs::metadata(&file_path).map_err(Into::into)
    }
    
    // Check if path exists
    pub fn path_exists(&self, path: &Path) -> Result<bool, Error> {
        let file_path = self.root_path.join(path);
        Ok(file_path.exists())
    }
}