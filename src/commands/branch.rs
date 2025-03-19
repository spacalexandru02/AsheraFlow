// src/commands/branch.rs
use std::path::Path;
use std::time::Instant;
use crate::errors::error::Error;
use crate::core::revision::Revision;
use crate::core::repository::repository::Repository;

pub struct BranchCommand;

impl BranchCommand {
    pub fn execute(branch_name: &str, start_point: Option<&str>) -> Result<(), Error> {
        let start_time = Instant::now();
        
        // Check branch name is not empty
        if branch_name.is_empty() {
            return Err(Error::Generic("Branch name is required".to_string()));
        }
        
        // Initialize repository
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Check if repository exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }
        
        // Create repository object
        let mut repo = Repository::new(".")?;
        
        // Determine the start point (commit OID)
        let start_oid = if let Some(revision_expr) = start_point {
            // Resolve the revision to a commit OID
            let mut revision = Revision::new(&mut repo, revision_expr);
            match revision.resolve("commit") {
                Ok(oid) => oid,
                Err(e) => {
                    // Print any additional error information collected during resolution
                    for err in revision.errors {
                        eprintln!("error: {}", err.message);
                        for hint in &err.hint {
                            eprintln!("hint: {}", hint);
                        }
                    }
                    return Err(e);
                }
            }
        } else {
            // Use HEAD as the default start point
            repo.refs.read_head()?.ok_or_else(|| {
                Error::Generic("Failed to resolve HEAD - repository may be empty".to_string())
            })?
        };
        
        // Create the branch
        match repo.refs.create_branch(branch_name, &start_oid) {
            Ok(_) => {
                println!("Created branch '{}' at {}", branch_name, &start_oid[0..8]);
                
                let elapsed = start_time.elapsed();
                println!("Branch command completed in {:.2}s", elapsed.as_secs_f32());
                
                Ok(())
            },
            Err(e) => Err(e)
        }
    }
}