// src/core/repository/migration.rs
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::core::database::tree::{Tree, TreeEntry};
use crate::core::file_mode::FileMode;
use crate::errors::error::Error;
use crate::core::repository::repository::Repository;
use crate::core::database::entry::DatabaseEntry;
use crate::core::repository::inspector::{Inspector, ChangeType};

// Define conflict types for different error scenarios
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum ConflictType {
    StaleFile,           // Local changes would be overwritten
    StaleDirectory,      // Directory contains modified files
    UntrackedOverwritten, // Untracked file would be overwritten
    UntrackedRemoved,    // Untracked file would be removed
    UncommittedChanges,  // Added a new type for uncommitted changes
}

pub struct Migration<'a> {
    pub repo: &'a mut Repository,
    pub diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
    pub errors: Vec<String>,
    conflicts: HashMap<ConflictType, HashSet<String>>,
    changes_to_make: Vec<Change>,
}

#[derive(Clone)]
enum Change {
    Create { path: PathBuf, entry: DatabaseEntry },
    Update { path: PathBuf, entry: DatabaseEntry },
    Delete { path: PathBuf },
}

impl<'a> Migration<'a> {
    pub fn new(repo: &'a mut Repository, tree_diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>) -> Self {
        // Initialize conflict types
        let mut conflicts = HashMap::new();
        conflicts.insert(ConflictType::StaleFile, HashSet::new());
        conflicts.insert(ConflictType::StaleDirectory, HashSet::new());
        conflicts.insert(ConflictType::UntrackedOverwritten, HashSet::new());
        conflicts.insert(ConflictType::UntrackedRemoved, HashSet::new());
        conflicts.insert(ConflictType::UncommittedChanges, HashSet::new()); // Add the new conflict type
        
        Migration {
            repo,
            diff: tree_diff,
            errors: Vec::new(),
            conflicts,
            changes_to_make: Vec::new(),
        }
    }
    

    pub fn apply_changes(&mut self) -> Result<(), Error> {
        // Analyze changes using Inspector to detect conflicts
        self.analyze_changes()?;
        
        // Check if there are any conflicts that would prevent checkout
        self.check_conflicts()?;
        
        // Apply the planned changes
        self.execute_changes()?;
        
        // Final phase: perform a comprehensive cleanup of empty directories
        self.cleanup_empty_directories()?;
        
        Ok(())
    }
    
    // New method to perform more comprehensive directory cleanup
    fn cleanup_empty_directories(&mut self) -> Result<(), Error> {
        println!("Performing final empty directory cleanup");
        
        // First get all directories that exist in the workspace
        let workspace_dirs = self.find_all_workspace_directories()?;
        
        // Sort directories by depth (deepest first) to ensure proper cleanup
        let mut sorted_dirs: Vec<_> = workspace_dirs.into_iter().collect();
        sorted_dirs.sort_by(|a, b| {
            let a_depth = a.components().count();
            let b_depth = b.components().count();
            b_depth.cmp(&a_depth) // Descending order - deepest first
        });
        
        // Try to remove each directory if it's empty
        for dir in sorted_dirs {
            // Skip the root directory
            if dir.as_os_str().is_empty() || dir.to_string_lossy() == "." {
                continue;
            }
            
            let full_path = self.repo.workspace.root_path.join(&dir);
            
            // Skip if directory doesn't exist
            if !full_path.exists() || !full_path.is_dir() {
                continue;
            }
            
            // Check if directory is empty or contains only hidden files
            let is_effectively_empty = if let Ok(entries) = std::fs::read_dir(&full_path) {
                !entries
                    .filter_map(Result::ok)
                    .any(|e| {
                        let name = e.file_name();
                        let name_str = name.to_string_lossy();
                        !name_str.starts_with('.')
                    })
            } else {
                false
            };
            
            if is_effectively_empty {
                println!("Removing empty directory in final cleanup: {}", dir.display());
                
                // First try normal directory removal
                match std::fs::remove_dir(&full_path) {
                    Ok(_) => {
                        println!("Successfully removed empty directory: {}", dir.display());
                    },
                    Err(e) => {
                        // If that fails, try force removal for directories that might have hidden files
                        println!("Standard removal failed, trying force removal: {} - {}", dir.display(), e);
                        
                        // First remove any hidden files
                        if let Ok(entries) = std::fs::read_dir(&full_path) {
                            for entry in entries.filter_map(Result::ok) {
                                let entry_path = entry.path();
                                let name = entry.file_name();
                                let name_str = name.to_string_lossy();
                                
                                if name_str.starts_with('.') && entry_path.is_file() {
                                    if let Err(e) = std::fs::remove_file(&entry_path) {
                                        println!("Warning: Failed to remove hidden file: {} - {}", entry_path.display(), e);
                                    }
                                }
                            }
                        }
                        
                        // Try removal again
                        if let Err(e) = std::fs::remove_dir(&full_path) {
                            println!("Warning: Still could not remove directory: {} - {}", dir.display(), e);
                        } else {
                            println!("Successfully removed directory after clearing hidden files: {}", dir.display());
                        }
                    }
                }
            }
        }
        
        Ok(())
    }

    fn find_all_workspace_directories(&self) -> Result<HashSet<PathBuf>, Error> {
        let mut dirs = HashSet::new();
        let root_path = &self.repo.workspace.root_path;
        
        // Skip .ash directory
        let git_dir = root_path.join(".ash");
        
        self.collect_directories_recursive(root_path, &PathBuf::new(), &mut dirs, &git_dir)?;
        
        Ok(dirs)
    }
    
    // Helper to recursively collect directories
    fn collect_directories_recursive(
        &self, 
        full_path: &Path, 
        rel_path: &Path, 
        dirs: &mut HashSet<PathBuf>,
        git_dir: &Path
    ) -> Result<(), Error> {
        // Skip if this is the .ash directory
        if full_path == git_dir {
            return Ok(());
        }
        
        // Skip if path doesn't exist or isn't a directory
        if !full_path.exists() || !full_path.is_dir() {
            return Ok(());
        }
        
        // Add this directory
        dirs.insert(rel_path.to_path_buf());
        
        // Process subdirectories
        if let Ok(entries) = std::fs::read_dir(full_path) {
            for entry_result in entries {
                if let Ok(entry) = entry_result {
                    let entry_path = entry.path();
                    let entry_name = entry.file_name();
                    
                    // Skip hidden directories
                    if entry_name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                    
                    // Only process directories
                    if entry_path.is_dir() {
                        // Get relative path
                        let entry_rel_path = if rel_path.as_os_str().is_empty() {
                            PathBuf::from(entry_name)
                        } else {
                            rel_path.join(entry_name)
                        };
                        
                        // Recursively collect this directory
                        self.collect_directories_recursive(&entry_path, &entry_rel_path, dirs, git_dir)?;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    fn analyze_changes(&mut self) -> Result<(), Error> {
        println!("Analyzing changes for migration");
        
        // Create Inspector to help analyze the repository state
        let inspector = Inspector::new(
            &self.repo.workspace,
            &self.repo.index,
            &self.repo.database
        );
        
        // First, check if there are uncommitted changes in the workspace
        // This is the key improvement - using the analyze_workspace_changes method
        let workspace_changes = inspector.analyze_workspace_changes()?;
        
        // If there are any uncommitted changes, record them as conflicts
        if !workspace_changes.is_empty() {
            println!("Found uncommitted changes in workspace:");
            for (path, change_type) in &workspace_changes {
                match change_type {
                    ChangeType::Modified | ChangeType::Added | ChangeType::Deleted => {
                        println!("  {} - {:?}", path, change_type);
                        self.conflicts.get_mut(&ConflictType::UncommittedChanges).unwrap().insert(path.clone());
                    },
                    _ => {} // Ignore untracked files here
                }
            }
            
            // If we found uncommitted changes, we can exit early
            if !self.conflicts.get(&ConflictType::UncommittedChanges).unwrap().is_empty() {
                return Ok(());
            }
        }
        
        // Next, find all files in current state that should be deleted
        let mut current_paths = HashSet::new();
        let mut target_paths = HashSet::new();
        
        // Clone diff to avoid borrowing issues
        let diff_clone = self.diff.clone();
        
        // Populate current and target path sets
        for (path, (old_entry, new_entry)) in &diff_clone {
            if old_entry.is_some() {
                current_paths.insert(path.clone());
            }
            if new_entry.is_some() {
                target_paths.insert(path.clone());
            }
        }
        
        // Find files that should be deleted (in current but not in target)
        let deleted_files: Vec<_> = current_paths.difference(&target_paths).cloned().collect();
        
        // Add deletions to our change list
        for path in deleted_files {
            println!("Planning deletion for file: {}", path.display());
            self.changes_to_make.push(Change::Delete { path });
        }
        
        // Now process all other changes
        for (path, (old_entry, new_entry)) in diff_clone {
            // Skip files that we're already planning to delete
            if new_entry.is_none() && old_entry.is_some() {
                // This is a deletion, already handled above
                continue;
            }
            
            // Skip directories from conflict check
            let is_directory = new_entry.as_ref().map_or(false, |e| {
                e.get_mode() == "040000" || FileMode::parse(e.get_mode()).is_directory()
            });
            
            if !is_directory {
                // Check for conflicts using Inspector
                let path_str = path.to_string_lossy().to_string();
                let entry = self.repo.index.get_entry(&path_str);
                
                // Check if index differs from both old and new versions
                if let Some(index_entry) = entry {
                    // Using Inspector to check tree-to-index relationships
                    let changed_from_old = inspector.compare_tree_to_index(old_entry.as_ref(), Some(index_entry));
                    let changed_from_new = inspector.compare_tree_to_index(new_entry.as_ref(), Some(index_entry));
                    
                    if changed_from_old.is_some() && changed_from_new.is_some() {
                        // Index has changes compared to both old and new - conflict
                        println!("Index entry for {} differs from both old and new trees", path_str);
                        self.conflicts.get_mut(&ConflictType::StaleFile).unwrap().insert(path_str.clone());
                        continue;
                    }
                    
                    // Use compare_workspace_vs_blob to check if workspace content matches the indexed content
                    if let Ok(has_changes) = inspector.compare_workspace_vs_blob(&path, index_entry.get_oid()) {
                        if has_changes {
                            println!("Uncommitted changes in workspace file: {}", path_str);
                            self.conflicts.get_mut(&ConflictType::StaleFile).unwrap().insert(path_str.clone());
                            continue;
                        }
                    }
                } else if self.repo.workspace.path_exists(&path)? {
                    // Untracked file in workspace - check for conflict
                    let stat = self.repo.workspace.stat_file(&path)?;
                    
                    if stat.is_file() {
                        if new_entry.is_some() {
                            // Would overwrite untracked file
                            println!("Untracked file would be overwritten: {}", path_str);
                            self.conflicts.get_mut(&ConflictType::UntrackedOverwritten).unwrap().insert(path_str.clone());
                            continue;
                        }
                    } else if stat.is_dir() {
                        // Check for untracked files in directory using Inspector
                        if inspector.trackable_file(&path, &stat)? {
                            println!("Directory contains untracked files: {}", path_str);
                            self.conflicts.get_mut(&ConflictType::StaleDirectory).unwrap().insert(path_str.clone());
                            continue;
                        }
                    }
                }
            }
            
            // No conflicts, plan the change
            if let Some(entry) = new_entry {
                if old_entry.is_some() {
                    // Update existing file
                    self.changes_to_make.push(Change::Update {
                        path: path.clone(),
                        entry,
                    });
                } else {
                    // Create new file
                    self.changes_to_make.push(Change::Create {
                        path: path.clone(),
                        entry,
                    });
                }
            }
        }
        
        Ok(())
    }
    
    // Check for conflicts and return appropriate error if any found
    fn check_conflicts(&mut self) -> Result<(), Error> {
        // Error messages for each conflict type
        let messages = HashMap::from([
            (ConflictType::StaleFile, (
                "Your local changes to the following files would be overwritten by checkout:",
                "Please commit your changes or stash them before you switch branches."
            )),
            (ConflictType::StaleDirectory, (
                "Updating the following directories would lose untracked files in them:",
                "\n"
            )),
            (ConflictType::UntrackedOverwritten, (
                "The following untracked working tree files would be overwritten by checkout:",
                "Please move or remove them before you switch branches."
            )),
            (ConflictType::UntrackedRemoved, (
                "The following untracked working tree files would be removed by checkout:",
                "Please move or remove them before you switch branches."
            )),
            (ConflictType::UncommittedChanges, (
                "You have uncommitted changes in your working tree:",
                "Please commit your changes or stash them before you switch branches."
            ))
        ]);
        
        // Check each conflict type
        for (conflict_type, paths) in &self.conflicts {
            if paths.is_empty() {
                continue;
            }
            
            // Get header and footer for this conflict type
            let (header, footer) = messages.get(conflict_type).unwrap();
            
            // Format the paths
            let mut sorted_paths: Vec<_> = paths.iter().collect();
            sorted_paths.sort();
            
            let mut lines = Vec::new();
            for path in sorted_paths {
                lines.push(format!("\t{}", path));
            }
            
            // Build the error message
            let mut error_message = String::new();
            error_message.push_str(header);
            error_message.push('\n');
            for line in lines {
                error_message.push_str(&line);
                error_message.push('\n');
            }
            error_message.push_str(footer);
            
            self.errors.push(error_message);
        }
        
        // If we have errors, we cannot proceed
        if !self.errors.is_empty() {
            return Err(Error::Generic("Checkout failed due to conflicts".to_string()));
        }
        
        Ok(())
    }
    
    // Execute all planned changes
    fn execute_changes(&mut self) -> Result<(), Error> {
        println!("Executing {} changes", self.changes_to_make.len());

        // Clone the changes to avoid borrowing issues
        let changes_clone = self.changes_to_make.clone();

        // Keep track of directories that might need cleanup
        let mut affected_dirs = HashSet::new();

        // First, handle deletions
        for change in &changes_clone {
            if let Change::Delete { path } = change {
                println!("Processing deletion for: {}", path.display());
                // Check the type in the workspace before removing
                let full_path = self.repo.workspace.root_path.join(path);
                
                // Remove from index first
                let path_str = path.to_string_lossy().to_string();
                self.repo.index.remove(&path_str)?;
                
                if full_path.is_dir() {
                    println!("  -> Removing directory using force_remove_directory");
                    self.repo.workspace.force_remove_directory(path)?;
                } else {
                    // If it's a file or doesn't exist, remove_file handles it
                    println!("  -> Removing file using remove_file");
                    self.repo.workspace.remove_file(path)?;
                }

                // Add parent directories to the affected dirs list
                if let Some(parent) = path.parent() {
                    if !(parent.as_os_str().is_empty() || parent.to_string_lossy() == ".") {
                        affected_dirs.insert(parent.to_path_buf());
                    }
                }
            }
        }

        // Find all directories needed for new/updated files
        let mut needed_dirs = HashSet::new();
        for change in &changes_clone {
            match change {
                Change::Create { path, .. } | Change::Update { path, .. } => {
                    // Add all parent directories
                    let mut current = path.parent();
                    while let Some(parent) = current {
                        if parent.as_os_str().is_empty() || parent.to_string_lossy() == "." {
                            break;
                        }
                        needed_dirs.insert(parent.to_path_buf());
                        current = parent.parent();
                    }
                },
                _ => {}
            }
        }

        // Sort the directories by path length to ensure we create them in order
        let mut dir_list: Vec<_> = needed_dirs.iter().cloned().collect();
        dir_list.sort_by_key(|p| p.to_string_lossy().len());

        // Create all needed directories
        for dir in dir_list {
            println!("Ensuring directory exists: {}", dir.display());
            self.repo.workspace.make_directory(&dir)?;
        }

        // Now apply file creations and updates
        for change in changes_clone {
            match change {
                Change::Create { path, entry } | Change::Update { path, entry } => {
                    // Check if this is a directory entry
                    if entry.get_mode() == "040000" || FileMode::parse(entry.get_mode()).is_directory() {
                        // Ensure directory exists (already done mostly, but good to be sure)
                        println!("Ensuring directory exists (via Create/Update): {}", path.display());
                        self.repo.workspace.make_directory(&path)?;
                    } else {
                        // Write the file and update index
                        println!("Writing file: {}", path.display());
                        self.write_file(&path, &entry)?;
                    }
                },
                _ => {} // Deletions already handled
            }
        }

        // Clean up affected directories - we'll use the improved recursive method
        // which will automatically clean up parent directories as well
        for dir in affected_dirs {
            // Skip if this directory is needed for new/updated files
            if needed_dirs.contains(&dir) {
                continue;
            }

            println!("Checking if previously affected directory is now empty: {}", dir.display());
            self.repo.workspace.remove_directory(&dir)?;
        }

        Ok(())
    }

    // Write a file to the workspace and update the index
    fn write_file(&mut self, path: &Path, entry: &DatabaseEntry) -> Result<(), Error> {
        // Get blob contents
        let blob_obj = self.repo.database.load(&entry.get_oid())?;
        let blob_data = blob_obj.to_bytes();
        
        // Write to workspace
        self.repo.workspace.write_file(path, &blob_data)?;
        
        // Update index
        if let Ok(stat) = self.repo.workspace.stat_file(path) {
            self.repo.index.add(path, &entry.get_oid(), &stat)?;
        }
        
        Ok(())
    }
}