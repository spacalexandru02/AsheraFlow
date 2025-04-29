use std::path::Path;

use crate::errors::error::Error;
use crate::core::sprint::{SprintManager, Task, Sprint};
use crate::core::commit_metadata::{TaskMetadata, TaskStatus, CommitMetadataManager};
use crate::core::branch_metadata::BranchMetadataManager;
use crate::commands::branch::BranchCommand;
use crate::commands::checkout::CheckoutCommand;

pub struct TaskCreateCommand;

impl TaskCreateCommand {
    pub fn execute(id: &str, description: &str, story_points: Option<u32>) -> Result<(), Error> {
        // Initialize the repository path
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository: .ash directory not found".into()));
        }
        
        // Create branch metadata manager to check for active sprint
        let branch_manager = BranchMetadataManager::new(root_path);
        
        // Debug: verificăm branch-ul curent
        println!("[DEBUG] Verificare branch curent...");
        match branch_manager.get_current_branch() {
            Ok(branch) => println!("[DEBUG] Branch curent: {}", branch),
            Err(e) => println!("[DEBUG] Eroare la obținerea branch-ului curent: {:?}", e),
        }
        
        // Debug: verificăm toate referințele din repo
        println!("[DEBUG] Verificare referințe din repo...");
        let repo = crate::core::repository::repository::Repository::new(".")?;
        let refs = repo.refs.list_refs_with_prefix("refs/meta/")?;
        println!("[DEBUG] Referințe meta găsite: {:?}", refs);
        
        // Debug: verificăm toate sprinturile
        println!("[DEBUG] Verificare toate sprinturile...");
        match branch_manager.get_all_sprints() {
            Ok(sprints) => {
                println!("[DEBUG] Sprinturi găsite: {}", sprints.len());
                for (branch, metadata) in sprints {
                    println!("[DEBUG]   - Sprint: '{}', Activ: {}", branch, metadata.is_active());
                }
            },
            Err(e) => println!("[DEBUG] Eroare la obținerea sprinturilor: {:?}", e),
        }
        
        // Check if there's an active sprint
        println!("[DEBUG] Căutare sprint activ...");
        let (sprint_branch, sprint_metadata) = match branch_manager.find_active_sprint()? {
            Some((branch, metadata)) => {
                println!("[DEBUG] Sprint activ găsit: '{}', '{}', Activ: {}", branch, metadata.name, metadata.is_active());
                (branch, metadata)
            },
            None => {
                println!("[DEBUG] Nu s-a găsit niciun sprint activ!");
                
                // Add more debugging to check for metadata objects in database
                println!("[DEBUG] Verificare obiecte existente în directory-ul .ash/objects/");
                let output = std::process::Command::new("find")
                    .arg(".ash/objects/")
                    .arg("-type")
                    .arg("f")
                    .arg("-not")
                    .arg("-path")
                    .arg("*.idx")
                    .output()
                    .expect("Failed to execute find command");
                
                println!("[DEBUG] Obiecte găsite: {}", String::from_utf8_lossy(&output.stdout));
                
                // Check refs directory
                println!("[DEBUG] Verificare referințe în .ash/refs/meta/");
                let output = std::process::Command::new("find")
                    .arg(".ash/refs/meta/")
                    .arg("-type")
                    .arg("f")
                    .output()
                    .expect("Failed to execute find command");
                
                println!("[DEBUG] Referințe meta găsite: {}", String::from_utf8_lossy(&output.stdout));
                
                return Err(Error::Generic("No active sprint found. Start a sprint first with 'ash sprint start'.".into()));
            },
        };
        
        // Get current branch (for information only, we no longer restrict based on it)
        let current_branch = branch_manager.get_current_branch()?;
        println!("[DEBUG] Current branch: {}, Sprint branch: {}", current_branch, sprint_branch);
        
        // Prioritizăm branch-ul curent pentru a determina sprintul în care creăm taskul
        // Dacă branch-ul curent este un branch de sprint (de forma "sprint-*"), îl folosim pe acesta
        // SAU dacă branch-ul curent este un branch de task (de forma "sprint-sprint*-task-*"), 
        // extragem sprint-ul din numele task-ului
        let (actual_sprint_branch, actual_sprint_metadata) = if current_branch.starts_with("sprint-") {
            if !current_branch.contains("-task-") {
                // Este un branch de sprint (sprint-sprintX)
                let extracted_sprint_name = current_branch.strip_prefix("sprint-").unwrap_or(&current_branch);
                
                // Verificăm dacă avem metadate pentru acest sprint
                match branch_manager.get_sprint_metadata(extracted_sprint_name)? {
                    Some(metadata) => {
                        println!("[DEBUG] Using current branch to determine sprint: {} -> {}", current_branch, extracted_sprint_name);
                        (extracted_sprint_name.to_string(), metadata)
                    },
                    None => {
                        println!("[DEBUG] Branch appears to be a sprint branch but no metadata found, falling back to active sprint");
                        (sprint_branch, sprint_metadata)
                    }
                }
            } else {
                // Este un branch de task (sprint-sprintX-task-taskY)
                // Extragem numele sprint-ului din branch-ul curent de task
                let pattern = "sprint-sprint";
                if let Some(idx) = current_branch.find(pattern) {
                    let sprint_part = &current_branch[idx + pattern.len()..];
                    if let Some(task_idx) = sprint_part.find("-task-") {
                        let extracted_sprint_name = &sprint_part[..task_idx];
                        
                        println!("[DEBUG] Extracted sprint from task branch: {} -> sprint{}", current_branch, extracted_sprint_name);
                        
                        // Verificăm dacă avem metadate pentru acest sprint
                        match branch_manager.get_sprint_metadata(&format!("sprint{}", extracted_sprint_name))? {
                            Some(metadata) => {
                                println!("[DEBUG] Using task branch to determine sprint: {} -> sprint{}", current_branch, extracted_sprint_name);
                                (format!("sprint{}", extracted_sprint_name), metadata)
                            },
                            None => {
                                println!("[DEBUG] Could not find metadata for sprint extracted from task branch, falling back to active sprint");
                                (sprint_branch, sprint_metadata)
                            }
                        }
                    } else {
                        println!("[DEBUG] Task branch format not recognized, falling back to active sprint");
                        (sprint_branch, sprint_metadata)
                    }
                } else {
                    println!("[DEBUG] Could not extract sprint from task branch, falling back to active sprint");
                    (sprint_branch, sprint_metadata)
                }
            }
        } else {
            // Folosim sprintul activ determinat anterior
            println!("[DEBUG] Current branch is not a sprint or task branch, using active sprint");
            (sprint_branch, sprint_metadata)
        };
        
        // Compute the expected sprint branch name (prefixed with "sprint-")
        let expected_sprint_branch = format!("sprint-{}", actual_sprint_branch);
        println!("Creating task in sprint: {}", actual_sprint_metadata.name);
        
        // Validate ID (alphanumeric with dashes and underscores)
        if !id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(Error::Generic("Task ID must contain only alphanumeric characters, dashes, or underscores.".into()));
        }
        
        // Initialize sprint manager for accessing sprint data
        let sprint_manager = SprintManager::new(root_path);
        
        // Try to get current sprint data 
        let mut current_sprint = match sprint_manager.get_current_sprint()? {
            Some(sprint) => sprint,
            None => {
                // Create a new Sprint if it doesn't exist yet
                let mut new_sprint = Sprint::new(
                    actual_sprint_metadata.name.clone(),
                    actual_sprint_metadata.duration_days
                );
                new_sprint.branch = actual_sprint_branch.clone();
                new_sprint
            }
        };
        
        // Make sure the sprint branch is set correctly
        if current_sprint.branch != actual_sprint_branch {
            current_sprint.branch = actual_sprint_branch.clone();
        }
        
        // Check if task already exists in sprint
        if current_sprint.tasks.contains_key(id) {
            return Err(Error::Generic(format!("Task with ID '{}' already exists in this sprint.", id)));
        }
        
        // let's now create the task
        let mut task = Task::new(
            id.to_owned(),
            description.to_string(),
            story_points,
        );
        
        // Set the task to InProgress directly instead of Todo
        task.status = crate::core::sprint::TaskStatus::InProgress;
        
        // Set the started_at timestamp
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        task.started_at = Some(current_time);
        
        // Create a new branch for the task based on sprint branch
        let task_branch_name = format!("{}-task-{}", expected_sprint_branch, id);
        
        // Debug: list branches using the simpler list_branches method
        println!("[DEBUG] Verificare dacă branch-ul de sprint '{}' există...", expected_sprint_branch);
        let branches = match repo.refs.list_branches() {
            Ok(branches) => branches,
            Err(e) => {
                println!("[DEBUG] Eroare la listarea branch-urilor: {:?}", e);
                vec![] // Empty vector on error
            }
        };
        println!("[DEBUG] Branch-uri disponibile: {:?}", branches);
        
        // Create the task branch based on the current branch if expected_sprint_branch doesn't exist
        let start_point = if branches.iter().any(|branch| {
            match branch {
                crate::core::refs::Reference::Direct(name) => name == &expected_sprint_branch,
                crate::core::refs::Reference::Symbolic(name) => name == &expected_sprint_branch,
            }
        }) {
            println!("[DEBUG] Branch-ul de sprint {} există, folosim ca punct de start", expected_sprint_branch);
            expected_sprint_branch.clone()
        } else {
            println!("[DEBUG] AVERTISMENT: Branch-ul de sprint {} nu există, folosim branch-ul curent {}", expected_sprint_branch, current_branch);
            current_branch.clone()
        };
        
        // Create the task branch
        println!("Creating task branch: {}", task_branch_name);
        match BranchCommand::execute(&task_branch_name, Some(&start_point)) {
            Ok(_) => {},
            Err(e) => {
                // Skip error if branch already exists
                if !e.to_string().contains("already exists") {
                    return Err(e);
                }
                println!("Branch already exists, using existing branch.");
            }
        }
        
        // Add task to sprint
        current_sprint.add_task(task.clone())?;
        
        // Save updated sprint
        sprint_manager.save_sprint(&current_sprint)?;
        
        // Create and store task metadata
        let task_manager = CommitMetadataManager::new(root_path);
        let task_metadata = TaskMetadata {
            id: id.to_string(),
            description: description.to_string(),
            story_points,
            status: TaskStatus::InProgress,
            created_at: task.created_at,
            started_at: Some(current_time),
            completed_at: None,
            commit_ids: Vec::new(),
        };
        
        // Store task metadata
        task_manager.store_task_metadata(&task_metadata)?;
        
        // Store the branch reference for this task
        let meta_ref = format!("refs/meta/taskbranch/{}", id);
        let repo = crate::core::repository::repository::Repository::new(".")?;
        repo.refs.update_ref(&meta_ref, &task_branch_name)?;
        
        // Also store which sprint this task belongs to
        let sprint_ref = format!("refs/meta/tasksprint/{}", id);
        repo.refs.update_ref(&sprint_ref, &expected_sprint_branch)?;
        
        // Display task information
        println!("Task created and started successfully:");
        println!("  ID: {}", task.id);
        println!("  Description: {}", task.description);
        println!("  Status: InProgress");
        println!("  Branch: {}", task_branch_name);
        
        if let Some(points) = task.story_points {
            println!("  Story Points: {}", points);
        } else {
            println!("  Story Points: None");
        }
        
        // Display sprint information
        println!("\nSprint progress:");
        println!("  Total Story Points: {}", current_sprint.total_story_points);
        println!("  Completed Story Points: {}", current_sprint.completed_story_points);
        println!("  Progress: {:.1}%", current_sprint.get_progress_percentage());
        
        // Switch to the task branch automatically instead of asking
        println!("\nSwitching to task branch: {}", task_branch_name);
        CheckoutCommand::execute(&task_branch_name)?;
        println!("Successfully switched to branch '{}'", task_branch_name);
        
        Ok(())
    }
} 