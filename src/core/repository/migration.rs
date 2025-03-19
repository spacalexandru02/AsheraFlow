// src/core/repository/migration.rs
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::errors::error::Error;
use crate::core::repository::Repository;
use crate::core::database::entry::DatabaseEntry;
use crate::core::repository::inspector::Inspector;

// Define conflict types for different error scenarios
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum ConflictType {
    StaleFile,
    StaleDirectory,
    UntrackedOverwritten,
    UntrackedRemoved,
}

// Custom error for conflicts
#[derive(Debug)]
pub struct Conflict;

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Conflicts prevented checkout")
    }
}

impl std::error::Error for Conflict {}

pub struct Migration<'a> {
    pub repo: &'a Repository,
    pub diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
    pub changes: HashMap<String, Vec<(PathBuf, Option<DatabaseEntry>)>>,
    pub mkdirs: HashSet<PathBuf>,
    pub rmdirs: HashSet<PathBuf>,
    pub errors: Vec<String>,
    pub conflicts: HashMap<ConflictType, HashSet<String>>,
    pub inspector: Inspector<'a>,
}

impl<'a> Migration<'a> {
    pub fn new(repo: &'a Repository, tree_diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>) -> Self {
        // Initialize the Migration with empty change structures
        let mut changes = HashMap::new();
        changes.insert("create".to_string(), Vec::new());
        changes.insert("update".to_string(), Vec::new());
        changes.insert("delete".to_string(), Vec::new());
        
        // Initialize conflict types
        let mut conflicts = HashMap::new();
        conflicts.insert(ConflictType::StaleFile, HashSet::new());
        conflicts.insert(ConflictType::StaleDirectory, HashSet::new());
        conflicts.insert(ConflictType::UntrackedOverwritten, HashSet::new());
        conflicts.insert(ConflictType::UntrackedRemoved, HashSet::new());
        
        Migration {
            repo,
            diff: tree_diff,
            changes,
            mkdirs: HashSet::new(),
            rmdirs: HashSet::new(),
            errors: Vec::new(),
            conflicts,
            inspector: Inspector::new(repo),
        }
    }
    
    // Main method to apply the migration
    pub fn apply_changes(&mut self) -> Result<(), Error> {
        self.plan_changes()?;
        self.collect_errors()?;
        
        // Only if no conflicts were found do we continue with the application
        self.update_workspace()?;
        self.update_index()?;
        
        Ok(())
    }
    
    // Plan all the changes needed based on the tree diff
    pub fn plan_changes(&mut self) -> Result<(), Error> {
        for (path, (old_item, new_item)) in &self.diff {
            self.check_for_conflict(path, old_item, new_item)?;
            self.record_change(path, old_item, new_item);
        }
        
        Ok(())
    }
    
    // Check if a change would cause a conflict
    fn check_for_conflict(&mut self, path: &Path, old_item: &Option<DatabaseEntry>, new_item: &Option<DatabaseEntry>) -> Result<(), Error> {
        // Get the entry from the index
        let entry = self.repo.index.get_entry(&path.to_string_lossy().to_string());
        
        // Check if index differs from both old and new trees
        if self.index_differs_from_trees(entry.as_ref(), old_item, new_item)? {
            self.conflicts.get_mut(&ConflictType::StaleFile).unwrap().insert(path.to_string_lossy().to_string());
            return Ok(());
        }
        
        // Check if path exists in workspace
        let stat = self.repo.workspace.stat_file(path);
        
        // Get the appropriate error type for this situation
        let conflict_type = self.get_error_type(&stat, entry.as_ref(), new_item);
        
        if stat.is_none() {
            // Check for untracked parent that would be overwritten
            if let Some(parent) = self.untracked_parent(path)? {
                let parent_str = parent.to_string_lossy().to_string();
                if entry.is_some() {
                    self.conflicts.get_mut(&conflict_type).unwrap().insert(path.to_string_lossy().to_string());
                } else {
                    self.conflicts.get_mut(&conflict_type).unwrap().insert(parent_str);
                }
            }
        } else if stat.as_ref().unwrap().is_file() {
            // Check if workspace file has uncommitted changes
            let changed = self.inspector.compare_index_to_workspace(entry.as_ref(), stat.as_ref())?;
            if changed.is_some() {
                self.conflicts.get_mut(&conflict_type).unwrap().insert(path.to_string_lossy().to_string());
            }
        } else if stat.as_ref().unwrap().is_dir() {
            // Check if directory contains untracked files
            let trackable = self.inspector.trackable_file(path, stat.as_ref().unwrap())?;
            if trackable {
                self.conflicts.get_mut(&conflict_type).unwrap().insert(path.to_string_lossy().to_string());
            }
        }
        
        Ok(())
    }
    
    // Check if the index entry differs from both old and new tree entries
    fn index_differs_from_trees(&self, entry: Option<&crate::core::index::entry::Entry>, 
                               old_item: &Option<DatabaseEntry>, 
                               new_item: &Option<DatabaseEntry>) -> Result<bool, Error> {
        let differs_from_old = self.inspector.compare_tree_to_index(old_item.as_ref(), entry).is_some();
        let differs_from_new = self.inspector.compare_tree_to_index(new_item.as_ref(), entry).is_some();
        
        Ok(differs_from_old && differs_from_new)
    }
    
    // Check for untracked parent directories that would be overwritten
    fn untracked_parent(&self, path: &Path) -> Result<Option<PathBuf>, Error> {
        // Start from the parent and go up the directory tree
        let mut current = path.parent().map(|p| p.to_path_buf());
        
        while let Some(parent) = current {
            if parent.as_os_str().is_empty() || parent.to_string_lossy() == "." {
                break;
            }
            
            if let Ok(parent_stat) = self.repo.workspace.stat_file(&parent) {
                if parent_stat.is_file() {
                    // Parent exists and is a file - this would be a conflict
                    if self.inspector.trackable_file(&parent, &parent_stat)? {
                        return Ok(Some(parent));
                    }
                }
            }
            
            // Move up to the next parent
            current = parent.parent().map(|p| p.to_path_buf());
        }
        
        Ok(None)
    }
    
    // Determine the error type based on the state of the path
    fn get_error_type(&self, stat: &Option<std::fs::Metadata>, entry: Option<&crate::core::index::entry::Entry>, item: &Option<DatabaseEntry>) -> ConflictType {
        if entry.is_some() {
            ConflictType::StaleFile
        } else if stat.as_ref().map_or(false, |s| s.is_dir()) {
            ConflictType::StaleDirectory
        } else if item.is_some() {
            ConflictType::UntrackedOverwritten
        } else {
            ConflictType::UntrackedRemoved
        }
    }
    
    // Record a change for the given path
    fn record_change(&mut self, path: &Path, old_item: &Option<DatabaseEntry>, new_item: &Option<DatabaseEntry>) {
        let action = if old_item.is_none() {
            // Add all parent directories to mkdirs
            self.add_parent_dirs_to_mkdirs(path);
            "create"
        } else if new_item.is_none() {
            // Add all parent directories to rmdirs
            self.add_parent_dirs_to_rmdirs(path);
            "delete"
        } else {
            // Update - we still need the directories
            self.add_parent_dirs_to_mkdirs(path);
            "update"
        };
        
        self.changes.get_mut(action).unwrap().push((path.to_path_buf(), new_item.clone()));
    }
    
    // Add all parent directories to mkdirs for creation
    fn add_parent_dirs_to_mkdirs(&mut self, path: &Path) {
        if let Some(parent) = path.parent() {
            self.add_dirs_recursively_to_set(&mut self.mkdirs, parent);
        }
    }
    
    // Add all parent directories to rmdirs for potential deletion
    fn add_parent_dirs_to_rmdirs(&mut self, path: &Path) {
        if let Some(parent) = path.parent() {
            self.add_dirs_recursively_to_set(&mut self.rmdirs, parent);
        }
    }
    
    // Add a directory and all its parents to a set
    fn add_dirs_recursively_to_set(&mut self, set: &mut HashSet<PathBuf>, path: &Path) {
        if path.as_os_str().is_empty() || path.to_string_lossy() == "." {
            return;
        }
        
        set.insert(path.to_path_buf());
        
        if let Some(parent) = path.parent() {
            self.add_dirs_recursively_to_set(set, parent);
        }
    }
    
    // Collect all errors from conflicts
    fn collect_errors(&mut self) -> Result<(), Error> {
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
            let mut lines = Vec::new();
            for path in paths {
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
            return Err(Error::Generic(format!("Checkout failed due to conflicts")));
        }
        
        Ok(())
    }
    
    // Update the workspace with planned changes
    fn update_workspace(&self) -> Result<(), Error> {
        // Handle deletions first
        for (path, _) in &self.changes["delete"] {
            self.repo.workspace.remove_file(path)?;
        }
        
        // Remove any empty directories (in reverse order)
        let mut rmdirs: Vec<_> = self.rmdirs.iter().collect();
        rmdirs.sort();
        rmdirs.reverse();
        for dir in rmdirs {
            self.repo.workspace.remove_directory(dir)?;
        }
        
        // Create necessary directories
        let mut mkdirs: Vec<_> = self.mkdirs.iter().collect();
        mkdirs.sort();
        for dir in mkdirs {
            self.repo.workspace.make_directory(dir)?;
        }
        
        // Handle updates
        for (path, entry) in &self.changes["update"] {
            if let Some(entry) = entry {
                self.write_file_to_workspace(path, entry)?;
            }
        }
        
        // Handle creations
        for (path, entry) in &self.changes["create"] {
            if let Some(entry) = entry {
                self.write_file_to_workspace(path, entry)?;
            }
        }
        
        Ok(())
    }
    
    // Write a file to the workspace
    fn write_file_to_workspace(&self, path: &Path, entry: &DatabaseEntry) -> Result<(), Error> {
        // Get the blob data from the database
        let blob = self.repo.database.load(&entry.oid)?;
        let data = blob.to_bytes();
        
        // Write the file
        self.repo.workspace.write_file(path, &data)?;
        
        Ok(())
    }
    
    // Update the index with planned changes
    fn update_index(&self) -> Result<(), Error> {
        // Handle deletions
        for (path, _) in &self.changes["delete"] {
            self.repo.index.remove(&path.to_string_lossy().to_string())?;
        }
        
        // Handle creations and updates
        for action in &["create", "update"] {
            for (path, entry) in &self.changes[action] {
                if let Some(entry) = entry {
                    let stat = self.repo.workspace.stat_file(path)?;
                    self.repo.index.add(path, &entry.oid, &stat)?;
                }
            }
        }
        
        Ok(())
    }
    
    // Get blob data from the database
    pub fn blob_data(&self, oid: &str) -> Result<Vec<u8>, Error> {
        let blob = self.repo.database.load(oid)?;
        Ok(blob.to_bytes())
    }
}