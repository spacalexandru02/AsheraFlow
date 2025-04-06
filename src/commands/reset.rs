// src/commands/reset.rs
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::collections::HashMap;

use crate::errors::error::Error;
use crate::core::color::Color;
use crate::core::repository::repository::Repository;
use crate::core::revision::Revision;
use crate::core::database::database::Database;
use crate::core::database::tree::{Tree, TreeEntry};
use crate::core::database::entry::DatabaseEntry;
use crate::core::database::commit::Commit;
use crate::core::refs::Refs;
use crate::core::path_filter::PathFilter;

// Constant for original head reference
pub const ORIG_HEAD: &str = "ORIG_HEAD";

// Different reset modes
enum ResetMode {
    Soft,   // Only move HEAD (don't touch index or working directory)
    Mixed,  // Move HEAD and reset index (default)
    Hard,   // Move HEAD, reset index, and reset working directory
}

pub struct ResetCommand;

impl ResetCommand {
    pub fn execute(revision: &str, paths: &[String], soft: bool, mixed: bool, hard: bool) -> Result<(), Error> {
        let start_time = Instant::now();

        // Determine the mode based on flags
        let mode = if hard {
            ResetMode::Hard
        } else if soft {
            ResetMode::Soft
        } else {
            // Mixed is the default
            ResetMode::Mixed
        };
        
        println!("Starting reset operation...");

        // Initialize repository
        let mut repo = Repository::new(".")?;
        
        // Get the current HEAD to save as ORIG_HEAD later
        let current_head = match repo.refs.read_head()? {
            Some(oid) => oid,
            None => {
                return Err(Error::Generic("No HEAD commit found. Repository may be empty.".into()));
            }
        };
        
        // Determine the target commit OID
        let target_commit_oid = if !revision.is_empty() {
            // Resolve the revision to a commit ID
            let mut revision_resolver = Revision::new(&mut repo, revision);
            revision_resolver.resolve("commit")?
        } else if !paths.is_empty() {
            // If no revision is given but paths are specified, use current HEAD
            current_head.clone()
        } else {
            // Default case: reset to HEAD
            current_head.clone()
        };

        // Convert paths to PathBuf
        let path_buffers: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        
        // Lock the index for updating
        repo.index.load_for_update()?;
        
        // Perform the reset based on the mode
        match mode {
            ResetMode::Soft => {
                if !paths.is_empty() {
                    println!("Warning: Ignoring paths with --soft option");
                }
                
                // Only update HEAD if no paths are specified
                if paths.is_empty() {
                    Self::update_refs(&repo.refs, &target_commit_oid, &current_head)?;
                }
            },
            ResetMode::Mixed => {
                // Reset the index but leave the working directory unchanged
                if paths.is_empty() {
                    // Reset entire index
                    Self::reset_all(&mut repo, &target_commit_oid)?;
                    
                    // Update HEAD reference if no paths are specified
                    Self::update_refs(&repo.refs, &target_commit_oid, &current_head)?;
                } else {
                    // Reset specific paths in the index
                    Self::reset_paths(&mut repo, &target_commit_oid, &path_buffers)?;
                }
            },
            ResetMode::Hard => {
                if !paths.is_empty() {
                    println!("Warning: Ignoring paths with --hard option");
                }
                
                // Reset the index and working directory
                Self::hard_reset(&mut repo, &target_commit_oid)?;
                
                // Update HEAD reference if no paths are specified
                if paths.is_empty() {
                    Self::update_refs(&repo.refs, &target_commit_oid, &current_head)?;
                }
            },
        }
        
        // Write index updates
        repo.index.write_updates()?;
        
        let elapsed = start_time.elapsed();
        println!("Reset completed in {:.2}s", elapsed.as_secs_f32());
        
        Ok(())
    }
    
    // Update HEAD and ORIG_HEAD references
    fn update_refs(refs: &Refs, target_oid: &str, current_head: &str) -> Result<(), Error> {
        // Save current HEAD to ORIG_HEAD
        let orig_head_path = Path::new(ORIG_HEAD);
        refs.update_ref_file(orig_head_path, current_head)?;
        
        // Update HEAD to target commit
        refs.update_head(target_oid)?;
        
        println!("{}: {} -> {}", 
            Color::green("HEAD is now at"),
            target_oid[0..std::cmp::min(8, target_oid.len())].to_string(),
            Color::yellow(&target_oid[0..std::cmp::min(8, target_oid.len())])
        );
        
        Ok(())
    }
    
    // Reset entire index to match a commit
    fn reset_all(repo: &mut Repository, commit_oid: &str) -> Result<(), Error> {
        println!("Resetting index to commit {}", 
            commit_oid[0..std::cmp::min(8, commit_oid.len())].to_string()
        );
        
        // Clear the index first
        repo.index.clear();
        
        // Load the commit and its tree
        let commit_obj = repo.database.load(commit_oid)?;
        let commit = match commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => return Err(Error::Generic(format!("Object {} is not a commit", commit_oid))),
        };
        
        let tree_oid = commit.get_tree();
        println!("Loading tree from commit: {}", tree_oid);
        
        // Use path filter to get all files
        let path_filter = PathFilter::new();
        
        // Load all entries from the tree
        let entries = Self::load_tree_list(&mut repo.database, Some(tree_oid), None, &path_filter)?;
        
        // Add each entry to the index
        for (path, entry) in entries {
            println!("Adding to index: {}", path.display());
            repo.index.add_from_db(&path, &entry)?;
        }
        
        Ok(())
    }
    
    // Reset specific paths in the index
    fn reset_paths(repo: &mut Repository, commit_oid: &str, paths: &[PathBuf]) -> Result<(), Error> {
        println!("Resetting {} paths in index to commit {}", 
            paths.len(),
            commit_oid[0..std::cmp::min(8, commit_oid.len())].to_string()
        );
        
        // Load the commit and its tree
        let commit_obj = repo.database.load(commit_oid)?;
        let commit = match commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => return Err(Error::Generic(format!("Object {} is not a commit", commit_oid))),
        };
        
        let tree_oid = commit.get_tree();
        
        // Process each path
        for path in paths {
            // Remove existing entry from the index
            let path_str = path.to_string_lossy().to_string();
            repo.index.remove(&path_str)?;
            
            // Create a path filter to include just this path
            let mut filter_paths = Vec::new();
            filter_paths.push(path.clone());
            let path_filter = PathFilter::build(&filter_paths);
            
            // Load entries from the tree that match this path
            let entries = Self::load_tree_list(&mut repo.database, Some(tree_oid), Some(path), &path_filter)?;
            
            // Add each entry to the index
            for (entry_path, entry) in entries {
                println!("Adding to index: {}", entry_path.display());
                repo.index.add_from_db(&entry_path, &entry)?;
            }
        }
        
        Ok(())
    }
    
    // Hard reset - update both index and working directory
    fn hard_reset(repo: &mut Repository, commit_oid: &str) -> Result<(), Error> {
        println!("Hard resetting to commit {}", 
            commit_oid[0..std::cmp::min(8, commit_oid.len())].to_string()
        );
        
        // Load the commit and its tree
        let commit_obj = repo.database.load(commit_oid)?;
        let commit = match commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => return Err(Error::Generic(format!("Object {} is not a commit", commit_oid))),
        };
        
        let tree_oid = commit.get_tree();
        
        // Get the HEAD commit tree to calculate changes
        let head_oid = repo.refs.read_head()?;
        let path_filter = PathFilter::new();
        
        // Get the tree diff between HEAD and target commit
        let tree_diff = repo.tree_diff(head_oid.as_deref(), Some(commit_oid))?;
        println!("Found {} differences between HEAD and target commit", tree_diff.len());
        
        // Clear the index
        repo.index.clear();
        
        // Use migration to apply changes to workspace and index
        let mut migration = repo.migration(tree_diff);
        
        match migration.apply_changes() {
            Ok(_) => {
                println!("Working directory and index updated successfully");
                Ok(())
            },
            Err(e) => {
                // If migration fails, ensure we release the index lock
                repo.index.rollback()?;
                Err(e)
            }
        }
    }
    
    // Load all entries from a tree matching a certain path and filter
    fn load_tree_list(
        database: &mut Database,
        tree_oid: Option<&str>,
        path: Option<&Path>,
        filter: &PathFilter
    ) -> Result<HashMap<PathBuf, DatabaseEntry>, Error> {
        let mut entries = HashMap::new();
        
        // If no tree OID provided or empty repository, return empty list
        let tree_oid = match tree_oid {
            Some(oid) => oid,
            None => return Ok(entries),
        };
        
        // Load the tree object
        let tree_obj = database.load(tree_oid)?;
        let tree = match tree_obj.as_any().downcast_ref::<Tree>() {
            Some(t) => t,
            None => return Err(Error::Generic(format!("Object {} is not a tree", tree_oid))),
        };
        
        // Get base path
        let base_path = path.unwrap_or_else(|| Path::new(""));
        
        // Collect entries from the tree
        Self::collect_entries_from_tree(
            database,
            tree,
            base_path.to_path_buf(),
            &mut entries,
            filter
        )?;
        
        Ok(entries)
    }
    
    // Recursively collect entries from a tree
    fn collect_entries_from_tree(
        database: &mut Database,
        tree: &Tree,
        prefix: PathBuf,
        entries: &mut HashMap<PathBuf, DatabaseEntry>,
        filter: &PathFilter
    ) -> Result<(), Error> {
        // Apply path filter to tree entries
        let filtered_entries = filter.filter_entries(tree.get_entries());
        
        for (name, entry) in filtered_entries {
            let entry_path = if prefix.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                prefix.join(name)
            };
            
            match entry {
                TreeEntry::Blob(oid, mode) => {
                    // Create a database entry
                    let db_entry = DatabaseEntry::new(
                        entry_path.to_string_lossy().to_string(),
                        oid.clone(),
                        &mode.to_octal_string()
                    );
                    
                    // Add to result
                    entries.insert(entry_path, db_entry);
                },
                TreeEntry::Tree(subtree) => {
                    if let Some(subtree_oid) = subtree.get_oid() {
                        // Load subtree and process recursively
                        let subtree_obj = database.load(subtree_oid)?;
                        if let Some(loaded_subtree) = subtree_obj.as_any().downcast_ref::<Tree>() {
                            // Create a sub-filter for this directory
                            let sub_filter = filter.join(name);
                            
                            // Process subtree recursively
                            Self::collect_entries_from_tree(
                                database,
                                loaded_subtree,
                                entry_path,
                                entries,
                                &sub_filter
                            )?;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
}