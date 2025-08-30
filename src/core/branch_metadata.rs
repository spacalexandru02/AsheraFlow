use std::path::Path;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use crate::errors::error::Error;
use crate::core::refs::Refs;
use crate::core::database::database::Database;
use crate::core::repository::repository::Repository;
use crate::core::database::sprint_metadata_object::SprintMetadataObject;

/// Stores metadata for a sprint, including name, start time, and duration.
#[derive(Debug, Clone)]
pub struct SprintMetadata {
    pub name: String, 
    pub start_timestamp: u64,
    pub duration_days: u32,
}

impl SprintMetadata {
    /// Creates a new SprintMetadata instance with the current timestamp.
    pub fn new(name: String, duration_days: u32) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        SprintMetadata {
            name,
            start_timestamp: now,
            duration_days,
        }
    }

    /// Returns the end timestamp for the sprint.
    pub fn end_timestamp(&self) -> u64 {
        self.start_timestamp + (self.duration_days as u64 * 24 * 60 * 60)
    }

    /// Checks if the sprint is currently active.
    pub fn is_active(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        now <= self.end_timestamp()
    }

    /// Formats a timestamp as a human-readable date string.
    pub fn format_date(timestamp: u64) -> String {
        let dt = chrono::DateTime::from_timestamp(timestamp as i64, 0)
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
        
        dt.format("%Y-%m-%d %H:%M").to_string()
    }

    /// Converts sprint metadata to a branch name.
    pub fn to_branch_name(&self) -> String {
        format!("sprint-{}", self.name.replace(" ", "-").to_lowercase())
    }

    /// Encodes sprint metadata into a branch description string.
    pub fn encode(&self) -> String {
        format!("SPRINT:{}:{}:{}", self.name, self.start_timestamp, self.duration_days)
    }

    /// Decodes sprint metadata from a branch description string.
    pub fn decode(encoded: &str) -> Option<Self> {
        let parts: Vec<&str> = encoded.split(':').collect();
        if parts.len() >= 4 && parts[0] == "SPRINT" {
            let name = parts[1].to_string();
            let start_timestamp = parts[2].parse::<u64>().ok()?;
            let duration_days = parts[3].parse::<u32>().ok()?;

            Some(SprintMetadata {
                name,
                start_timestamp,
                duration_days,
            })
        } else {
            None
        }
    }
}

pub struct BranchMetadataManager {
    repo_path: std::path::PathBuf,
}

impl BranchMetadataManager {
    pub fn new(repo_path: &Path) -> Self {
        BranchMetadataManager {
            repo_path: repo_path.to_path_buf(),
        }
    }

    // Get the current branch name
    pub fn get_current_branch(&self) -> Result<String, Error> {
        // Initialize the repository path
        let git_path = self.repo_path.join(".ash");
        
        // Create a reference to the refs module
        let refs = crate::core::refs::Refs::new(&git_path);
        
        // Get current reference
        let current = refs.current_ref()?;
        
        match current {
            crate::core::refs::Reference::Symbolic(path) => {
                // Extract branch name from symbolic reference
                // Usually in the format "refs/heads/branch-name"
                if path.starts_with("refs/heads/") {
                    Ok(path.strip_prefix("refs/heads/")
                        .unwrap_or(&path)
                        .to_string())
                } else {
                    Ok(path)
                }
            },
            crate::core::refs::Reference::Direct(_) => {
                // Detached HEAD state
                Err(Error::Generic("HEAD is in a detached state".into()))
            }
        }
    }

    /// Store sprint metadata in the object database
    pub fn store_sprint_metadata(&self, branch_name: &str, metadata: &SprintMetadata) -> Result<(), Error> {
        // Create a repository and get access to database
        let repo_str = self.repo_path.to_str().unwrap_or(".");
        let mut repo = Repository::new(repo_str)?;
        
        // Convert metadata to string representation and then to object
        let encoded = metadata.encode();
        let mut obj = SprintMetadataObject::new(metadata.clone());
        
        // Store the object in database
        let oid = repo.database.store(&mut obj)?;
        
        // Update the reference to metadata
        // Use the branch name as is for consistency
        let meta_ref = format!("refs/meta/{}", branch_name);
        repo.refs.update_ref(&meta_ref, &oid)?;
        
        // Make sure sprint- prefixed branch also exists for task creation
        let sprint_branch_name = if branch_name.starts_with("sprint-") {
            branch_name.to_string()
        } else {
            format!("sprint-{}", branch_name)
        };
        
        // Create the sprint branch if it doesn't exist
        let head_oid = match repo.refs.read_head()? {
            Some(oid) => oid,
            None => return Err(Error::Generic("HEAD reference not found".into())),
        };
        
        // Try to create the branch (ignore error if branch already exists)
        match repo.refs.create_branch(&sprint_branch_name, &head_oid) {
            Ok(_) => {},
            Err(e) => {
                if e.to_string().contains("already exists") {
                    // Branch already exists, that's fine
                } else {
                    return Err(e);
                }
            }
        }
        
        Ok(())
    }

    /// Retrieve sprint metadata from the object database
    pub fn get_sprint_metadata(&self, branch_name: &str) -> Result<Option<SprintMetadata>, Error> {
        // Create a repository and get access to database
        let repo_str = self.repo_path.to_str().unwrap_or(".");
        let mut repo = Repository::new(repo_str)?;
        
        // Use the branch name directly without additional modifications
        // to maintain consistency with what we initially stored
        let meta_ref = format!("refs/meta/{}", branch_name);
        
        // Read the reference for metadata
        let oid = match repo.refs.read_ref(&meta_ref)? {
            Some(oid) => {
                oid
            },
            None => {
                // Try also with the alternative format for compatibility
                let alt_meta_ref = format!("refs/meta/sprint-{}", branch_name);
                match repo.refs.read_ref(&alt_meta_ref)? {
                    Some(oid) => {
                        oid
                    },
                    None => {
                        return Ok(None);
                    },
                }
            },
        };
        
        // Load the object from database
        match repo.database.load(&oid) {
            Ok(obj) => {
                if let Some(meta_obj) = obj.as_any().downcast_ref::<SprintMetadataObject>() {
                    Ok(Some(meta_obj.get_metadata().clone()))
                } else {
                    Err(Error::Generic("Invalid metadata object type".into()))
                }
            },
            Err(e) => {
                Err(e)
            }
        }
    }

    /// Find the current active sprint
    pub fn find_active_sprint(&self) -> Result<Option<(String, SprintMetadata)>, Error> {
        // First check the current branch
        let current_branch = self.get_current_branch()?;
        
        // If the current branch is a sprint branch (format: sprint-*), first check this sprint
        if current_branch.starts_with("sprint-") && !current_branch.contains("-task-") {
            // Extract the sprint name from the branch (without the "sprint-" prefix)
            let sprint_name = current_branch.strip_prefix("sprint-").unwrap_or(&current_branch);
            
            // Check metadata and if the sprint is active
            if let Ok(Some(metadata)) = self.get_sprint_metadata(sprint_name) {
                if metadata.is_active() {
                    return Ok(Some((sprint_name.to_string(), metadata)));
                }
            }
        }
        
        // If the current branch didn't provide an active sprint, search through all references
        
        // Initialize repository and database for additional checks
        let repo_str = self.repo_path.to_str().unwrap_or(".");
        let mut repo = Repository::new(repo_str)?;
        let db_path = self.repo_path.join(".ash").join("objects");
        let mut database = Database::new(db_path);
        
        // Check references from repository
        let git_path = self.repo_path.join(".ash");
        
        // Check all refs to find sprint metadata
        let refs = match repo.refs.list_refs_with_prefix("refs/meta/") {
            Ok(refs) => {
                refs
            }
            Err(e) => {
                Vec::new()
            }
        };
        
        // Check all refs directly from the filesystem
        let meta_refs_path = git_path.join("refs/meta/");
        let meta_refs = match std::fs::read_dir(meta_refs_path) {
            Ok(entries) => {
                let refs: Vec<_> = entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path().to_string_lossy().to_string())
                    .collect();
                refs
            }
            Err(e) => {
                Vec::new()
            }
        };
        
        // Check if sprint-meta exists
        let sprint_meta_path = git_path.join("refs/meta/sprint-meta");
        let sprint_meta_content = match std::fs::read_to_string(sprint_meta_path) {
            Ok(content) => {
                content
            }
            Err(e) => {
                String::new()
            }
        };
        
        // Get current timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Find all sprint references (refs/meta/sprint-*)
        let sprint_refs: Vec<_> = repo.refs.list_refs_with_prefix("refs/meta/sprint-")?
            .into_iter()
            .filter(|r| {
                match r {
                    crate::core::refs::Reference::Symbolic(path) => path.starts_with("refs/meta/sprint-"),
                    _ => false,
                }
            })
            .collect();
        
        // Check each sprint reference
        for sprint_ref in sprint_refs {
            match sprint_ref {
                crate::core::refs::Reference::Symbolic(path) => {
                    // Extract branch name from sprint-*
                    let branch_name = path.strip_prefix("refs/meta/sprint-")
                        .unwrap_or(&path)
                        .to_string();
                    
                    // Check if the sprint is active
                    if let Ok(Some(metadata)) = self.get_sprint_metadata(&branch_name) {
                        if metadata.is_active() {
                            return Ok(Some((branch_name, metadata)));
                        }
                    }
                },
                _ => {
                    continue;
                },
            }
        }
        
        Ok(None)
    }

    // Get all sprints
    pub fn get_all_sprints(&self) -> Result<Vec<(String, SprintMetadata)>, Error> {
        // Create a repository and get access to database
        let repo_str = self.repo_path.to_str().unwrap_or(".");
        let repo = Repository::new(repo_str)?;
        
        let mut results = Vec::new();
        
        // Get all meta/sprint-* references
        let refs = repo.refs.list_refs_with_prefix("refs/meta/sprint-")?;
            
        for reference in refs {
            match reference {
                crate::core::refs::Reference::Symbolic(path) => {
                    let branch_name = path.strip_prefix("refs/meta/sprint-")
                        .unwrap_or(&path)
                        .to_string();
                    
                    if let Ok(Some(metadata)) = self.get_sprint_metadata(&branch_name) {
                        results.push((branch_name, metadata));
                    }
                },
                _ => continue,
            }
        }
        
        // Sort by start date, newest first
        results.sort_by(|a, b| b.1.start_timestamp.cmp(&a.1.start_timestamp));
        
        Ok(results)
    }

    // Get all sprint branches from the repository
    pub fn get_all_sprint_branches(&self) -> Result<Vec<String>, Error> {
        // Create repository to access refs
        let repo_str = self.repo_path.to_str().unwrap_or(".");
        let repo = Repository::new(repo_str)?;
        
        let mut results = Vec::new();
        
        // Get all references to branches (refs/heads/*)
        let refs = repo.refs.list_refs_with_prefix("refs/heads/")?;
        
        // Filter to only include sprint branches (sprint-*)
        for reference in refs {
            match reference {
                crate::core::refs::Reference::Symbolic(path) => {
                    let branch_name = path.strip_prefix("refs/heads/")
                        .unwrap_or(&path)
                        .to_string();
                    
                    // Only include branches starting with sprint-
                    if branch_name.starts_with("sprint-") && !branch_name.contains("-task-") {
                        results.push(branch_name);
                    }
                },
                _ => continue,
            }
        }
        
        Ok(results)
    }
} 