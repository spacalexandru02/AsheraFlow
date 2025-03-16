use std::path::{Path, PathBuf};
use std::fs::{self, File};

use std::io::Read;
use std::collections::{HashMap, HashSet};
use sha1::{Digest, Sha1};
use flate2::read::ZlibDecoder;
use crate::errors::error::Error;
use crate::core::database::{database::Database, blob::Blob};
use crate::core::index::index::Index;
use crate::core::workspace::Workspace;

pub struct RepositoryValidator {
    root_path: PathBuf,
    git_path: PathBuf,
    database: Database,
    issues: Vec<String>,
}

impl RepositoryValidator {
    pub fn new(root_path: &Path) -> Self {
        let git_path = root_path.join(".ash");
        let db_path = git_path.join("objects");
        
        RepositoryValidator {
            root_path: root_path.to_path_buf(),
            git_path,
            database: Database::new(db_path),
            issues: Vec::new(),
        }
    }
    
    pub fn validate_repository(&mut self) -> Result<(), Error> {
        println!("Validating repository structure...");
        self.check_repository_structure()?;
        
        println!("\nValidating index file...");
        self.validate_index()?;
        
        println!("\nValidating objects database...");
        self.validate_objects()?;
        
        println!("\nChecking for unstaged changes...");
        self.check_unstaged_changes()?;
        
        println!("\nValidation complete.");
        if self.issues.is_empty() {
            println!("No issues found. Repository is healthy.");
        } else {
            println!("\nIssues found:");
            for (i, issue) in self.issues.iter().enumerate() {
                println!("  {}. {}", i + 1, issue);
            }
        }
        
        Ok(())
    }
    
    fn check_repository_structure(&mut self) -> Result<(), Error> {
        if !self.git_path.exists() {
            self.issues.push("Repository not initialized: .ash directory missing".to_string());
            return Ok(()); // Continue validation with limited checks
        }
        
        // Check for required directories
        for dir in &["objects", "refs"] {
            let dir_path = self.git_path.join(dir);
            if !dir_path.exists() {
                self.issues.push(format!("Required directory missing: {}", dir_path.display()));
            } else {
                println!("✓ {}: Directory exists", dir);
            }
        }
        
        // Check for lock files that might be left over
        for lock_file in &["index.lock", "HEAD.lock"] {
            let lock_path = self.git_path.join(lock_file);
            if lock_path.exists() {
                self.issues.push(format!(
                    "Stale lock file found: {}. This may indicate a crashed process.", 
                    lock_path.display()
                ));
            }
        }
        
        Ok(())
    }
    
    fn validate_index(&mut self) -> Result<(), Error> {
        let index_path = self.git_path.join("index");
        
        if !index_path.exists() {
            println!("Index file doesn't exist. This is normal for a new repository.");
            return Ok(());
        }
        
        let mut index = Index::new(&index_path);
        
        // Try to load the index
        match index.load() {
            Ok(_) => {
                println!("✓ Index file loaded successfully");
                println!("  Found {} entries in index", index.entries.len());
                
                // Validate each entry in the index
                let mut invalid_entries = Vec::new();
                let mut missing_objects = Vec::new();
                
                for entry in index.each_entry() {
                    // Check if the object exists in the database
                    if !self.database.exists(&entry.oid) {
                        missing_objects.push(format!("{}: {}", entry.path, entry.oid));
                    }
                    
                    // Check if the file exists in the workspace
                    let file_path = self.root_path.join(&entry.path);
                    if !file_path.exists() {
                        invalid_entries.push(entry.path.clone());
                    }
                }
                
                if !missing_objects.is_empty() {
                    self.issues.push(format!(
                        "Found {} entries in index pointing to missing objects", 
                        missing_objects.len()
                    ));
                    for missing in missing_objects.iter().take(5) {
                        self.issues.push(format!("  Missing object: {}", missing));
                    }
                    if missing_objects.len() > 5 {
                        self.issues.push(format!("  ... and {} more", missing_objects.len() - 5));
                    }
                }
                
                if !invalid_entries.is_empty() {
                    self.issues.push(format!(
                        "Found {} entries in index pointing to non-existent files", 
                        invalid_entries.len()
                    ));
                    for invalid in invalid_entries.iter().take(5) {
                        self.issues.push(format!("  Missing file: {}", invalid));
                    }
                    if invalid_entries.len() > 5 {
                        self.issues.push(format!("  ... and {} more", invalid_entries.len() - 5));
                    }
                }
            },
            Err(e) => {
                self.issues.push(format!("Failed to load index file: {}", e));
            }
        }
        
        Ok(())
    }
    
    fn validate_objects(&mut self) -> Result<(), Error> {
        let objects_dir = self.git_path.join("objects");
        
        if !objects_dir.exists() {
            self.issues.push("Objects directory is missing".to_string());
            return Ok(());
        }
        
        let mut total_objects = 0;
        let mut valid_objects = 0;
        let mut invalid_objects = 0;
        let mut object_types = HashMap::new();
        
        // Walk through the objects directory
        for prefix_entry in fs::read_dir(&objects_dir)? {
            let prefix_entry = prefix_entry?;
            let prefix_path = prefix_entry.path();
            
            if prefix_path.is_dir() && prefix_path.file_name().unwrap().len() == 2 {
                for obj_entry in fs::read_dir(&prefix_path)? {
                    let obj_entry = obj_entry?;
                    let obj_path = obj_entry.path();
                    
                    if obj_path.is_file() {
                        total_objects += 1;
                        
                        // Get the object ID
                        let prefix = prefix_path.file_name().unwrap().to_string_lossy();
                        let suffix = obj_path.file_name().unwrap().to_string_lossy();
                        let oid = format!("{}{}", prefix, suffix);
                        
                        // Validate the object
                        match self.validate_object(&obj_path, &oid) {
                            Ok(obj_type) => {
                                valid_objects += 1;
                                let count = object_types.entry(obj_type).or_insert(0);
                                *count += 1;
                            },
                            Err(e) => {
                                invalid_objects += 1;
                                self.issues.push(format!("Invalid object {}: {}", oid, e));
                            }
                        }
                    }
                }
            }
        }
        
        println!("✓ Object database statistics:");
        println!("  Total objects: {}", total_objects);
        println!("  Valid objects: {}", valid_objects);
        if invalid_objects > 0 {
            println!("  Invalid objects: {}", invalid_objects);
        }
        
        println!("  Object types:");
        for (obj_type, count) in object_types {
            println!("    {}: {}", obj_type, count);
        }
        
        Ok(())
    }
    
    fn validate_object(&self, path: &Path, oid: &str) -> Result<String, String> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => return Err(format!("Failed to open file: {}", e)),
        };
        
        let mut decoder = ZlibDecoder::new(file);
        let mut content = Vec::new();
        
        // Read and decompress the object
        if let Err(e) = decoder.read_to_end(&mut content) {
            return Err(format!("Failed to decompress: {}", e));
        }
        
        // Find the null byte separating header from content
        let null_pos = match content.iter().position(|&b| b == 0) {
            Some(pos) => pos,
            None => return Err("No null byte separating header from content".to_string()),
        };
        
        // Parse the header
        let header = match std::str::from_utf8(&content[..null_pos]) {
            Ok(h) => h,
            Err(_) => return Err("Invalid UTF-8 in header".to_string()),
        };
        
        let parts: Vec<&str> = header.split(' ').collect();
        if parts.len() != 2 {
            return Err(format!("Invalid header format: {}", header));
        }
        
        let obj_type = parts[0];
        let size = match parts[1].parse::<usize>() {
            Ok(s) => s,
            Err(_) => return Err(format!("Invalid size: {}", parts[1])),
        };
        
        // Verify the content size
        if size != content.len() - null_pos - 1 {
            return Err(format!(
                "Size mismatch: header claims {} bytes, actual content is {} bytes",
                size, content.len() - null_pos - 1
            ));
        }
        
        // Verify the object ID
        let mut hasher = Sha1::new();
        hasher.update(&content);
        let calculated_oid = format!("{:x}", hasher.finalize());
        
        if calculated_oid != oid {
            return Err(format!(
                "OID mismatch: expected {}, calculated {}",
                oid, calculated_oid
            ));
        }
        
        Ok(obj_type.to_string())
    }
    
    fn check_unstaged_changes(&mut self) -> Result<(), Error> {
        let workspace = Workspace::new(&self.root_path);
        let mut index = Index::new(self.git_path.join("index"));
        
        // Load the index
        if let Err(e) = index.load() {
            return Err(Error::Generic(format!("Failed to load index: {}", e)));
        }
        
        // List all files in the workspace
        let workspace_files = match workspace.list_files() {
            Ok(files) => files,
            Err(e) => return Err(Error::Generic(format!("Failed to list workspace files: {}", e))),
        };
        
        // Create sets for comparison
        let index_files: HashSet<String> = index.each_entry()
            .map(|entry| entry.get_path().to_string())
            .collect();
        
        let workspace_file_set: HashSet<String> = workspace_files.iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        
        // Find untracked files (in workspace but not in index)
        let untracked: Vec<_> = workspace_file_set.difference(&index_files).collect();
        
        // Find modified files (in both but with different content)
        let mut modified = Vec::new();
        
        for entry in index.each_entry() {
            let path = Path::new(entry.get_path());
            
            if workspace_file_set.contains(entry.get_path()) {
                // File exists in both - check if modified
                match workspace.read_file(path) {
                    Ok(data) => {
                        // Create a blob and calculate its OID
                        
                    },
                    Err(_) => {
                        // If we can't read the file, consider it modified
                        modified.push(entry.get_path().to_string());
                    }
                }
            }
        }
        
        // Find deleted files (in index but not in workspace)
        let deleted: Vec<_> = index_files.difference(&workspace_file_set).collect();
        
        // Report the results
        if !untracked.is_empty() {
            println!("Found {} untracked files:", untracked.len());
            for file in untracked.iter().take(5) {
                println!("  {}", file);
            }
            if untracked.len() > 5 {
                println!("  ... and {} more", untracked.len() - 5);
            }
        } else {
            println!("✓ No untracked files");
        }
        
        if !modified.is_empty() {
            println!("Found {} modified files:", modified.len());
            for file in modified.iter().take(5) {
                println!("  {}", file);
            }
            if modified.len() > 5 {
                println!("  ... and {} more", modified.len() - 5);
            }
        } else {
            println!("✓ No modified files");
        }
        
        if !deleted.is_empty() {
            println!("Found {} deleted files:", deleted.len());
            for file in deleted.iter().take(5) {
                println!("  {}", file);
            }
            if deleted.len() > 5 {
                println!("  ... and {} more", deleted.len() - 5);
            }
        } else {
            println!("✓ No deleted files");
        }
        
        Ok(())
    }
}