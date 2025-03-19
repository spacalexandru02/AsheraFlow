use std::fs;
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use regex::Regex;
use crate::errors::error::Error;

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
    pub fn list_files_from(&self, start_path: &Path, index_entries: &HashMap<String, String>) -> Result<(Vec<PathBuf>, Vec<String>), Error> {
        let mut files_found = Vec::new();
        let mut files_missing = Vec::new();
        
        // Convertim la calea absolută dacă nu este deja
        let abs_start_path = if start_path.is_absolute() {
            start_path.to_path_buf()
        } else {
            self.root_path.join(start_path)
        };
        
        // Verifică dacă calea există
        if !abs_start_path.exists() {
            return Err(Error::InvalidPath(format!(
                "Path '{}' does not exist", abs_start_path.display()
            )));
        }
        
        // Colectează toate fișierele indexate care ar trebui să fie sub această cale
        let rel_start_path = if abs_start_path == self.root_path {
            PathBuf::new()
        } else {
            match abs_start_path.strip_prefix(&self.root_path) {
                Ok(rel) => rel.to_path_buf(),
                Err(_) => return Err(Error::InvalidPath(format!(
                    "Cannot make '{}' relative to repository root", abs_start_path.display()
                )))
            }
        };
        
        let path_prefix = rel_start_path.to_string_lossy().to_string();
        let mut expected_files = HashSet::new();
        
        // Colectează toate fișierele din index care încep cu acest prefix
        for index_path in index_entries.keys() {
            if index_path == &path_prefix || (path_prefix.is_empty() || index_path.starts_with(&format!("{}/", path_prefix))) {
                expected_files.insert(index_path.clone());
            }
        }
        
        // Dacă este un director, procesează-l recursiv
        if abs_start_path.is_dir() {
            // Încarcă modelele de ignorare
            let ignore_patterns = self.load_ignore_patterns();
            
            // Procesează recursiv directorul pentru a găsi fișierele
            let process_result = self.process_directory(
                &abs_start_path, 
                &rel_start_path, 
                &ignore_patterns, 
                &mut files_found,
                &mut expected_files
            );
            
            if let Err(e) = process_result {
                return Err(e);
            }
            
            // Orice fișier rămas în expected_files nu a fost găsit pe disc, deci a fost șters
            for missing_path in expected_files {
                files_missing.push(missing_path);
            }
        } else {
            // Pentru fișiere individuale, adaugă-le direct dacă nu sunt ignorate
            let rel_path_str = rel_start_path.to_string_lossy().to_string();
            let ignore_patterns = self.load_ignore_patterns();
            
            if !self.matches_any_pattern(&rel_path_str, &ignore_patterns) {
                files_found.push(rel_start_path);
            }
            
            // Elimină din fișierele așteptate
            expected_files.remove(&rel_path_str);
        }
        
        Ok((files_found, files_missing))
    }
    
    // Helper pentru procesarea recursivă a directoarelor
    fn process_directory(
        &self,
        abs_path: &Path,
        rel_path: &Path,
        ignore_patterns: &HashSet<String>,
        files: &mut Vec<PathBuf>,
        expected_files: &mut HashSet<String>
    ) -> Result<(), Error> {
        match fs::read_dir(abs_path) {
            Ok(entries) => {
                for entry_result in entries {
                    match entry_result {
                        Ok(entry) => {
                            let entry_path = entry.path();
                            
                            // Obține calea relativă
                            let entry_rel_path = if rel_path.as_os_str().is_empty() {
                                PathBuf::from(entry.file_name())
                            } else {
                                rel_path.join(entry.file_name())
                            };
                            
                            // Convertă la string pentru verificarea ignorării
                            let rel_path_str = entry_rel_path.to_string_lossy().to_string();
                            
                            // Verifică dacă această cale trebuie ignorată
                            if self.matches_any_pattern(&rel_path_str, ignore_patterns) {
                                continue;
                            }
                            
                            if entry_path.is_dir() {
                                // Procesează recursiv subdirectoarele
                                self.process_directory(
                                    &entry_path, 
                                    &entry_rel_path, 
                                    ignore_patterns, 
                                    files,
                                    expected_files
                                )?;
                            } else {
                                // Adaugă fișierul la lista găsită
                                files.push(entry_rel_path.clone());
                                
                                // Marchează fișierul ca fiind găsit
                                expected_files.remove(&rel_path_str);
                            }
                        },
                        Err(e) => return Err(Error::IO(e)),
                    }
                }
                Ok(())
            },
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    Err(Error::Generic(format!(
                        "open('{}'): Permission denied", abs_path.display()
                    )))
                } else {
                    Err(Error::IO(e))
                }
            }
        }
    }

    fn list_files_recursive(
        &self,
        path: &Path,
        files: &mut Vec<PathBuf>,
        ignore_patterns: &HashSet<String>,
    ) -> Result<(), Error> {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry_result in entries {
                    match entry_result {
                        Ok(entry) => {
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
                        },
                        Err(e) => return Err(Error::IO(e)),
                    }
                }
                Ok(())
            },
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    Err(Error::Generic(format!(
                        "open('{}'): Permission denied", path.display()
                    )))
                } else {
                    Err(Error::IO(e))
                }
            }
        }
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
            
            if let Ok(re) = Regex::new(&format!("^{}$", regex_pattern)) {
                return re.is_match(path);
            }
        }
        
        false
    }

    pub fn read_file(&self, path: &Path) -> Result<Vec<u8>, Error> {
        let file_path = self.root_path.join(path);
        match fs::read(&file_path) {
            Ok(data) => Ok(data),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    Err(Error::Generic(format!(
                        "open('{}'): Permission denied", path.display()
                    )))
                } else {
                    Err(Error::IO(e))
                }
            }
        }
    }
    
    // Get file metadata
    pub fn stat_file(&self, path: &Path) -> Result<fs::Metadata, Error> {
        let file_path = self.root_path.join(path);
        match fs::metadata(&file_path) {
            Ok(metadata) => Ok(metadata),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    Err(Error::Generic(format!(
                        "stat('{}'): Permission denied", path.display()
                    )))
                } else {
                    Err(Error::IO(e))
                }
            }
        }
    }

    pub fn path_exists(&self, path: &Path) -> Result<bool, Error> {
        let file_path = self.root_path.join(path);
        Ok(file_path.exists())
    }

    pub fn write_file(&self, path: &Path, data: &[u8]) -> Result<(), Error> {
        let full_path = self.root_path.join(path);
        
        // Make sure parent directory exists
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Error::IO(e)
            })?;
        }
        
        // Write the file data
        std::fs::write(&full_path, data).map_err(|e| {
            Error::IO(e)
        })
    }

    /// Remove a file from the workspace
    pub fn remove_file(&self, path: &Path) -> Result<(), Error> {
        let full_path = self.root_path.join(path);
        
        if full_path.exists() {
            if full_path.is_file() {
                std::fs::remove_file(&full_path).map_err(|e| {
                    Error::IO(e)
                })?;
            } else if full_path.is_dir() {
                // Remove directory and all its contents
                std::fs::remove_dir_all(&full_path).map_err(|e| {
                    Error::IO(e)
                })?;
            }
        }
        
        Ok(())
    }
    
    /// Try to remove a directory (only if empty)
    pub fn remove_directory(&self, path: &Path) -> Result<(), Error> {
        let full_path = self.root_path.join(path);
        
        if full_path.exists() && full_path.is_dir() {
            // Only remove if directory is empty
            if let Ok(entries) = std::fs::read_dir(&full_path) {
                if entries.count() == 0 {
                    std::fs::remove_dir(&full_path).map_err(|e| {
                        Error::IO(e)
                    })?;
                }
            }
        }
        
        Ok(())
    }
    
    /// Create a directory if it doesn't exist
    pub fn make_directory(&self, path: &Path) -> Result<(), Error> {
        let full_path = self.root_path.join(path);
        
        if full_path.exists() {
            if full_path.is_file() {
                // Remove file to replace with directory
                std::fs::remove_file(&full_path).map_err(|e| {
                    Error::IO(e)
                })?;
            } else {
                // Already a directory, nothing to do
                return Ok(());
            }
        }
        
        // Create the directory
        std::fs::create_dir_all(&full_path).map_err(|e| {
            Error::IO(e)
        })
    }
}