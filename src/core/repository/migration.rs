// src/core/repository/migration.rs
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::core::database::tree::{Tree, TreeEntry};
use crate::core::file_mode::FileMode;
use crate::errors::error::Error;
use crate::core::repository::repository::Repository;
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

// Modified to own the components it needs from repo instead of borrowing them
pub struct Migration<'a> {
    pub repo: &'a mut Repository,
    pub diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
    pub changes: HashMap<&'static str, Vec<(PathBuf, Option<DatabaseEntry>)>>,
    pub mkdirs: HashSet<PathBuf>,
    pub rmdirs: HashSet<PathBuf>,
    pub errors: Vec<String>,
    pub conflicts: HashMap<ConflictType, HashSet<String>>,
    // Remove the Inspector field since it's causing borrowing conflicts
}

impl<'a> Migration<'a> {
    pub fn new(repo: &'a mut Repository, tree_diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>) -> Self {
        // Initialize the Migration with empty change structures
        let mut changes = HashMap::new();
        changes.insert("create", Vec::new());
        changes.insert("update", Vec::new());
        changes.insert("delete", Vec::new());
        
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
        }
    }

    // Main method to apply the migration
    pub fn apply_changes(&mut self) -> Result<(), Error> {
        // Clone the diff to avoid borrowing issues
        let diff_clone = self.diff.clone();
        self.plan_changes_improved(diff_clone)?;
        
        self.collect_errors()?;
        
        // Only if no conflicts were found do we continue with the application
        self.update_workspace()?;
        self.update_index()?;
        
        Ok(())
    }
    
    // Improved version that avoids borrowing issues
    pub fn plan_changes_improved(&mut self, diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>) -> Result<(), Error> {
        for (path, (old_item, new_item)) in diff {
            // Check for conflicts first
            self.check_for_conflict(&path, &old_item, &new_item)?;
            
            // Then record the change
            self.record_change(&path, &old_item, &new_item);
        }
        
        Ok(())
    }
    
    // Check if a change would cause a conflict
    fn check_for_conflict(&mut self, path: &Path, old_item: &Option<DatabaseEntry>, new_item: &Option<DatabaseEntry>) -> Result<(), Error> {
        // Get the entry from the index
        let entry = self.repo.index.get_entry(&path.to_string_lossy().to_string());
        
        // Skip conflict check for directories and handle them specially
        if new_item.as_ref().map_or(false, |e| e.get_mode() == "040000" || FileMode::parse(e.get_mode()).is_directory()) {
            // For directories, recursively check each file inside
            if let Some(item) = new_item {
                return self.check_directory_conflicts(path, &item.get_oid());
            }
            return Ok(());
        }
        
        // Check if index differs from both old and new trees - you need to use the method signature you actually have
        // This line needs to match your existing method
        if self.compare_trees_to_index(entry, old_item, new_item)? {
            self.conflicts.get_mut(&ConflictType::StaleFile).unwrap().insert(path.to_string_lossy().to_string());
            return Ok(());
        }
        
        // Check if path exists in workspace
        let stat_result = self.repo.workspace.stat_file(path);
        let stat = match &stat_result {
            Ok(s) => Some(s),
            Err(_) => None,
        };
        
        // Get the appropriate error type for this situation
        let conflict_type = self.get_error_type(&stat, entry, new_item);
        
        if stat.is_none() {
            // Check for untracked parent that would be overwritten
            // Update this to match your existing method signature
            if let Some(parent) = self.check_untracked_parent(path)? {
                let parent_str = parent.to_string_lossy().to_string();
                if entry.is_some() {
                    self.conflicts.get_mut(&conflict_type).unwrap().insert(path.to_string_lossy().to_string());
                } else {
                    self.conflicts.get_mut(&conflict_type).unwrap().insert(parent_str);
                }
            }
        } else if stat.unwrap().is_file() {
            // Check if workspace file has uncommitted changes
            // Update this to use your existing method or implement directly
            let changed = self.compare_workspace_to_index(entry, stat)?;
            if changed {
                self.conflicts.get_mut(&conflict_type).unwrap().insert(path.to_string_lossy().to_string());
            }
        } else if stat.unwrap().is_dir() {
            // Check if directory contains untracked files
            // Update this to use your existing method or implement directly
            let trackable = self.check_directory_trackable(path, stat.unwrap())?;
            if trackable {
                self.conflicts.get_mut(&conflict_type).unwrap().insert(path.to_string_lossy().to_string());
            }
        }
        
        Ok(())
    }

    fn check_directory_conflicts(&mut self, dir_path: &Path, dir_oid: &str) -> Result<(), Error> {
        println!("DEBUG: Checking conflicts in directory: {} (OID: {})", dir_path.display(), dir_oid);
        
        // Load the tree object for this directory
        let obj = match self.repo.database.load(dir_oid) {
            Ok(o) => o,
            Err(e) => {
                println!("DEBUG: Error loading directory object: {}", e);
                return Ok(());
            }
        };
        
        // Verify it's a tree
        if obj.get_type() != "tree" {
            println!("DEBUG: Object is not a tree: {}", obj.get_type());
            return Ok(());
        }
        
        // Check each entry in the tree
        if let Some(tree) = obj.as_any().downcast_ref::<Tree>() {
            for (name, entry) in tree.get_entries() {
                let entry_path = dir_path.join(name);
                
                match entry {
                    TreeEntry::Blob(oid, mode) => {
                        // Create a DatabaseEntry for this file
                        let db_entry = DatabaseEntry::new(
                            entry_path.to_string_lossy().to_string(),
                            oid.clone(),
                            &mode.to_octal_string()
                        );
                        
                        // Check for conflicts with this specific file
                        println!("DEBUG: Checking file conflict: {}", entry_path.display());
                        self.check_for_conflict(&entry_path, &None, &Some(db_entry))?;
                    },
                    TreeEntry::Tree(subtree) => {
                        if let Some(subtree_oid) = subtree.get_oid() {
                            // Recursively check this subdirectory
                            println!("DEBUG: Checking subdirectory: {}", entry_path.display());
                            self.check_directory_conflicts(&entry_path, subtree_oid)?;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }

    // Check if the index entry differs from both old and new tree entries
    fn index_differs_from_trees(
        &self,
        inspector: &Inspector,
        entry: Option<&crate::core::index::entry::Entry>, 
        old_item: &Option<DatabaseEntry>, 
        new_item: &Option<DatabaseEntry>
    ) -> Result<bool, Error> {
        let differs_from_old = inspector.compare_tree_to_index(old_item.as_ref(), entry).is_some();
        let differs_from_new = inspector.compare_tree_to_index(new_item.as_ref(), entry).is_some();
        
        Ok(differs_from_old && differs_from_new)
    }
    
    // Check for untracked parent directories that would be overwritten
    fn untracked_parent(&self, inspector: &Inspector, path: &Path) -> Result<Option<PathBuf>, Error> {
        // Start from the parent and go up the directory tree
        let mut current = path.parent().map(|p| p.to_path_buf());
        
        while let Some(parent) = current {
            if parent.as_os_str().is_empty() || parent.to_string_lossy() == "." {
                break;
            }
            
            if let Ok(parent_stat) = self.repo.workspace.stat_file(&parent) {
                if parent_stat.is_file() {
                    // Parent exists and is a file - this would be a conflict
                    if inspector.trackable_file(&parent, &parent_stat)? {
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
    fn get_error_type(&self, 
                      stat: &Option<&std::fs::Metadata>, 
                      entry: Option<&crate::core::index::entry::Entry>, 
                      item: &Option<DatabaseEntry>) -> ConflictType {
        if entry.is_some() {
            ConflictType::StaleFile
        } else if stat.map_or(false, |s| s.is_dir()) {
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
            // Directly add directories without recursion to avoid borrowing issues
            let mut current = Some(parent);
            while let Some(p) = current {
                if p.as_os_str().is_empty() || p.to_string_lossy() == "." {
                    break;
                }
                self.mkdirs.insert(p.to_path_buf());
                current = p.parent();
            }
        }
    }
    
    // Add all parent directories to rmdirs for potential deletion
    fn add_parent_dirs_to_rmdirs(&mut self, path: &Path) {
        if let Some(parent) = path.parent() {
            // Directly add directories without recursion to avoid borrowing issues
            let mut current = Some(parent);
            while let Some(p) = current {
                if p.as_os_str().is_empty() || p.to_string_lossy() == "." {
                    break;
                }
                self.rmdirs.insert(p.to_path_buf());
                current = p.parent();
            }
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
    fn update_workspace(&mut self) -> Result<(), Error> {
        // Handle deletions first
        for (path, _) in &self.changes["delete"] {
            println!("DEBUG: Removing file: {}", path.display());
            self.repo.workspace.remove_file(path)?;
        }
        
        // Remove any empty directories (in reverse order)
        let mut rmdirs: Vec<_> = self.rmdirs.iter().collect();
        rmdirs.sort();
        rmdirs.reverse();
        for dir in rmdirs {
            println!("DEBUG: Removing directory: {}", dir.display());
            self.repo.workspace.remove_directory(dir)?;
        }
        
        // Create necessary directories
        let mut mkdirs: Vec<_> = self.mkdirs.iter().collect();
        mkdirs.sort();
        for dir in mkdirs {
            println!("DEBUG: Creating directory: {}", dir.display());
            self.repo.workspace.make_directory(dir)?;
        }
        
        // Handle updates and creations with proper directory handling
        for action in &["update", "create"] {
            let entries_to_process: Vec<_> = self.changes[*action].iter()
                .filter_map(|(path, entry)| {
                    if let Some(entry) = entry {
                        Some((path.clone(), entry.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            
            for (path, entry) in entries_to_process {
                // Check if this is a directory entry
                if entry.get_mode() == "040000" || FileMode::parse(entry.get_mode()).is_directory() {
                    println!("DEBUG: Creating directory from {}: {}", action, path.display());
                    self.repo.workspace.make_directory(&path)?;
                    
                    // Recursively process directory contents
                    println!("DEBUG: Processing directory contents: {}", path.display());
                    self.process_directory_contents(&path, &entry.get_oid())?;
                    continue;
                }
                
                // For files, ensure parent directory exists
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() && parent.to_string_lossy() != "." {
                        println!("DEBUG: Ensuring parent directory: {}", parent.display());
                        self.repo.workspace.make_directory(parent)?;
                    }
                }
                
                // Write the file
                println!("DEBUG: Writing file from {}: {}", action, path.display());
                self.write_file_to_workspace(&path, &entry)?;
            }
        }
        
        Ok(())
    }

    // Write a file to the workspace
    fn write_file_to_workspace(&mut self, path: &Path, entry: &DatabaseEntry) -> Result<(), Error> {
        println!("DEBUG: Writing file: {} (mode: {}, OID: {})", 
                 path.display(), entry.get_mode(), entry.get_oid());
        
        // Get the blob data from the database
        let blob_obj = match self.repo.database.load(&entry.get_oid()) {
            Ok(obj) => obj,
            Err(e) => {
                println!("ERROR: Failed to load blob {}: {}", entry.get_oid(), e);
                return Err(e);
            }
        };
        
        // Convert to blob
        let blob_data = blob_obj.to_bytes();
        
        // Write the file
        match self.repo.workspace.write_file(path, &blob_data) {
            Ok(_) => {
                println!("DEBUG: Successfully wrote file: {}", path.display());
                Ok(())
            },
            Err(e) => {
                println!("ERROR: Failed to write file {}: {}", path.display(), e);
                Err(e)
            }
        }
    }
    
    // Update the index with planned changes
    fn update_index(&mut self) -> Result<(), Error> {
        // Handle deletions
        for (path, _) in &self.changes["delete"] {
            self.repo.index.remove(&path.to_string_lossy().to_string())?;
        }
        
        // Handle creations and updates
        for action in &["create", "update"] {
            let entries_to_process: Vec<_> = self.changes[*action].iter()
                .filter_map(|(path, entry)| {
                    if let Some(entry) = entry {
                        Some((path.clone(), entry.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            
            for (path, entry) in entries_to_process {
                let stat = self.repo.workspace.stat_file(&path)?;
                self.repo.index.add(&path, &entry.oid, &stat)?;
            }
        }
        
        Ok(())
    }
    
    // Get blob data from the database
    pub fn blob_data(&mut self, oid: &str) -> Result<Vec<u8>, Error> {
        let blob = self.repo.database.load(oid)?;
        Ok(blob.to_bytes())
    }

    // In Migration implementation
    // Add this function to your Migration implementation
fn process_directory_contents(&mut self, directory_path: &Path, directory_oid: &str) -> Result<(), Error> {
    println!("DEBUG: Processing contents of directory: {} (OID: {})", directory_path.display(), directory_oid);
    
    // Load the tree object
    let obj = self.repo.database.load(directory_oid)?;
    
    // Make sure it's a tree
    if let Some(tree) = obj.as_any().downcast_ref::<Tree>() {
        // Process each entry
        for (name, entry) in tree.get_entries() {
            let entry_path = directory_path.join(name);
            
            match entry {
                TreeEntry::Blob(oid, mode) => {
                    // It's a file, write it
                    println!("DEBUG: Writing file: {}", entry_path.display());
                    
                    // Get blob content
                    let blob_obj = self.repo.database.load(oid)?;
                    let data = blob_obj.to_bytes();
                    
                    // Write file
                    self.repo.workspace.write_file(&entry_path, &data)?;
                },
                TreeEntry::Tree(subtree) => {
                    if let Some(subtree_oid) = subtree.get_oid() {
                        // Create directory
                        println!("DEBUG: Creating directory: {}", entry_path.display());
                        self.repo.workspace.make_directory(&entry_path)?;
                        
                        // Process contents recursively
                        self.process_directory_contents(&entry_path, subtree_oid)?;
                    }
                }
            }
        }
    }
    
    Ok(())
}
    fn compare_trees_to_index(&self, entry: Option<&crate::core::index::entry::Entry>, 
        old_item: &Option<DatabaseEntry>, 
        new_item: &Option<DatabaseEntry>) -> Result<bool, Error> {
    // Implement the comparison logic here based on your existing code
    // This should return true if the index entry differs from both old and new trees
    let differs_from_old = self.compare_tree_to_index(old_item.as_ref(), entry);
    let differs_from_new = self.compare_tree_to_index(new_item.as_ref(), entry);

    Ok(differs_from_old && differs_from_new)
    }

    fn compare_tree_to_index(&self, item: Option<&DatabaseEntry>, entry: Option<&crate::core::index::entry::Entry>) -> bool {
    // Implement based on your existing code
    // This should return true if the item differs from the entry
    match (item, entry) {
    (Some(item), Some(entry)) => {
    let mode_match = item.get_mode() == entry.mode_octal();
    let oid_match = item.get_oid() == entry.oid;
    !mode_match || !oid_match
    },
    (Some(_), None) | (None, Some(_)) => true,
    (None, None) => false,
    }
    }

    fn check_untracked_parent(&self, path: &Path) -> Result<Option<PathBuf>, Error> {
    // Implement based on your existing untracked_parent method
    // Start from the parent and go up the directory tree
    let mut current = path.parent().map(|p| p.to_path_buf());

    while let Some(parent) = current {
    if parent.as_os_str().is_empty() || parent.to_string_lossy() == "." {
    break;
    }

    if let Ok(parent_stat) = self.repo.workspace.stat_file(&parent) {
    if parent_stat.is_file() {
    // Parent exists and is a file - this would be a conflict
    if self.check_directory_trackable(&parent, &parent_stat)? {
    return Ok(Some(parent));
    }
    }
    }

    // Move up to the next parent
    current = parent.parent().map(|p| p.to_path_buf());
    }

    Ok(None)
    }

    fn compare_workspace_to_index(&self, entry: Option<&crate::core::index::entry::Entry>, 
            stat: Option<&std::fs::Metadata>) -> Result<bool, Error> {
    // Implement based on your existing code for comparing workspace to index
    if entry.is_none() {
    return Ok(false); // No change if not in index
    }

    let entry = entry.unwrap();

    if stat.is_none() {
    return Ok(true); // Changed if file doesn't exist in workspace
    }

    let stat = stat.unwrap();

    // Check file metadata (size, mode)
    if entry.size as u64 != stat.len() {
    return Ok(true);
    }

    // Check file mode
    if !entry.mode_match(stat) {
    return Ok(true);
    }

    // If timestamps match, assume content is the same
    if entry.time_match(stat) {
    return Ok(false);
    }

    // Check content by reading and hashing the file
    let path = Path::new(&entry.path);
    let data = self.repo.workspace.read_file(path)?;
    let oid = self.repo.database.hash_file_data(&data);

    Ok(entry.oid != oid)
    }

    fn check_directory_trackable(&self, path: &Path, stat: &std::fs::Metadata) -> Result<bool, Error> {
    // Implement based on your existing trackable_file method
    if !stat.is_dir() {
    return Ok(false);
    }

    // Check for untracked files in the directory
    match std::fs::read_dir(path) {
    Ok(entries) => {
    for entry in entries {
    if let Ok(entry) = entry {
    let entry_path = entry.path();
    let rel_path = entry_path.strip_prefix(self.repo.workspace.root_path.clone())
        .unwrap_or(&entry_path);
    let rel_path_str = rel_path.to_string_lossy().to_string();
    
    if !self.repo.index.tracked(&rel_path_str) {
        return Ok(true);
    }
    
    if entry_path.is_dir() {
        if let Ok(subdir_stat) = entry_path.metadata() {
            if self.check_directory_trackable(&entry_path, &subdir_stat)? {
                return Ok(true);
            }
        }
    }
    }
    }
    Ok(false)
    },
    Err(_) => Ok(false),
    }
    }
}