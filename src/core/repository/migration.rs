// src/core/repository/migration.rs
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::core::database::blob::Blob;
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
        
        // First, handle deletions
        for change in &changes_clone {
            if let Change::Delete { path } = change {
                println!("Removing file: {}", path.display());
                self.repo.workspace.remove_file(path)?;
                
                // Also remove from index
                let path_str = path.to_string_lossy().to_string();
                self.repo.index.remove(&path_str)?;
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
        let mut dir_list: Vec<_> = needed_dirs.into_iter().collect();
        dir_list.sort_by_key(|p| p.to_string_lossy().len());
        
        // Create all needed directories
        for dir in dir_list {
            println!("Creating directory: {}", dir.display());
            self.repo.workspace.make_directory(&dir)?;
        }
        
        // Now apply file creations and updates
        for change in changes_clone {
            match change {
                Change::Create { path, entry } | Change::Update { path, entry } => {
                    // Check if this is a directory entry
                    if entry.get_mode() == "040000" || FileMode::parse(entry.get_mode()).is_directory() {
                        println!("Creating directory: {}", path.display());
                        self.repo.workspace.make_directory(&path)?;
                        
                        // Process directory contents
                        self.process_directory_contents(&path, &entry.get_oid())?;
                    } else {
                        // Write the file and update index
                        println!("Writing file: {}", path.display());
                        self.write_file(&path, &entry)?;
                    }
                },
                _ => {}
            }
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
    
    // Process a directory's contents recursively
    fn process_directory_contents(&mut self, directory_path: &Path, directory_oid: &str) -> Result<(), Error> {
        println!("Processing directory contents: {}", directory_path.display());
        
        // Load the tree object
        let obj = self.repo.database.load(directory_oid)?;
        
        // Create inspector to help track files
        let inspector = Inspector::new(
            &self.repo.workspace,
            &self.repo.index,
            &self.repo.database
        );
        
        // Find all current files in this directory from index
        let current_files = self.get_current_files_in_dir(directory_path)?;
        
        // Make sure it's a tree
        if let Some(tree) = obj.as_any().downcast_ref::<Tree>() {
            // Track which files we process in the new tree
            let mut processed_files = HashSet::new();
            
            // Process each entry
            for (name, entry) in tree.get_entries() {
                let entry_path = directory_path.join(name);
                processed_files.insert(entry_path.clone());
                
                match entry {
                    TreeEntry::Blob(oid, mode) => {
                        // Check if this is a directory stored as a blob
                        if *mode == FileMode::DIRECTORY || mode.is_directory() {
                            // It's a directory, handle it recursively
                            println!("Creating directory (from blob): {}", entry_path.display());
                            self.repo.workspace.make_directory(&entry_path)?;
                            self.process_directory_contents(&entry_path, oid)?;
                        } else {
                            // It's a file
                            println!("Writing file: {}", entry_path.display());
                            
                            // Get and write the blob content
                            let blob_obj = self.repo.database.load(oid)?;
                            let blob_data = blob_obj.to_bytes();
                            self.repo.workspace.write_file(&entry_path, &blob_data)?;
                            
                            // Update index
                            if let Ok(stat) = self.repo.workspace.stat_file(&entry_path) {
                                self.repo.index.add(&entry_path, oid, &stat)?;
                            }
                        }
                    },
                    TreeEntry::Tree(subtree) => {
                        if let Some(subtree_oid) = subtree.get_oid() {
                            // Create directory
                            println!("Creating directory: {}", entry_path.display());
                            self.repo.workspace.make_directory(&entry_path)?;
                            
                            // Process contents recursively
                            self.process_directory_contents(&entry_path, subtree_oid)?;
                        }
                    }
                }
            }
            
            // Delete files that exist in current state but not in target state
            for file_path in current_files {
                if !processed_files.contains(&file_path) {
                    println!("Removing file that doesn't exist in target: {}", file_path.display());
                    self.repo.workspace.remove_file(&file_path)?;
                    
                    // Also remove from index
                    let path_str = file_path.to_string_lossy().to_string();
                    self.repo.index.remove(&path_str)?;
                }
            }
        }
        
        Ok(())
    }
    
    // Get all current files in a specific directory
    fn get_current_files_in_dir(&self, dir_path: &Path) -> Result<HashSet<PathBuf>, Error> {
        let mut files = HashSet::new();
        let dir_prefix = dir_path.to_string_lossy().to_string();
        
        // Get files from index that match this directory
        for entry in self.repo.index.each_entry() {
            let path = PathBuf::from(entry.get_path());
            
            if (path.starts_with(dir_path) || 
                entry.get_path().starts_with(&dir_prefix)) &&
               path != *dir_path {
                files.insert(path);
            }
        }
        
        Ok(files)
    }
}