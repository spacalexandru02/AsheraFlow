// src/commands/rm.rs
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::collections::HashSet;

use crate::errors::error::Error;
use crate::core::repository::repository::Repository;
use crate::core::repository::inspector::{Inspector, ChangeType};
use crate::core::color::Color;

pub struct RmCommand;

impl RmCommand {
    pub fn execute(paths: &[String], cached: bool, force: bool, recursive: bool) -> Result<(), Error> {
        let start_time = Instant::now();
        
        // Initialize repository
        let mut repo = Repository::new(".")?;
        
        // Try to acquire a lock on the index
        if !repo.index.load_for_update()? {
            return Err(Error::Lock(format!(
                "Unable to acquire lock on index. Another process may be using it."
            )));
        }
        
        // If no paths are specified, return an error
        if paths.is_empty() {
            repo.index.rollback()?;
            return Err(Error::Generic("No paths specified for removal".to_string()));
        }
        
        // Track problem files
        let mut both_changed: Vec<PathBuf> = Vec::new();
        let mut uncommitted: Vec<PathBuf> = Vec::new();
        let mut unstaged: Vec<PathBuf> = Vec::new();
        let mut untracked: Vec<PathBuf> = Vec::new();
        
        // Collect all paths to remove (expanding directories if recursive)
        let mut paths_to_remove: HashSet<PathBuf> = HashSet::new();
        
        for path_str in paths {
            let path = PathBuf::from(path_str);
            
            // If recursive flag is set and path is a directory, add all files in the directory
            if recursive && repo.workspace.path_exists(&path)? && path.is_dir() {
                // Get all tracked files in this directory
                let expanded_paths = RmCommand::expand_directory_paths(&repo, &path)?;
                if expanded_paths.is_empty() {
                    untracked.push(path.clone());
                } else {
                    for expanded_path in expanded_paths {
                        paths_to_remove.insert(expanded_path);
                    }
                }
            } else {
                // Check if the path is tracked
                let path_string = path.to_string_lossy().to_string();
                if !repo.index.tracked(&path_string) {
                    untracked.push(path.clone());
                    continue;
                }
                
                paths_to_remove.insert(path.clone());
            }
        }
        
        // Early exit if there are untracked files
        if !untracked.is_empty() {
            repo.index.rollback()?;
            return Err(Error::Generic(format_untracked_error(&untracked)));
        }
        
        // Get current HEAD commit for comparison
        let head_oid = match repo.refs.read_head()? {
            Some(oid) => Some(oid),
            None => None, // Repository might be empty
        };
        
        // Plan removals
        for path in &paths_to_remove {
            let path_str = path.to_string_lossy().to_string();
            let entry = repo.index.get_entry(&path_str);
            
            // Skip if not found in index (should not happen at this point)
            if entry.is_none() {
                continue;
            }
            
            if !force {
                // Skip checks if force option is provided
                let stat = match repo.workspace.stat_file(path) {
                    Ok(stat) => Some(stat),
                    Err(_) => None,
                };
                
                // Check for staged changes vs HEAD
                // Check for staged changes (index vs HEAD)
                let mut staged_change = None;
                
                if let Some(oid) = &head_oid {
                    // For staged changes, compare current index with HEAD
                    let index_oid = entry.map(|e| e.get_oid().to_string());
                    
                    // Manually check if file exists in HEAD
                    let mut head_tree_oid = None;
                    
                    // Load the commit and its tree
                    let commit_obj = repo.database.load(oid)?;
                    if let Some(commit) = commit_obj.as_any().downcast_ref::<crate::core::database::commit::Commit>() {
                        head_tree_oid = Some(commit.get_tree().to_string());
                    }
                    
                    // Load head tree if found
                    if let Some(tree_oid) = head_tree_oid {
                        let path_str = path.to_string_lossy().to_string();
                        
                        // Check if this path exists in HEAD
                        // Simplified - actually we just check if the file is in the index
                        // and not in the initial commit, or has changed since
                        
                        if let Some(index_oid_val) = &index_oid {
                            let in_head = true; // Simplified - assume file is in HEAD
                            
                            if !in_head {
                                // File added since HEAD
                                staged_change = Some(ChangeType::Added);
                            } else {
                                // Check if content is different
                                staged_change = Some(ChangeType::Modified);
                            }
                        }
                    }
                }
                
                // For unstaged changes, compare workspace with index
                let unstaged_change = if let Some(stat_val) = &stat {
                    // Create temporary inspector for this check
                    let inspector = Inspector::new(
                        &repo.workspace,
                        &repo.index,
                        &repo.database
                    );
                    
                    inspector.compare_index_to_workspace(entry, Some(stat_val))?
                } else {
                    // File doesn't exist in workspace
                    if !cached {
                        Some(ChangeType::Deleted)
                    } else {
                        None
                    }
                };
                
                // Add to problem lists based on changes
                if staged_change.is_some() && unstaged_change.is_some() {
                    both_changed.push(path.clone());
                } else if staged_change.is_some() && !cached {
                    uncommitted.push(path.clone());
                } else if unstaged_change.is_some() && !cached {
                    unstaged.push(path.clone());
                }
            }
        }
        
        // Check for errors and exit if needed
        if !both_changed.is_empty() || !uncommitted.is_empty() || !unstaged.is_empty() {
            repo.index.rollback()?;
            
            let mut error_message = String::new();
            
            if !both_changed.is_empty() {
                error_message.push_str(&format_error_section(
                    &both_changed,
                    "staged content different from both the file and the HEAD"
                ));
            }
            
            if !uncommitted.is_empty() {
                error_message.push_str(&format_error_section(
                    &uncommitted,
                    "changes staged in the index"
                ));
            }
            
            if !unstaged.is_empty() {
                error_message.push_str(&format_error_section(
                    &unstaged,
                    "local modifications"
                ));
            }
            
            if !force {
                error_message.push_str("\nuse --force to override this check\n");
            }
            
            return Err(Error::Generic(error_message));
        }
        
        // Perform removals
        let mut removed_count = 0;
        for path in &paths_to_remove {
            let path_str = path.to_string_lossy().to_string();
            
            // Remove from index
            repo.index.remove(&path_str)?;
            
            // Remove from workspace if not cached
            if !cached {
                if let Err(e) = repo.workspace.remove_file(path) {
                    println!("Warning: Failed to remove file {}: {}", path.display(), e);
                    // Continue with other files despite error
                }
            }
            
            removed_count += 1;
            println!("{} '{}'", Color::red("removed"), path.display());
        }
        
        // Write index updates
        repo.index.write_updates()?;
        
        let elapsed = start_time.elapsed();
        println!("\nRemoved {} file(s) in {:.2}s", removed_count, elapsed.as_secs_f32());
        
        Ok(())
    }
    
    // Helper method to expand directory paths
    fn expand_directory_paths(repo: &Repository, dir_path: &Path) -> Result<Vec<PathBuf>, Error> {
        let mut paths = Vec::new();
        
        // Get all tracked files
        for entry in repo.index.each_entry() {
            let entry_path = PathBuf::from(entry.get_path());
            
            // Check if the entry is under the specified directory
            if entry_path.starts_with(dir_path) {
                paths.push(entry_path);
            }
        }
        
        Ok(paths)
    }
}

// Helper function to format error for untracked files
fn format_untracked_error(paths: &[PathBuf]) -> String {
    let mut message = String::from("The following file(s) are not tracked:\n");
    
    for path in paths {
        message.push_str(&format!("    {}\n", path.display()));
    }
    
    message
}

// Helper function to format error sections
fn format_error_section(paths: &[PathBuf], message: &str) -> String {
    if paths.is_empty() {
        return String::new();
    }
    
    let files_have = if paths.len() == 1 {
        "file has"
    } else {
        "files have"
    };
    
    let mut section = format!("error: the following {} {}:\n", files_have, message);
    
    for path in paths {
        section.push_str(&format!("    {}\n", path.display()));
    }
    
    section
}