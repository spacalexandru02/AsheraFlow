// src/commands/add.rs - Updated to use refactored Database
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::time::Instant;
use crate::core::database::blob::Blob;
use crate::core::database::database::Database;
use crate::core::index::index::Index;
use crate::core::workspace::Workspace;
use crate::errors::error::Error;
use std::fs;

pub struct AddCommand;

impl AddCommand {
    pub fn execute(paths: &[String]) -> Result<(), Error> {
        let start_time = Instant::now();
        
        if paths.is_empty() {
            return Err(Error::Generic("No paths specified for add command".into()));
        }

        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }
        
        let workspace = Workspace::new(root_path);
        let mut database = Database::new(git_path.join("objects"));
        let mut index = Index::new(git_path.join("index"));
        
        // Prepare a set to deduplicate files (in case of overlapping path arguments)
        let mut files_to_add: HashSet<PathBuf> = HashSet::new();
        let mut had_missing_files = false;
        
        // Verify all paths exist before making any changes
        for path_str in paths {
            let path = PathBuf::from(path_str);
            match workspace.list_files_from(&path) {
                Ok(found_files) => {
                    if found_files.is_empty() {
                        println!("warning: '{path_str}' didn't match any files");
                    } else {
                        // Add files to our set (automatically deduplicates)
                        for file in found_files {
                            files_to_add.insert(file);
                        }
                    }
                },
                Err(Error::InvalidPath(_)) => {
                    println!("fatal: pathspec '{path_str}' did not match any files");
                    had_missing_files = true;
                },
                Err(e) => return Err(e),
            }
        }
        
        // If any paths were invalid, exit without modifying the index
        if had_missing_files {
            return Err(Error::Generic("Adding files failed: some paths don't exist".into()));
        }
        
        // If no files were found, exit early
        if files_to_add.is_empty() {
            println!("No files to add");
            return Ok(());
        }
        
        // Check for existing index.lock file before trying to acquire lock
        let lock_path = git_path.join("index.lock");
        if lock_path.exists() {
            return Err(Error::Lock(format!(
                "Unable to create '.ash/index.lock': File exists.\n\
                Another ash process seems to be running in this repository.\n\
                If it still fails, a process may have crashed in this repository earlier:\n\
                remove the file manually to continue."
            )));
        }
        
        // Try to acquire the lock on the index
        if !index.load_for_update()? {
            return Err(Error::Lock(format!(
                "Unable to acquire lock on index. Another process may be using it. \
                If not, the .ash/index.lock file may need to be manually removed."
            )));
        }
        
        // Get current files in index to avoid unnecessary operations
        let mut existing_oids = std::collections::HashMap::new();
        for entry in index.each_entry() {
            existing_oids.insert(entry.get_path().to_string(), entry.oid.clone());
        }
        
        // Track the number of files we successfully add
        let mut added_count = 0;
        let mut unchanged_count = 0;
        
        // Create a buffer for batch processing
        let mut blobs_to_save: Vec<(PathBuf, Vec<u8>, fs::Metadata)> = Vec::with_capacity(files_to_add.len());
        
        // First pass: read all files and check for errors before we start modifying anything
        for file_path in &files_to_add {
            // Try to read file content and metadata
            match (
                workspace.read_file(file_path),
                workspace.stat_file(file_path)
            ) {
                (Ok(data), Ok(stat)) => {
                    // Check if file is already in index with same content
                    let file_key = file_path.to_string_lossy().to_string();
                    
                    // Pre-compute hash to check if the file has changed
                    // Use the refactored Database's hash_file_data method
                    let new_oid = database.hash_file_data(&data);
                    
                    if let Some(old_oid) = existing_oids.get(&file_key) {
                        if old_oid == &new_oid {
                            // File exists in index with same content, skip it
                            unchanged_count += 1;
                            continue;
                        }
                    }
                    
                    // Queue file for processing
                    blobs_to_save.push((file_path.clone(), data, stat));
                },
                (Err(Error::IO(e)), _) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    // Permission denied error
                    index.rollback()?;
                    return Err(Error::Generic(format!(
                        "error: open('{}'): Permission denied\nfatal: adding files failed",
                        file_path.display()
                    )));
                },
                (Err(e), _) => {
                    // Other read errors
                    index.rollback()?;
                    return Err(Error::Generic(format!(
                        "error: Failed to read '{}': {}\nfatal: adding files failed",
                        file_path.display(), e
                    )));
                },
                (_, Err(e)) => {
                    // Metadata errors
                    index.rollback()?;
                    return Err(Error::Generic(format!(
                        "error: Failed to get stats for '{}': {}\nfatal: adding files failed",
                        file_path.display(), e
                    )));
                }
            }
        }
        
        // Second pass: process all files that need to be updated
        for (file_path, data, stat) in blobs_to_save {
            // Create and store the blob
            let mut blob = Blob::new(data);
            if let Err(e) = database.store(&mut blob) {
                // Release the lock if we fail to store the blob
                index.rollback()?;
                return Err(Error::Generic(format!(
                    "Failed to store blob for '{}': {}", file_path.display(), e
                )));
            }
            
            // Get the OID
            let oid = match blob.get_oid() {
                Some(id) => id,
                None => {
                    // Release the lock if the blob has no OID
                    index.rollback()?;
                    return Err(Error::Generic(
                        "Blob OID not set after storage".into()
                    ));
                }
            };
            
            // Add to index
            if let Err(e) = index.add(&file_path, oid, &stat) {
                index.rollback()?;
                return Err(e);
            }
            
            added_count += 1;
        }
        
        // Write index updates
        if added_count > 0 {
            match index.write_updates()? {
                true => {
                    let elapsed = start_time.elapsed();
                    
                    if unchanged_count > 0 {
                        println!(
                            "Added {} file{} to index, {} file{} unchanged ({:.2}s)",
                            added_count,
                            if added_count == 1 { "" } else { "s" },
                            unchanged_count,
                            if unchanged_count == 1 { "" } else { "s" },
                            elapsed.as_secs_f32()
                        );
                    } else {
                        println!(
                            "Added {} file{} to index ({:.2}s)",
                            added_count,
                            if added_count == 1 { "" } else { "s" },
                            elapsed.as_secs_f32()
                        );
                    }
                    Ok(())
                },
                false => Err(Error::Generic("Failed to update index".into())),
            }
        } else if unchanged_count > 0 {
            // If we didn't add any files, release the lock
            index.rollback()?;
            println!(
                "No files changed, {} file{} already up to date",
                unchanged_count,
                if unchanged_count == 1 { "" } else { "s" }
            );
            Ok(())
        } else {
            // If we didn't add any files, release the lock
            index.rollback()?;
            println!("No changes were made to the index");
            Ok(())
        }
    }
}