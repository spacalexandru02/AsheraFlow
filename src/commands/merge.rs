// src/commands/merge.rs
use std::time::Instant;
use std::env;
use std::path::{Path, PathBuf}; // Ensure PathBuf is imported
use std::collections::HashMap; // Ensure HashMap is imported
use crate::errors::error::Error;
use crate::core::merge::inputs::Inputs;
use crate::core::merge::resolve::Resolve;
use crate::core::refs::Refs;
use crate::core::database::database::Database; // Import Database
use crate::core::database::database::GitObject; // Import GitObject Trait
use crate::core::database::commit::Commit;
use crate::core::database::author::Author;
use crate::core::path_filter::PathFilter;
use crate::core::workspace::Workspace;
use crate::core::database::tree::{Tree, TreeEntry}; // Import TreeEntry
use crate::core::file_mode::FileMode;
use crate::core::database::entry::DatabaseEntry; // Import DatabaseEntry


const MERGE_MSG: &str = "\
Merge branch '%s'

# Please enter a commit message to explain why this merge is necessary,
# especially if it merges an updated upstream into a topic branch.
#
# Lines starting with '#' will be ignored, and an empty message aborts
# the commit.
";

pub struct MergeCommand;

impl MergeCommand {
    pub fn execute(revision: &str, message: Option<&str>) -> Result<(), Error> {
        let start_time = Instant::now();

        println!("Merge started...");

        // Initialize repository components
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");

        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }

        let workspace = Workspace::new(root_path);
        let mut database = Database::new(git_path.join("objects"));
        let mut index = crate::core::index::index::Index::new(git_path.join("index"));
        let refs = Refs::new(&git_path);

        // Load the current index
        if !index.load_for_update()? {
            return Err(Error::Lock("Failed to acquire lock on index".to_string()));
        }

        // Check for conflicts or pending operations
        if index.has_conflict() {
            index.rollback()?;
            return Err(Error::Generic("Cannot merge with conflicts. Fix conflicts and commit first.".into()));
        }

        // Get the current HEAD
        let head_oid = match refs.read_head()? {
            Some(oid) => oid,
            None => {
                 index.rollback()?;
                 return Err(Error::Generic("No HEAD commit found. Create an initial commit first.".into()));
            }
        };

        // Parse merge inputs
        let inputs = Inputs::new(&mut database, &refs, "HEAD".to_string(), revision.to_string())?;

        // Check for already merged or fast-forward cases
        if inputs.already_merged() {
            println!("Already up to date.");
            index.rollback()?; // Release lock
            return Ok(());
        }

        if inputs.is_fast_forward() {
            // Handle fast-forward merge
            println!("Fast-forward possible.");
            let result = Self::handle_fast_forward(
                &mut database,
                &workspace,
                &mut index, // Pass mutable index
                &refs,
                &inputs.left_oid, // Current HEAD OID
                &inputs.right_oid // Target commit OID
            );
            // handle_fast_forward manages its own lock release/commit
            return result;
        }

        // Perform a real merge
         println!("Performing recursive merge.");
        let mut merge = Resolve::new(&mut database, &workspace, &mut index, &inputs);
        merge.on_progress = |info| println!("{}", info);

        // Execute the merge (applies clean changes to workspace and index)
        if let Err(e) = merge.execute() {
             index.rollback()?; // Release lock on error
             return Err(e);
        }

        // Write the index updates (after clean changes are applied by Resolve)
        // Resolve should have set index.changed = true if there were changes
        if !index.write_updates()? {
             // If write_updates returned false, it means no changes were detected
             // by the Resolve step, which might indicate an issue, but we proceed.
             println!("Warning: Index was not written as no changes were detected by Resolve.");
        }


        // Check for conflicts AFTER applying clean changes and writing index
        if index.has_conflict() {
            println!("\nAutomatic merge failed; fix conflicts and then commit the result.");
            // Keep index locked with conflicts - Don't rollback here
            return Err(Error::Generic("Automatic merge failed; fix conflicts and then commit the result.".into()));
        }

        // Commit the successful merge
        let commit_message = message.map(|s| s.to_string()).unwrap_or_else(|| {
            format!("Merge branch '{}' into {}", revision, inputs.left_name) // Make message clearer
        });

        // Create author
        let author = Author::new(
            env::var("GIT_AUTHOR_NAME").unwrap_or_else(|_| "A. U. Thor".to_string()),
            env::var("GIT_AUTHOR_EMAIL").unwrap_or_else(|_| "author@example.com".to_string()),
        );

        // Get tree from the updated index
        let tree_oid = match Self::write_tree_from_index(&mut database, &mut index) {
            Ok(oid) => oid,
            Err(e) => {
                // Index lock state managed by write_tree_from_index or its callees on error
                return Err(e);
            }
        };

        // Create commit with two parents
        let parent1 = head_oid.clone(); // The current branch HEAD
        let parent2 = inputs.right_oid.clone(); // The commit being merged

        let mut commit = Commit::new(
            Some(parent1), // First parent is HEAD
            tree_oid.clone(),
            author.clone(),
            commit_message, // Initial message
        );
        // Add second parent information. We'll store this directly in the commit message for simplicity,
        // though git uses separate 'parent' lines in the commit object raw data.
        let final_message = format!("{}\n\nParent: {}", commit.get_message(), parent2);
         commit = Commit::new( // Recreate commit with final message and parent info
             commit.get_parent().cloned(), // Keep first parent
             tree_oid.clone(),
             author.clone(),
             final_message, // Use the message including the second parent info
         );


        // Store the commit
        if let Err(e) = database.store(&mut commit) {
             // Lock state? Assume store doesn't affect index lock
             return Err(e);
        }

        // Update HEAD to the new commit
        let commit_oid = match commit.get_oid() {
            Some(oid) => oid.clone(),
             None => {
                 // Lock state? Assume commit_oid error doesn't affect index lock
                 return Err(Error::Generic("Commit OID not set after storage".into()));
             }
        };

        if let Err(e) = refs.update_head(&commit_oid) {
            // Lock state? Assume update_head doesn't affect index lock
            return Err(e);
        }

        // Index lock was committed by write_updates earlier.
        let elapsed = start_time.elapsed();
        println!("Merge completed in {:.2}s", elapsed.as_secs_f32());

        Ok(())
    }

    // --- Updated handle_fast_forward ---
    fn handle_fast_forward(
        database: &mut Database,
        workspace: &Workspace,
        index: &mut crate::core::index::index::Index, // Needs to be mutable
        refs: &Refs,
        _current_oid: &str, // Renamed to avoid confusion, not directly used for diff *target*
        target_oid: &str,
    ) -> Result<(), Error> {
        let a = &_current_oid[0..std::cmp::min(8, _current_oid.len())];
        let b = &target_oid[0..std::cmp::min(8, target_oid.len())];

        println!("Updating {}..{}", a, b);
        println!("Fast-forward");

        // Load the target commit to get its tree
        let target_commit_obj = database.load(target_oid)?;
        let target_commit = match target_commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => {
                index.rollback()?; // Release lock on error
                return Err(Error::Generic(format!("Target OID {} is not a commit", target_oid)));
            }
        };
        let target_tree_oid = target_commit.get_tree();
        println!("Target tree OID: {}", target_tree_oid);

        // Clear the current index completely
        index.clear(); // Use the internal clear method
        println!("Cleared existing index entries.");

        // Recursively read the target tree and update workspace and index
        match Self::read_tree_recursive(database, workspace, index, target_tree_oid, &PathBuf::new()) {
            Ok(_) => println!("Successfully updated workspace and index from target tree."),
            Err(e) => {
                index.rollback()?; // Release lock on error
                return Err(e);
            }
        }

        println!("Attempting to write index updates...");
        match index.write_updates() { // write_updates handles the changed flag internally
            Ok(updated) => {
                if updated {
                    println!("Index successfully written.");
                } else {
                    // This case should ideally not happen if read_tree_recursive added entries
                    println!("Warning: Index write reported no changes, but expected updates.");
                }
            },
            Err(e) => {
                println!("ERROR writing index updates: {}", e);
                // write_updates should manage its lock state on error (rollback)
                return Err(e);
            }
        }

        println!("Attempting to update HEAD to {}", target_oid);
        match refs.update_head(target_oid) {
            Ok(_) => println!("Successfully updated HEAD"),
            Err(e) => {
                println!("ERROR updating HEAD: {}", e);
                // The index is already written, HEAD update failed.
                return Err(e);
            }
        }

        println!("Fast-forward merge completed.");
        Ok(())
    }

    // --- New Recursive Helper for handle_fast_forward ---
    fn read_tree_recursive(
        database: &mut Database,
        workspace: &Workspace,
        index: &mut crate::core::index::index::Index,
        tree_oid: &str,
        prefix: &PathBuf,
    ) -> Result<(), Error> {
        let tree_obj = database.load(tree_oid)?;
        let tree = match tree_obj.as_any().downcast_ref::<Tree>() {
            Some(t) => t,
            None => return Err(Error::Generic(format!("Object {} is not a tree", tree_oid))),
        };

        println!("Processing tree {} at path '{}'", tree_oid, prefix.display());

        for (name, entry) in tree.get_entries() {
            let path = prefix.join(name);
            let path_str = path.to_string_lossy();
            println!("  Processing entry: {}", path_str);

            match entry {
                TreeEntry::Blob(oid, mode) => {
                    println!("    -> Blob: OID={}, Mode={}", oid, mode);
                    // Ensure parent directory exists
                    if let Some(parent) = path.parent() {
                        if !parent.as_os_str().is_empty() {
                           workspace.make_directory(parent)?;
                        }
                    }
                    // Read blob content
                    let blob_obj = database.load(oid)?;
                    let content = blob_obj.to_bytes();
                    // Write file to workspace
                    workspace.write_file(&path, &content)?;
                    // Get stats and add to index
                    let stat = workspace.stat_file(&path)?;
                    index.add(&path, oid, &stat)?;
                    println!("    -> Updated workspace and index for file: {}", path_str);
                },
                TreeEntry::Tree(subtree) => {
                    if let Some(subtree_oid) = subtree.get_oid() {
                         println!("    -> Subtree: OID={}", subtree_oid);
                         // Ensure directory exists in workspace
                         workspace.make_directory(&path)?;
                         // Recursively process the subtree
                         Self::read_tree_recursive(database, workspace, index, subtree_oid, &path)?;
                    } else {
                         println!("    -> Warning: Subtree entry '{}' has no OID", path_str);
                    }
                }
            }
        }
        Ok(())
    }


    fn write_tree_from_index(database: &mut Database, index: &mut crate::core::index::index::Index) -> Result<String, Error> {
        let database_entries: Vec<_> = index.each_entry()
            .filter(|entry| entry.stage == 0) // Only include stage 0 entries
            .map(|index_entry| {
                DatabaseEntry::new(
                    index_entry.get_path().to_string(),
                    index_entry.get_oid().to_string(),
                    &index_entry.mode_octal()
                )
            })
            .collect();

         if database_entries.is_empty() {
              // Handle the case of an empty index after resolving conflicts,
              // perhaps by returning the OID of an empty tree.
              // For now, let's return an error or a predefined empty tree OID.
              // Creating and storing an empty tree:
              let mut empty_tree = Tree::new();
              database.store(&mut empty_tree)?;
              return empty_tree.get_oid().cloned().ok_or_else(|| Error::Generic("Failed to get OID for empty tree".into()));
              // return Err(Error::Generic("Index is empty after merge, cannot write tree.".into()));
         }


        let mut root = crate::core::database::tree::Tree::build(database_entries.iter())?;
        root.traverse(|tree| database.store(tree).map(|_| ()))?;
        let tree_oid = root.get_oid()
            .ok_or(Error::Generic("Tree OID not set after storage".into()))?;
        Ok(tree_oid.clone())
    }
}