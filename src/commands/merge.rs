// src/commands/merge.rs
use std::time::Instant;
use std::env;
use std::path::Path;
use crate::errors::error::Error;
use crate::core::merge::inputs::Inputs;
use crate::core::merge::resolve::Resolve;
use crate::core::refs::Refs;
use crate::core::database::database::Database;
use crate::core::database::commit::Commit;
use crate::core::database::author::Author;
use crate::core::path_filter::PathFilter;
use crate::core::workspace::Workspace;
use crate::core::database::tree::Tree;
use crate::core::file_mode::FileMode;

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
            index.rollback()?;
            return Ok(());
        }

        if inputs.is_fast_forward() {
            // Handle fast-forward merge
            let result = Self::handle_fast_forward(
                &mut database,
                &workspace,
                &mut index,
                &refs,
                &inputs.left_oid,
                &inputs.right_oid
            );
            // handle_fast_forward should manage its own lock release/commit now
            return result;
        }

        // Perform a real merge
        let mut merge = Resolve::new(&mut database, &workspace, &mut index, &inputs);
        merge.on_progress = |info| println!("{}", info);

        // Execute the merge
        if let Err(e) = merge.execute() {
             index.rollback()?;
             return Err(e);
        }

        // Write the index updates (after clean changes are applied by Resolve)
        if let Err(e) = index.write_updates() {
            // Assuming write_updates handles its lock state on error
            return Err(e);
        }

        // Check for conflicts AFTER applying clean changes and writing index
        if index.has_conflict() {
            println!("\nAutomatic merge failed; fix conflicts and then commit the result.");
            // Keep index locked with conflicts
            return Err(Error::Generic("Automatic merge failed; fix conflicts and then commit the result.".into()));
        }

        // Commit the successful merge
        let commit_message = message.map(|s| s.to_string()).unwrap_or_else(|| {
            format!("Merge branch '{}'", revision)
        });

        // Create author
        let author = Author::new(
            env::var("GIT_AUTHOR_NAME").unwrap_or_else(|_| "A. U. Thor".to_string()),
            env::var("GIT_AUTHOR_EMAIL").unwrap_or_else(|_| "author@example.com".to_string()),
        );

        // Get tree from index (Index should be up-to-date now)
        let tree_oid = match Self::write_tree_from_index(&mut database, &mut index) {
            Ok(oid) => oid,
            Err(e) => {
                return Err(e);
            }
        };

        // Create commit with two parents
        let mut commit = Commit::new(
            Some(head_oid.clone()),
            tree_oid.clone(),
            author.clone(),
            commit_message,
        );

        // Add the second parent marker
        let special_message = format!("{}\n__PARENT2__:{}", commit.get_message(), inputs.right_oid);
        commit = Commit::new(
            Some(head_oid.clone()),
            tree_oid.clone(),
            author.clone(),
            special_message
        );

        // Store the commit
        if let Err(e) = database.store(&mut commit) {
             return Err(e);
        }

        // Update HEAD to the new commit
        let commit_oid = match commit.get_oid() {
            Some(oid) => oid.clone(),
             None => {
                 return Err(Error::Generic("Commit OID not set after storage".into()));
             }
        };

        if let Err(e) = refs.update_head(&commit_oid) {
            return Err(e);
        }

        // Lock was likely committed by write_updates or should be released if error occurred before that.
        let elapsed = start_time.elapsed();
        println!("Merge completed in {:.2}s", elapsed.as_secs_f32());

        Ok(())
    }

    // Handle fast-forward merge
    fn handle_fast_forward(
        database: &mut Database,
        workspace: &Workspace,
        index: &mut crate::core::index::index::Index, // Needs to be mutable
        refs: &Refs,
        current_oid: &str,
        target_oid: &str,
    ) -> Result<(), Error> {
        let a = &current_oid[0..std::cmp::min(8, current_oid.len())];
        let b = &target_oid[0..std::cmp::min(8, target_oid.len())];

        println!("Updating {}..{}", a, b);
        println!("Fast-forward");

        let path_filter = PathFilter::new();
        println!("Calculating tree diff between {} and {}", current_oid, target_oid);
        let tree_diff = database.tree_diff(Some(current_oid), Some(target_oid), &path_filter)?;
        println!("Tree diff calculated, {} entries found", tree_diff.len());

        for (path, (old_entry, new_entry)) in &tree_diff {
            println!("Processing diff entry: {}", path.display());
            if let Some(entry) = new_entry {
                println!("  -> New/Modified entry found: OID={}, Mode={}", entry.get_oid(), entry.get_mode());
                let mode = FileMode::parse(entry.get_mode());

                // Create parent directories first
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        workspace.make_directory(parent)?; // Use ? directly
                        println!("  -> Ensured parent directory exists: {}", parent.display());
                    }
                }

                if mode.is_directory() {
                    println!("  -> Entry is a directory. Creating directory: {}", path.display());
                    workspace.make_directory(path)?; // Use ? directly
                    println!("  -> Successfully created directory: {}", path.display());
                } else {
                    println!("  -> Entry is a file. Attempting to write file: {}", path.display());
                    let blob_obj = database.load(&entry.get_oid())?;
                    let content = blob_obj.to_bytes();
                    println!("  -> Blob loaded, size: {} bytes", content.len());

                    workspace.write_file(path, &content)?; // Use ? directly
                    println!("  -> Successfully wrote file to workspace: {}", path.display());

                    if let Ok(stat) = workspace.stat_file(path) {
                        index.add(path, &entry.get_oid(), &stat)?; // Use ? directly
                        println!("  -> Successfully updated index for: {}", path.display());
                    } else {
                        println!("  -> Warning: Could not stat file {} after writing.", path.display());
                         // Consider if this should be an error: return Err(...)
                    }
                }
            } else if let Some(entry) = old_entry {
                println!("  -> Deleting entry: {}", path.display());
                let old_mode = FileMode::parse(entry.get_mode());
                if old_mode.is_directory() {
                    println!("  -> Removing directory: {}", path.display());
                    workspace.force_remove_directory(path)?; // Use ? directly
                    println!("  -> Successfully removed directory: {}", path.display());
                } else {
                    println!("  -> Removing file: {}", path.display());
                    workspace.remove_file(path)?; // Use ? directly
                    println!("  -> Successfully removed file: {}", path.display());
                }

                let path_str = path.to_string_lossy().to_string();
                println!("  -> Removing '{}' from index...", path_str);
                index.remove(&path_str)?; // Use ? directly
                println!("  -> Removed '{}' from index", path_str);
            } else {
                println!("  -> Skipping entry (no new_entry and no old_entry): {}", path.display());
            }
        } // End of loop

        println!("Attempting to write index updates...");
        match index.write_updates() {
            Ok(updated) => println!("Index write_updates returned: {}", updated),
            Err(e) => {
                println!("ERROR writing index updates: {}", e);
                // If write_updates fails, the lock should ideally be released by it or rollback
                index.rollback()?; // Explicit rollback just in case
                return Err(e);
            }
        }

        println!("Attempting to update HEAD to {}", target_oid);
        match refs.update_head(target_oid) {
            Ok(_) => println!("Successfully updated HEAD"),
            Err(e) => {
                println!("ERROR updating HEAD: {}", e);
                // The index is already written, HEAD update failed. This leaves the repo
                // in a slightly inconsistent state (index matches target, HEAD doesn't).
                // Report the error. A more robust solution might try to revert index.
                return Err(e);
            }
        }

        println!("Fast-forward merge completed.");
        Ok(())
    }


    fn write_tree_from_index(database: &mut Database, index: &mut crate::core::index::index::Index) -> Result<String, Error> {
        let database_entries: Vec<_> = index.each_entry()
            .filter(|entry| entry.stage == 0)
            .map(|index_entry| {
                crate::core::database::entry::DatabaseEntry::new(
                    index_entry.get_path().to_string(),
                    index_entry.get_oid().to_string(),
                    &index_entry.mode_octal()
                )
            })
            .collect();

        let mut root = crate::core::database::tree::Tree::build(database_entries.iter())?;
        root.traverse(|tree| database.store(tree).map(|_| ()))?;
        let tree_oid = root.get_oid()
            .ok_or(Error::Generic("Tree OID not set after storage".into()))?;
        Ok(tree_oid.clone())
    }
}