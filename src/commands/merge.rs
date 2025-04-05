// src/commands/merge.rs
use std::time::Instant;
use std::env;
use std::path::{Path, PathBuf};
use std::collections::HashMap;

use crate::errors::error::Error;
use crate::core::merge::inputs::Inputs;
use crate::core::merge::resolve::Resolve;
use crate::core::refs::Refs;
use crate::core::database::database::Database;
use crate::core::database::commit::Commit;
use crate::core::database::author::Author;
use crate::core::path_filter::PathFilter;
use crate::core::workspace::Workspace;

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
        index.load_for_update()?;
        
        // Check for conflicts or pending operations
        if index.has_conflict() {
            return Err(Error::Generic("Cannot merge with conflicts. Fix conflicts and commit first.".into()));
        }
        
        // Get the current HEAD
        let head_oid = match refs.read_head()? {
            Some(oid) => oid,
            None => return Err(Error::Generic("No HEAD commit found. Create an initial commit first.".into())),
        };
        
        // Parse merge inputs
        let inputs = Inputs::new(&mut database, &refs, "HEAD".to_string(), revision.to_string())?;
        
        // Check for already merged or fast-forward cases
        if inputs.already_merged() {
            println!("Already up to date.");
            return Ok(());
        }
        
        if inputs.is_fast_forward() {
            // Handle fast-forward merge
            return Self::handle_fast_forward(
                &mut database, 
                &workspace, 
                &mut index, 
                &refs, 
                &inputs.left_oid, 
                &inputs.right_oid
            );
        }
        
        // Perform a real merge
        let mut merge = Resolve::new(&mut database, &workspace, &mut index, &inputs);
        merge.on_progress = |info| println!("{}", info);
        
        // Execute the merge
        merge.execute()?;
        
        // Write the index updates
        index.write_updates()?;
        
        // Check for conflicts
        if index.has_conflict() {
            println!("\nAutomatic merge failed; fix conflicts and then commit the result.");
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
        
        // Get tree from index
        let tree_oid = Self::write_tree_from_index(&mut database, &mut index)?;
        
        // Create commit with two parents
        let mut commit = Commit::new(
            Some(head_oid.clone()),
            tree_oid,
            author,
            commit_message,
        );
        
        // Add the second parent - the branch we're merging in
        // Hack: For now, since our Commit doesn't support multiple parents directly, 
        // we'll store them in the message with a special marker
        let special_message = format!("{}\n__PARENT2__:{}", commit.get_message().to_string(), inputs.right_oid);
        if let Some(parent) = commit.get_parent() {
            // Store the second parent information in a way that our code can interpret
            // In a real implementation, we would modify the Commit struct to support multiple parents
            commit = Commit::new(
                Some(parent.to_string()),
                tree_oid,
                author.clone(),
                special_message
            );
        }
        
        // Store the commit
        database.store(&mut commit)?;
        
        // Update HEAD to the new commit
        let commit_oid = commit.get_oid().unwrap().clone();
        refs.update_head(&commit_oid)?;
        
        let elapsed = start_time.elapsed();
        println!("Merge completed in {:.2}s", elapsed.as_secs_f32());
        
        Ok(())
    }
    
    // Handle fast-forward merge
    fn handle_fast_forward(
        database: &mut Database,
        workspace: &Workspace,
        index: &mut crate::core::index::index::Index,
        refs: &Refs,
        current_oid: &str,
        target_oid: &str,
    ) -> Result<(), Error> {
        // Log fast-forward status
        let a = &current_oid[0..8];  // Use first 8 chars as short OID
        let b = &target_oid[0..8];   // Use first 8 chars as short OID
        
        println!("Updating {}..{}", a, b);
        println!("Fast-forward");
        
        // Create a PathFilter for the tree_diff call
        let path_filter = PathFilter::new();
        
        // Get tree diff between current and target
        let tree_diff = database.tree_diff(Some(current_oid), Some(target_oid), &path_filter)?;
        
        // Apply changes to workspace and index
        for (path, (old_entry, new_entry)) in &tree_diff {
            if let Some(entry) = new_entry {
                // File being added or modified
                let blob_obj = database.load(&entry.get_oid())?;
                let content = blob_obj.to_bytes();
                
                // Create any necessary parent directories
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        workspace.make_directory(parent)?;
                    }
                }
                
                // Write file to workspace
                workspace.write_file(path, &content)?;
                
                // Update index
                if let Ok(stat) = workspace.stat_file(path) {
                    index.add(path, &entry.get_oid(), &stat)?;
                }
            } else if old_entry.is_some() {
                // File being deleted
                workspace.remove_file(path)?;
                
                // Remove from index
                let path_str = path.to_string_lossy().to_string();
                index.remove(&path_str)?;
            }
        }
        
        // Write updates to index
        index.write_updates()?;
        
        // Update HEAD to the target
        refs.update_head(target_oid)?;
        
        Ok(())
    }
    
    // Function to write tree from current index state
    fn write_tree_from_index(database: &mut Database, index: &mut crate::core::index::index::Index) -> Result<String, Error> {
        // Convert index entries to database entries
        let database_entries: Vec<_> = index.each_entry()
            .filter(|entry| entry.stage == 0) // Only include regular entries, not conflict entries
            .map(|index_entry| {
                crate::core::database::entry::DatabaseEntry::new(
                    index_entry.get_path().to_string(),
                    index_entry.get_oid().to_string(),
                    &index_entry.mode_octal()
                )
            })
            .collect();
        
        // Build tree from index entries
        let mut root = crate::core::database::tree::Tree::build(database_entries.iter())?;
        
        // Store all trees in the database
        root.traverse(|tree| database.store(tree))?;
        
        // Get the root tree OID
        let tree_oid = root.get_oid()
            .ok_or(Error::Generic("Tree OID not set after storage".into()))?;
            
        Ok(tree_oid.clone())
    }
}