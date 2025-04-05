// src/core/merge/resolve.rs
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::core::database::blob::Blob;
use crate::core::database::database::{Database, GitObject};
use crate::core::database::entry::DatabaseEntry;
use crate::core::file_mode::FileMode;
use crate::core::index::index::Index;
use crate::core::workspace::Workspace;
use crate::errors::error::Error;
use crate::core::merge::diff3;
use crate::core::merge::inputs::MergeInputs;
use crate::core::path_filter::PathFilter;

pub struct Resolve<'a, T: MergeInputs> {
    database: &'a mut Database,
    workspace: &'a Workspace,
    index: &'a mut Index,
    inputs: &'a T,
    left_diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
    right_diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
    clean_diff: HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
    conflicts: HashMap<String, Vec<Option<DatabaseEntry>>>,
    untracked: HashMap<String, DatabaseEntry>,
    pub on_progress: fn(String),
}

impl<'a, T: MergeInputs> Resolve<'a, T> {
    pub fn new(
        database: &'a mut Database,
        workspace: &'a Workspace,
        index: &'a mut Index,
        inputs: &'a T,
    ) -> Self {
        Self {
            database,
            workspace,
            index,
            inputs,
            left_diff: HashMap::new(),
            right_diff: HashMap::new(),
            clean_diff: HashMap::new(),
            conflicts: HashMap::new(),
            untracked: HashMap::new(),
            on_progress: |_info| (),
        }
    }

    pub fn execute(&mut self) -> Result<(), Error> {
        // Prepare the tree differences
        self.prepare_tree_diffs()?;

        // Apply all clean changes to workspace and index
        self.apply_clean_changes()?;

        // Add conflicts to index
        self.add_conflicts_to_index();

        // Write any untracked files (e.g., from rename conflicts)
        self.write_untracked_files()?;
    }
    
    fn file_dir_conflict(
        &mut self,
        path: &Path,
        diff: &HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
        name: &str,
    ) {
        // Check each parent directory
        for parent in self.parent_directories(path) {
            // Skip if not in the other diff
            if !diff.contains_key(&parent) {
                continue;
            }
            
            let (old_item, new_item) = &diff[&parent];
            if new_item.is_none() {
                continue;
            }
            
            // We have a file/directory conflict
            // One side has a file where the other side has a directory
            let parent_path = parent.to_string_lossy().to_string();
            
            if name == self.inputs.left_name() {
                // Left has a file, right has a directory at this path
                self.conflicts.insert(
                    parent_path.clone(),
                    vec![old_item.clone(), new_item.clone(), None],
                );
            } else {
                // Right has a file, left has a directory at this path
                self.conflicts.insert(
                    parent_path.clone(),
                    vec![old_item.clone(), None, new_item.clone()],
                );
            }
            
            // Remove from clean diff since it's now a conflict
            self.clean_diff.remove(&parent);
            
            // Rename conflicting file to avoid data loss
            let rename = format!("{}~{}", parent_path, name);
            self.untracked.insert(rename.clone(), new_item.clone().unwrap());
            
            // Log the conflict
            if !diff.contains_key(path) {
                self.log(format!("Adding {}", path.to_string_lossy()));
            }
            self.log_conflict(&parent, Some(rename));
        }
    }
    
    fn apply_clean_changes(&mut self) -> Result<(), Error> {
        // Process all clean changes (no conflicts)
        for (path, (_, new_entry)) in &self.clean_diff {
            if let Some(entry) = new_entry {
                // File being added or modified
                let path_str = path.to_string_lossy().to_string();
                
                // Load blob content
                let blob_obj = self.database.load(&entry.get_oid())?;
                let content = blob_obj.to_bytes();
                
                // Write file to workspace
                self.workspace.write_file(path, &content)?;
                
                // Update index
                if let Ok(stat) = self.workspace.stat_file(path) {
                    self.index.add(path, &entry.get_oid(), &stat)?;
                }
            } else {
                // File being deleted
                self.workspace.remove_file(path)?;
                
                // Remove from index
                let path_str = path.to_string_lossy().to_string();
                self.index.remove(&path_str)?;
            }
        }
        
        Ok(())
    }
    
    fn add_conflicts_to_index(&mut self) {
        // Add all conflicts to the index's conflict state
        for (path, entries) in &self.conflicts {
            // Add the conflict markers to the index
            let path_obj = Path::new(path);
            self.index.add_conflict(path_obj, entries.clone());
        }
    }
    
    fn write_untracked_files(&mut self) -> Result<(), Error> {
        // Write renamed files to avoid data loss in conflicts
        for (path, entry) in &self.untracked {
            // Get a temporary database reference to load blob
            let blob_obj = self.database.load(&entry.get_oid())?;
            let content = blob_obj.to_bytes();
            
            // Write to workspace
            self.workspace.write_file(&Path::new(path), &content)?;
        }
        
        Ok(())
    }
    
    fn parent_directories(&self, path: &Path) -> Vec<PathBuf> {
        let mut result = Vec::new();
        let mut current = PathBuf::from(path);
        
        while let Some(parent) = current.parent() {
            if parent.as_os_str().is_empty() {
                break;
            }
            
            result.push(parent.to_path_buf());
            current = parent.to_path_buf();
        }
        
        result
    }
    
    fn log(&self, message: String) {
        (self.on_progress)(message);
    }
    
    fn log_conflict(&self, path: &Path, rename: Option<String>) {
        let path_str = path.to_string_lossy().to_string();
        let conflict = &self.conflicts[&path_str];
        let (base, left, right) = (&conflict[0], &conflict[1], &conflict[2]);
        
        if left.is_some() && right.is_some() {
            self.log_left_right_conflict(&path_str);
        } else if base.is_some() && (left.is_some() || right.is_some()) {
            self.log_modify_delete_conflict(&path_str, rename);
        } else if let Some(rename) = rename {
            self.log_file_directory_conflict(&path_str, rename);
        }
    }
    
    fn log_left_right_conflict(&self, path: &str) {
        let conflict_type = if self.conflicts[path][0].is_some() {
            "content"
        } else {
            "add/add"
        };
        
        self.log(format!("CONFLICT ({}): Merge conflict in {}", conflict_type, path));
    }
    
    fn log_modify_delete_conflict(&self, path: &str, rename: Option<String>) {
        let (deleted, modified) = self.log_branch_names(path);
        
        let rename_msg = if let Some(rename) = rename {
            format!(" at {}", rename)
        } else {
            String::new()
        };
        
        self.log(format!(
            "CONFLICT (modify/delete): {} deleted in {} and modified in {}. Version {} of {} left in tree{}.",
            path, deleted, modified, modified, path, rename_msg,
        ));
    }
    
    fn log_file_directory_conflict(&self, path: &str, rename: String) {
        let conflict_type = if self.conflicts[path][1].is_some() {
            "file/directory"
        } else {
            "directory/file"
        };
        
        let (branch, _) = self.log_branch_names(path);
        
        self.log(format!(
            "CONFLICT ({}): There is a directory with name {} in {}. Adding {} as {}",
            conflict_type, path, branch, path, rename,
        ));
    }
    
    fn log_branch_names(&self, path: &str) -> (String, String) {
        let (a, b) = (self.inputs.left_name(), self.inputs.right_name());
        
        if self.conflicts[path][1].is_some() {
            (b, a)
        } else {
            (a, b)
        }
    }

        self.apply_clean_changes()?;

        // Add conflicts to index
        self.add_conflicts_to_index();

        // Write any untracked files (e.g., from rename conflicts)
        self.write_untracked_files()?;

        Ok(())
    }

    fn merge_blobs(
        &mut self,
        base_oid: Option<&str>,
        left_oid: Option<&str>,
        right_oid: Option<&str>,
    ) -> Result<(bool, String), Error> {
        // Quick resolution for special cases
        if let Some(result) = self.merge3(base_oid, left_oid, right_oid) {
            return Ok((true, result.to_string()));
        }
        
        // Full 3-way merge required
        // Load the blob contents
        let blobs: Vec<String> = vec![base_oid, left_oid, right_oid]
            .into_iter()
            .map(|oid| -> Result<String, Error> {
                if let Some(oid) = oid {
                    let blob_obj = self.database.load(oid)?;
                    let content = blob_obj.to_bytes();
                    Ok(String::from_utf8_lossy(&content).to_string())
                } else {
                    Ok(String::new())
                }
            })
            .collect::<Result<Vec<String>, Error>>()?;
        
        // Perform 3-way merge
        let merge_result = diff3::merge(&blobs[0], &blobs[1], &blobs[2])?;
        
        // Format the result with conflict markers
        let result_text = merge_result.to_string(
            Some(&self.inputs.left_name()),
            Some(&self.inputs.right_name()),
        );
        
        // Create and store the result blob
        let mut blob = Blob::new(result_text.as_bytes().to_vec());
        self.database.store(&mut blob)?;
        
        let blob_oid = blob.get_oid().unwrap_or(&String::new()).clone();
        
        // Return success based on whether the merge was clean
        Ok((merge_result.is_clean(), blob_oid))
    }
    
    fn merge_modes(
        &self,
        base_mode: Option<u32>,
        left_mode: Option<u32>,
        right_mode: Option<u32>,
    ) -> (bool, u32) {
        // Simple 3-way merge for file modes
        if left_mode == base_mode || left_mode == right_mode {
            return (true, right_mode.unwrap_or(FileMode::REGULAR.0));
        } else if right_mode == base_mode {
            return (true, left_mode.unwrap_or(FileMode::REGULAR.0));
        } else if left_mode.is_none() {
            return (false, right_mode.unwrap_or(FileMode::REGULAR.0));
        } else if right_mode.is_none() {
            return (false, left_mode.unwrap_or(FileMode::REGULAR.0));
        }
        
        // Conflicting modes
        (false, left_mode.unwrap_or(FileMode::REGULAR.0))
    }
    
    fn merge3<R: Eq + std::fmt::Debug>(
        &self,
        base: Option<R>,
        left: Option<R>,
        right: Option<R>,
    ) -> Option<R> where R: Clone {
        // One side is missing, use the other side
        if left.is_none() {
            return right.clone();
        }
        if right.is_none() {
            return left.clone();
        }
        
        // Both sides identical, or one side matches base
        if left == right {
            return left.clone();
        }
        if left == base {
            return right.clone();
        }
        if right == base {
            return left.clone();
        }
        
        // True conflict, no simple resolution
        None
    }
    
    fn prepare_tree_diffs(&mut self) -> Result<(), Error> {
        // Get the base OID
        let base_oids = self.inputs.base_oids();
        let base_oid = base_oids.first().map(String::as_str);
        
        // Create a PathFilter for the database.tree_diff calls
        let path_filter = PathFilter::new();
        
        // Calculate diff between base and left (ours)
        self.left_diff = self.database.tree_diff(
            base_oid,
            Some(&self.inputs.left_oid()),
            &path_filter // Pass path_filter reference
        )?;
        
        // Calculate diff between base and right (theirs)
        self.right_diff = self.database.tree_diff(
            base_oid,
            Some(&self.inputs.right_oid()),
            &path_filter // Pass path_filter reference
        )?;
        
        // Initialize containers
        self.clean_diff = HashMap::new();
        self.conflicts = HashMap::new();
        self.untracked = HashMap::new();
        
        // Process each path in the right diff to detect conflicts
        let right_diff = self.right_diff.clone();
        let left_diff = self.left_diff.clone();
        
        // First, check for file/directory conflicts
        for (path, (old_item, new_item)) in &right_diff {
            if new_item.is_some() {
                self.file_dir_conflict(path, &left_diff, &self.inputs.left_name());
            }
            self.same_path_conflict(path, old_item.clone(), new_item.clone())?;
        }
        
        // Check for file/directory conflicts in the other direction
        for (path, (_, new_item)) in &left_diff {
            if new_item.is_some() {
                self.file_dir_conflict(path, &right_diff, &self.inputs.right_name());
            }
        }
        
        // Apply the changes to workspace and index
        self.apply_clean_changes()?;
        
        // Add conflicts to index
        self.add_conflicts_to_index();
        
        // Write any untracked files (e.g., from rename conflicts)
        self.write_untracked_files()?;
        
        Ok(())
    }
    
    fn same_path_conflict(
        &mut self,
        path: &Path,
        base: Option<DatabaseEntry>,
        right: Option<DatabaseEntry>,
    ) -> Result<(), Error> {
        // If we already have a conflict for this path, do nothing
        let path_str = path.to_string_lossy().to_string();
        if self.conflicts.contains_key(&path_str) {
            return Ok(());
        }
        
        // If the path doesn't exist in the left diff, it's a clean change
        if !self.left_diff.contains_key(path) {
            self.clean_diff.insert(path.to_path_buf(), (base, right));
            return Ok(());
        }
        
        // Get the left (ours) version
        let left = self.left_diff[path].1.clone();
        
        // If left and right are identical, no conflict
        if left == right {
            return Ok(());
        }
        
        // Extract OIDs and modes
        let base_oid = base.as_ref().map(|b| b.get_oid().to_string());
        let left_oid = left.as_ref().map(|l| l.get_oid().to_string());
        let right_oid = right.as_ref().map(|r| r.get_oid().to_string());
        
        let base_mode = base.as_ref().map(|b| FileMode::parse(b.get_mode()).0);
        let left_mode = left.as_ref().map(|l| FileMode::parse(l.get_mode()).0);
        let right_mode = right.as_ref().map(|r| FileMode::parse(r.get_mode()).0);
        
        // Log that we're merging this file if both sides modified it
        if left.is_some() && right.is_some() {
            self.log(format!("Auto-merging {}", path_str));
        }
        
        // Try to merge the blob contents
        let (oid_ok, oid) = self.merge_blobs(
            base_oid.as_deref(),
            left_oid.as_deref(),
            right_oid.as_deref(),
        )?;
        
        // Try to merge the file modes
        let (mode_ok, mode) = self.merge_modes(base_mode, left_mode, right_mode);
        
        // Convert mode to string format
        let mode_str = FileMode(mode).to_octal_string();
        
        // Add to clean diff
        self.clean_diff.insert(
            path.to_path_buf(),
            (base.clone(), Some(DatabaseEntry::new(
                path_str.clone(),
                oid.clone(),
                &mode_str,
            ))),
        );
        
        // If either merge failed, record a conflict
        if !oid_ok || !mode_ok {
            self.conflicts.insert(path_str.clone(), vec![base, left, right]);
            self.log_conflict(path, None);
        }
        
        Ok(())
    }