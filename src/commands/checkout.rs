use std::time::Instant;
use crate::errors::error::Error;
use crate::core::revision::Revision;
use crate::core::repository::repository::Repository;

pub struct CheckoutCommand;

impl CheckoutCommand {
    pub fn execute(target: &str) -> Result<(), Error> {
        let start_time = Instant::now();
        
        // Initialize repository
        let mut repo = Repository::new(".")?;
        
        // Read the current HEAD
        let current_oid = match repo.refs.read_head()? {
            Some(oid) => Some(oid),
            None => None,
        };
        
        // Resolve the target revision to a commit ID
        let mut revision = Revision::new(&mut repo, target);
        let target_oid = match revision.resolve("commit") {
            Ok(oid) => oid,
            Err(e) => {
                // Handle invalid revision
                for err in revision.errors {
                    eprintln!("error: {}", err.message);
                    for hint in &err.hint {
                        eprintln!("hint: {}", hint);
                    }
                }
                return Err(e);
            }
        };
        
        // Create a tree diff between current and target commits
        let tree_diff = repo.tree_diff(current_oid.as_deref(), Some(&target_oid))?;
        
        // Load the index for update
        repo.index.load_for_update()?;
        
        // Create and apply migration
        let mut migration = repo.migration(tree_diff);
        
        match migration.apply_changes() {
            Ok(_) => {
                // Migration succeeded, write index updates
                repo.index.write_updates()?;
                
                // Update HEAD to point to the new target
                repo.refs.update_head(&target_oid)?;
                
                let elapsed = start_time.elapsed();
                println!("Switched to commit {}", &target_oid[0..8]);
                println!("Checkout completed in {:.2}s", elapsed.as_secs_f32());
                
                Ok(())
            },
            Err(_e) => {
                // Migration failed
                // Clone the errors first before releasing the lock to avoid borrow conflicts
                let errors = migration.errors.clone();
                
                // Release index lock
                repo.index.rollback()?;
                
                // Print all error messages without referencing migration
                for message in errors {
                    eprintln!("error: {}", message);
                }
                
                eprintln!("Aborting");
                
                Err(Error::Generic("Checkout failed due to conflicts".to_string()))
            }
        }
    }
}