use std::time::Instant;

use crate::errors::error::Error;
use crate::core::merge::inputs::Inputs;
use crate::core::merge::resolve::Resolve;
use crate::core::refs::Refs;
use crate::core::database::database::Database;
use crate::core::database::commit::Commit;
use crate::core::database::author::Author;

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
        let root_path = std::path::Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }
        
        let workspace = crate::core::workspace::Workspace::new(root_path);
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
            return Err(Error::Generic("Already up to date.".into()));
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
            format!("Merge branch '{}'", inputs.right_name)
        });
        
        // Create author
        let author = Author::new(
            String::from("A. U. Thor"), // Replace with configured user name
            String::from("author@example.com"), // Replace with configured email
        );
        
        // Create commit
        let mut commit = Commit::new(
            Some(inputs.left_oid.clone()),
            "tree_placeholder".to_string(), // Will be calculated during store
            author.clone(),
            commit_message,
        );
        
        // Add the second parent - the branch we're merging in
        // Note: This assumes your Commit struct supports multiple parents
        if let Some(parent_ref) = commit.get_parent_mut() {
            let mut parents = vec![parent_ref.clone(), inputs.right_oid.clone()];
            *parent_ref = parents.remove(0);
            // Add the additional parent - this may need to be adjusted based on your Commit implementation
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
    
    fn handle_fast_forward(
        database: &mut Database,
        workspace: &crate::core::workspace::Workspace,
        index: &mut crate::core::index::index::Index,
        refs: &Refs,
        current_oid: &str,
        target_oid: &str,
    ) -> Result<(), Error> {
        // Log fast-forward status
        let a = Database::short_oid(current_oid);
        let b = Database::short_oid(target_oid);
        
        println!("Updating {}..{}", a, b);
        println!("Fast-forward");
        
        // Get tree diff between current and target
        let tree_diff = database.tree_diff(Some(current_oid), Some(target_oid), None)?;
        
        // Create migration to apply changes
        let mut migration = crate::core::repository::migration::Migration::new(database, tree_diff);
        migration.apply_changes()?;
        
        // Write updates to index
        index.write_updates()?;
        
        // Update HEAD to the target
        refs.update_head(target_oid)?;
        
        Ok(())
    }
}