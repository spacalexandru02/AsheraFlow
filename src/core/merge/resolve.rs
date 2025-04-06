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

        Ok(())
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

            // --- Clone needed for conflict vec ---
            let old_item_clone = old_item.clone();
            let new_item_clone = new_item.clone();

            if name == self.inputs.left_name() {
                // Left has a file, right has a directory at this path
                self.conflicts.insert(
                    parent_path.clone(),
                    vec![old_item_clone, new_item_clone, None], // Use clones
                );
            } else {
                // Right has a file, left has a directory at this path
                self.conflicts.insert(
                    parent_path.clone(),
                    vec![old_item_clone, None, new_item_clone], // Use clones
                );
            }

            // Remove from clean diff since it's now a conflict
            self.clean_diff.remove(&parent);

            // Rename conflicting file to avoid data loss
            let rename = format!("{}~{}", parent_path, name);
             // --- Clone needed for untracked insert ---
             // Ensure new_item is Some before unwrapping
             if let Some(entry_to_rename) = new_item {
                self.untracked.insert(rename.clone(), entry_to_rename.clone());
                 // Log the conflict
                if !diff.contains_key(path) {
                    self.log(format!("Adding {}", path.to_string_lossy()));
                }
                self.log_conflict(&parent, Some(rename)); // Pass rename by value
             } else {
                 // Log an error or handle the case where new_item is None but we expected it
                 eprintln!("Error: Expected DatabaseEntry for renaming in file_dir_conflict for path {}", parent_path);
             }
        }
    }


    fn apply_clean_changes(&mut self) -> Result<(), Error> {
        // Process all clean changes (no conflicts)
        // Clone clean_diff to avoid borrowing issues while iterating and modifying workspace/index
        let clean_diff_clone = self.clean_diff.clone();
        for (path, (_, new_entry)) in clean_diff_clone { // Iterate over the clone
            if let Some(entry) = new_entry {
                // File being added or modified
                //let path_str = path.to_string_lossy().to_string();

                // Load blob content only if it's not a directory
                if !FileMode::parse(entry.get_mode()).is_directory() {
                    let blob_obj = self.database.load(&entry.get_oid())?;
                    let content = blob_obj.to_bytes();

                    // Ensure parent directory exists
                     if let Some(parent) = path.parent() {
                         self.workspace.make_directory(parent)?;
                     }

                    // Write file to workspace
                    self.workspace.write_file(&path, &content)?; // Pass path by reference

                    // Update index
                    if let Ok(stat) = self.workspace.stat_file(&path) { // Pass path by reference
                        self.index.add(&path, &entry.get_oid(), &stat)?; // Pass path by reference
                    }
                } else {
                    // It's a directory, ensure it exists
                    self.workspace.make_directory(&path)?; // Pass path by reference
                    // Optionally update index if needed for directories, but typically not needed
                     // Update index for the directory entry itself
                     if let Ok(stat) = self.workspace.stat_file(&path) { // Pass path by reference
                         self.index.add(&path, &entry.get_oid(), &stat)?; // Pass path by reference
                     }
                }

            } else {
                // File or directory being deleted
                self.workspace.remove_file(&path)?; // remove_file handles both files and dirs, Pass path by reference

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
             let path_obj = Path::new(path);
             if let Some(parent) = path_obj.parent() {
                 self.workspace.make_directory(parent)?;
             }
            self.workspace.write_file(path_obj, &content)?;
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
        // Check if the key exists before accessing it
        if let Some(conflict) = self.conflicts.get(&path_str) {
            // Clone values before using them if needed multiple times or moved
            let base = conflict[0].clone();
            let left = conflict[1].clone();
            let right = conflict[2].clone();

            if left.is_some() && right.is_some() {
                self.log_left_right_conflict(&path_str);
            } else if base.is_some() && (left.is_some() || right.is_some()) {
                self.log_modify_delete_conflict(&path_str, rename); // rename is Option<String>, passes ownership ok
            } else if let Some(renamed_to) = rename { // Use the value directly if Some
                self.log_file_directory_conflict(&path_str, renamed_to);
            }
        } else {
            // Log a generic message if the conflict details are missing for some reason
             self.log(format!("CONFLICT: Merge conflict detected for {}", path_str));
        }
    }


    fn log_left_right_conflict(&self, path: &str) {
         // Check if the key exists before accessing it
         if let Some(conflict) = self.conflicts.get(path) {
             // Clone values before using them if needed multiple times or moved
             let base = conflict[0].clone();

            let conflict_type = if base.is_some() {
                "content"
            } else {
                "add/add"
            };
             self.log(format!("CONFLICT ({}): Merge conflict in {}", conflict_type, path));
         } else {
             self.log(format!("CONFLICT (unknown type): Merge conflict in {}", path));
         }
    }

    fn log_modify_delete_conflict(&self, path: &str, rename: Option<String>) {
        let (deleted, modified) = self.log_branch_names(path);

        let rename_msg = if let Some(rename_val) = rename { // Borrow rename temporarily
            format!(" at {}", rename_val)
        } else {
            String::new()
        };


        self.log(format!(
            "CONFLICT (modify/delete): {} deleted in {} and modified in {}. Version {} of {} left in tree{}.",
            path, deleted, modified, modified, path, rename_msg,
        ));
    }

    fn log_file_directory_conflict(&self, path: &str, rename: String) {
        // Check if the key exists before accessing it
        let conflict_type = if let Some(conflict) = self.conflicts.get(path) {
             // Clone before use
             let left = conflict[1].clone();
            if left.is_some() { "file/directory" } else { "directory/file" }
        } else {
             "unknown" // Fallback if conflict details are missing
        };


        let (branch, _) = self.log_branch_names(path);

        self.log(format!(
            "CONFLICT ({}): There is a directory with name {} in {}. Adding {} as {}",
            conflict_type, path, branch, // Use branch name here
            path, rename, // rename is moved here
        ));
    }


    fn log_branch_names(&self, path: &str) -> (String, String) {
        let (a, b) = (self.inputs.left_name(), self.inputs.right_name());

         // Check if the key exists before accessing it
         if let Some(conflict) = self.conflicts.get(path) {
              // Clone values before using them if needed multiple times or moved
             let left = conflict[1].clone();

            if left.is_some() {
                (b.clone(), a.clone()) // Clone strings
            } else {
                (a.clone(), b.clone()) // Clone strings
            }
         } else {
             // Default branch names if conflict details are missing
             (a.clone(), b.clone()) // Clone strings
         }
    }

    fn merge_blobs(
        &mut self,
        base_oid: Option<&str>,
        left_oid: Option<&str>,
        right_oid: Option<&str>,
    ) -> Result<(bool, String), Error> {
        // Quick resolution for special cases
        if let Some(result) = self.merge3_oid(base_oid, left_oid, right_oid) {
            return Ok((true, result.to_string()));
        }

        // Full 3-way merge required
        // Load the blob contents
        let blobs: Vec<String> = vec![base_oid, left_oid, right_oid]
            .into_iter()
            .map(|oid| -> Result<String, Error> {
                if let Some(oid_str) = oid {
                     // Ensure OID is valid hex before loading
                     if oid_str.len() == 40 && oid_str.chars().all(|c| c.is_ascii_hexdigit()) {
                         let blob_obj = self.database.load(oid_str)?;
                         let content = blob_obj.to_bytes();
                         Ok(String::from_utf8_lossy(&content).to_string())
                     } else {
                         // Invalid OID format, treat as empty
                         Ok(String::new())
                     }
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

        let blob_oid = blob.get_oid().map(|s| s.to_string()).unwrap_or_default();


        // Return success based on whether the merge was clean
        Ok((merge_result.is_clean(), blob_oid))
    }

    fn merge_modes(
        &self,
        base_mode: Option<FileMode>, // Use FileMode directly
        left_mode: Option<FileMode>,
        right_mode: Option<FileMode>,
    ) -> (bool, FileMode) { // Return FileMode
        // Simple 3-way merge for file modes
        if left_mode == base_mode || left_mode == right_mode {
            return (true, right_mode.unwrap_or(FileMode::REGULAR));
        } else if right_mode == base_mode {
            return (true, left_mode.unwrap_or(FileMode::REGULAR));
        } else if left_mode.is_none() {
            // Mode deleted on left, use right if it exists, otherwise conflict (false)
             return (right_mode.is_none(), right_mode.unwrap_or(FileMode::REGULAR));
        } else if right_mode.is_none() {
             // Mode deleted on right, use left
             return (false, left_mode.unwrap_or(FileMode::REGULAR)); // Conflict if left exists
        }


        // Conflicting modes - prefer left side as per original logic
        (false, left_mode.unwrap_or(FileMode::REGULAR))
    }

    // Specific merge3 for Option<&str> representing OIDs
    fn merge3_oid<'b>(
        &self,
        base: Option<&'b str>,
        left: Option<&'b str>,
        right: Option<&'b str>,
    ) -> Option<&'b str> {
         if left == right { return left; }
         if left == base { return right; }
         if right == base { return left; }
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

        // Collect all unique paths from both diffs
        let mut all_paths = HashSet::new();
        for path in self.left_diff.keys() {
            all_paths.insert(path.clone());
        }
        for path in self.right_diff.keys() {
            all_paths.insert(path.clone());
        }

        // --- Refactor Loop to Avoid Borrowing Conflicts ---
        // Create a list of paths to process to avoid iterating while potentially modifying self
        let paths_to_process: Vec<PathBuf> = all_paths.into_iter().collect();

        for path in paths_to_process {
             // --- Extract necessary data immutably first ---
             let base_entry = self.left_diff.get(&path).and_then(|(old, _)| old.clone())
                 .or_else(|| self.right_diff.get(&path).and_then(|(old, _)| old.clone()));
             let left_entry = self.left_diff.get(&path).and_then(|(_, new)| new.clone());
             let right_entry = self.right_diff.get(&path).and_then(|(_, new)| new.clone());

             let left_new_is_some = self.left_diff.get(&path).map_or(false, |(_, new)| new.is_some());
             let left_new_is_dir = self.left_diff.get(&path)
                 .and_then(|(_, new)| new.as_ref())
                 .map_or(false, |e| e.get_file_mode().is_directory());

             let right_new_is_some = self.right_diff.get(&path).map_or(false, |(_, new)| new.is_some());
             let right_new_is_dir = self.right_diff.get(&path)
                 .and_then(|(_, new)| new.as_ref())
                 .map_or(false, |e| e.get_file_mode().is_directory());

            // --- Now perform mutable operations ---
             // same_path_conflict requires &mut self
             self.same_path_conflict(&path, base_entry, left_entry, right_entry)?;

             // --- Perform immutable checks AFTER mutable operation ---
             // Check for parent/directory conflicts (these checks are now immutable)
             let mut conflict_found = false;
             if left_new_is_some && !left_new_is_dir {
                 if let Some(conflicting_parent) = self.check_parent_dir_conflict(&path, &self.right_diff, &self.inputs.right_name()) {
                     // --- Record conflict based on return value ---
                     self.record_parent_dir_conflict(&path, &conflicting_parent, &self.inputs.right_name());
                     conflict_found = true;
                 }
             }
             if !conflict_found && right_new_is_some && !right_new_is_dir { // Avoid double recording
                 if let Some(conflicting_parent) = self.check_parent_dir_conflict(&path, &self.left_diff, &self.inputs.left_name()) {
                     // --- Record conflict based on return value ---
                     self.record_parent_dir_conflict(&path, &conflicting_parent, &self.inputs.left_name());
                 }
             }
        }

        Ok(())
    }


     // --- check_parent_dir_conflict now takes &self and returns Option<PathBuf> ---
     fn check_parent_dir_conflict(
         &self, // Changed back to &self
         file_path: &Path,
         other_diff: &HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
         _other_branch_name: &str, // Name not strictly needed here anymore
     ) -> Option<PathBuf> { // Return Option<PathBuf>
         let mut current = file_path.parent();
         while let Some(parent_dir) = current {
             if parent_dir.as_os_str().is_empty() { break; } // Stop at root

             if let Some((_, other_parent_change)) = other_diff.get(parent_dir) {
                 // If the parent directory was modified or added on the other side
                 if other_parent_change.is_some() {
                     // Found a conflict, return the path of the conflicting parent directory
                     return Some(parent_dir.to_path_buf());
                 }
             }
             current = parent_dir.parent();
         }
         // No conflict found
         None
     }

     // --- New function to record the conflict based on check_parent_dir_conflict's result ---
      fn record_parent_dir_conflict(
          &mut self,
          file_path: &Path,
          conflicting_parent: &Path,
          other_branch_name: &str,
      ) {
          let file_path_str = file_path.to_string_lossy().to_string();
          let parent_path_str = conflicting_parent.to_string_lossy().to_string();

          // Avoid double-marking if already a conflict
          if !self.conflicts.contains_key(&file_path_str) {
               self.log(format!(
                   "CONFLICT (directory/file): Modification of file '{}' conflicts with modification of parent directory '{}' in branch '{}'",
                   file_path_str, parent_path_str, other_branch_name
               ));
               // Mark the file itself as conflicted (using existing entries if available)
              // Clone base, left, right before moving into vec!
              let base = self.left_diff.get(file_path).and_then(|(b, _)| b.clone());
              let left = self.left_diff.get(file_path).and_then(|(_, l)| l.clone());
              let right = self.right_diff.get(file_path).and_then(|(_, r)| r.clone());
              self.conflicts.insert(file_path_str.clone(), vec![base, left, right]);
              self.clean_diff.remove(file_path); // Remove from clean changes
          }
      }


    fn handle_directory_conflict(
        &mut self,
        path: &Path,
        other_diff: &HashMap<PathBuf, (Option<DatabaseEntry>, Option<DatabaseEntry>)>,
        branch_name: &str
    ) -> Result<(), Error> {
        // Check if the other diff has a file at this path
        if let Some((_, new_item_opt)) = other_diff.get(path) {
             if let Some(new_item) = new_item_opt { // Check if new_item is Some
                let new_mode = new_item.get_file_mode();
                if !new_mode.is_directory() {
                    // Actual file/directory conflict
                    let path_str = path.to_string_lossy().to_string();
                    let rename = format!("{}~{}", path_str, branch_name);

                    // Determine which entry corresponds to the file
                    let file_entry = Some(new_item.clone()); // Clone the file entry


                    self.conflicts.insert(
                        path_str.clone(),
                        // Conflict structure: [base, left_is_file, right_is_dir] or [base, left_is_dir, right_is_file]
                         // If branch_name is left, right has the file. If branch_name is right, left has the file.
                         if branch_name == self.inputs.left_name() {
                              // Pass None for base and left (dir side), file_entry for right
                             vec![None, None, file_entry]
                         } else {
                              // Pass None for base and right (dir side), file_entry for left
                             vec![None, file_entry, None]
                         }
                    );

                    // Rename the conflicting file to avoid data loss
                     self.untracked.insert(rename.clone(), new_item.clone()); // Clone needed for insert


                    self.log(format!("CONFLICT (file/directory): {} exists as both a file and a directory", path_str));
                }
             }
        }
        Ok(())
    }


    // Updated same_path_conflict to handle Option<DatabaseEntry> correctly
    fn same_path_conflict(
        &mut self, // Requires mutable borrow
        path: &Path,
        base: Option<DatabaseEntry>,
        left: Option<DatabaseEntry>,
        right: Option<DatabaseEntry>,
    ) -> Result<(), Error> {
        let path_str = path.to_string_lossy().to_string();

        // If we already have a conflict for this path, do nothing
        if self.conflicts.contains_key(&path_str) {
            return Ok(());
        }

        // Check for file/directory conflict first
        let left_is_dir = left.as_ref().map_or(false, |e| e.get_file_mode().is_directory());
        let right_is_dir = right.as_ref().map_or(false, |e| e.get_file_mode().is_directory());

        // If one is a directory and the other is a file (and exists)
         if (left_is_dir && right.is_some() && !right_is_dir) || (right_is_dir && left.is_some() && !left_is_dir) {
             self.log(format!("CONFLICT (file/directory): {} is a file in one version and a directory in another.", path_str));
              // --- Fix: Clone base, left, right before moving into vec! ---
             self.conflicts.insert(path_str.clone(), vec![base.clone(), left.clone(), right.clone()]);
             self.clean_diff.remove(path); // Ensure it's not processed as clean
             // Decide which one to possibly rename/keep untracked if needed, e.g., the file one
              // --- Fix: Use cloned values for file_entry ---
             let file_entry_ref = if left_is_dir { right.as_ref() } else { left.as_ref() }; // Borrow first
             if let Some(entry_to_rename) = file_entry_ref { // Check if Some before cloning
                  let rename_branch = if left_is_dir { self.inputs.right_name() } else { self.inputs.left_name() };
                  let rename_path = format!("{}~{}", path_str, rename_branch);
                  // --- Fix: Clone rename_path before move ---
                  self.untracked.insert(rename_path.clone(), entry_to_rename.clone());
                  // --- Fix: Pass cloned rename_path to format! ---
                  self.log(format!("  Renaming file version to {}", rename_path));
             }
             return Ok(());
         }

        // If left and right are identical (including None), it's clean
        if left == right {
             // If both are None, it's not a change relative to base, do nothing special here
             // If both are Some and equal, add to clean_diff if different from base
             if left.is_some() && left != base {
                 // Clone base and left before moving into tuple for clean_diff
                 self.clean_diff.insert(path.to_path_buf(), (base.clone(), left.clone()));
             }
            return Ok(());
        }


        // --- Start Merge Logic ---
        // Borrow OIDs as &str
        let base_oid_str = base.as_ref().map(|b| b.get_oid());
        let left_oid_str = left.as_ref().map(|l| l.get_oid());
        let right_oid_str = right.as_ref().map(|r| r.get_oid());


        let base_mode = base.as_ref().map(|b| b.get_file_mode());
        let left_mode = left.as_ref().map(|l| l.get_file_mode());
        let right_mode = right.as_ref().map(|r| r.get_file_mode());


        // Log merge attempt only if both sides changed it differently
        if left.is_some() && right.is_some() && left != base && right != base && left != right {
             // Skip logging for directory entries as we don't merge their "content" directly
             if !left_is_dir && !right_is_dir {
                 self.log(format!("Auto-merging {}", path_str));
             }
        }

        // Merge Modes
        let (mode_ok, merged_mode) = self.merge_modes(base_mode, left_mode, right_mode);

        // Merge Content (OIDs) - Skip for directories
         let (oid_ok, merged_oid_str_result) = if left_is_dir || right_is_dir {
             // For directories, "merge" means taking the OID from the side that changed,
             // or conflict if both changed differently. Use merge3_oid logic.
             let merged_oid = self.merge3_oid(base_oid_str, left_oid_str, right_oid_str); // Pass &str
             if let Some(oid) = merged_oid {
                  // Mode merge determines final ok status, oid is ok here
                 (true, oid.to_string())
             } else {
                 // Both directories changed differently, this is a conflict.
                 // Mark oid as not ok. Pick left OID arbitrarily for the conflict entry.
                  (false, left_oid_str.unwrap_or("").to_string())
             }
         } else {
             // For files, perform blob merge
             self.merge_blobs(base_oid_str, left_oid_str, right_oid_str)? // Pass &str
         };


        // Determine the final merged entry
        // It exists if either left or right exists after the merge attempt
        let merged_entry = if left.is_some() || right.is_some() {
             // Check if the merge resulted in a valid OID before creating entry
             if !merged_oid_str_result.is_empty() {
                 Some(DatabaseEntry::new(
                     path_str.clone(),
                     merged_oid_str_result.clone(), // Use the merged OID string
                     &merged_mode.to_octal_string(),
                 ))
             } else {
                  // If merge failed to produce an OID (shouldn't happen with current logic but safer)
                  None
             }
        } else {
            None // Both sides deleted it compared to base
        };


        // Add to clean diff if it's different from base
         // Ensure we handle the case where the final result is deletion (merged_entry is None)
         if merged_entry != base {
             self.clean_diff.insert(
                 path.to_path_buf(),
                 (base.clone(), merged_entry.clone()), // Clone base and merged_entry
             );
         }


        // Record conflict if either merge failed
        if !oid_ok || !mode_ok {
             // Clone base, left, right before moving into vec!
             // Check if base, left, or right is None before cloning
            let base_clone = base.clone();
            let left_clone = left.clone();
            let right_clone = right.clone();
            self.conflicts.insert(path_str.clone(), vec![base_clone, left_clone, right_clone]);

             // Determine the rename path based on which side's file might need renaming
             let rename_path_opt = if left_is_dir && right.is_some() { // Right was the file
                  Some(format!("{}~{}", path_str, self.inputs.right_name()))
             } else if right_is_dir && left.is_some() { // Left was the file
                  Some(format!("{}~{}", path_str, self.inputs.left_name()))
             } else { // General conflict, no specific file rename needed here usually
                  None
             };

             self.log_conflict(path, rename_path_opt); // Pass Option<String>


        }

        Ok(())
    }
}