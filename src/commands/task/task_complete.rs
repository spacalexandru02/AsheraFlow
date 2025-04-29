use std::path::Path;
use std::time::Duration;
use crate::errors::error::Error;
use crate::core::sprint::sprint::SprintManager;
use crate::core::sprint::{TaskStatus, Task, Sprint};
use crate::commands::checkout::CheckoutCommand;
use crate::commands::merge::MergeCommand;
use crate::core::refs::{Refs, Reference};
use crate::core::commit_metadata::{TaskMetadata, CommitMetadataManager, TaskStatus as CommitTaskStatus};
use crate::core::branch_metadata::BranchMetadataManager;
use crate::core::repository::repository::Repository;

pub struct TaskCompleteCommand;

impl TaskCompleteCommand {
    pub fn execute(id: &str, auto_merge: bool) -> Result<(), Error> {
        // Initialize the repository path
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository: .ash directory not found".into()));
        }
        
        // Create branch metadata manager to check for sprints
        let branch_manager = BranchMetadataManager::new(root_path);
        
        // Initialize sprint manager to get sprints
        let sprint_manager = SprintManager::new(root_path);
        
        // Get all sprint branches
        let all_sprints = branch_manager.get_all_sprints()?;
        if all_sprints.is_empty() {
            return Err(Error::Generic("No sprints found. Start a sprint first with 'ash sprint start'.".into()));
        }
        
        // Verifică dacă există o referință pentru sprint-ul task-ului
        let repo = Repository::new(".")?;
        let sprint_ref = format!("refs/meta/tasksprint/{}", id);
        let original_sprint_branch = match repo.refs.read_ref(&sprint_ref) {
            Ok(Some(branch_name)) => {
                Some(branch_name)
            },
            _ => None,
        };
        
        // Try to find the task in any of the sprints
        let mut found_sprint: Option<Sprint> = None;
        let mut found_branch_name: Option<String> = None;
        
        // Dacă avem branch-ul original, încercăm mai întâi acel sprint
        if let Some(sprint_branch) = &original_sprint_branch {
            
            // Găsește metadatele pentru acest sprint
            for (branch_name, sprint_metadata) in &all_sprints {
                // Comparăm cu și fără prefixul "sprint-"
                let branch_match = branch_name == sprint_branch || 
                                  format!("sprint-{}", branch_name) == *sprint_branch;
                
                if branch_match {
                    if let Ok(tasks) = sprint_manager.get_sprint_tasks(branch_name) {
                        // Create a sprint object with the data we have
                        let (total_points, completed_points) = tasks.values().fold((0, 0), |(total, completed), task| {
                            let task_points = task.story_points.unwrap_or(0);
                            let completed_points = if task.status == TaskStatus::Done {
                                completed + task_points
                            } else {
                                completed
                            };
                            (total + task_points, completed_points)
                        });
                        
                        let sprint = Sprint {
                            name: sprint_metadata.name.clone(),
                            start_date: sprint_metadata.start_timestamp,
                            end_date: sprint_metadata.end_timestamp(),
                            tasks,
                            branch: branch_name.clone(),
                            total_story_points: total_points,
                            completed_story_points: completed_points,
                        };
                        
                        found_sprint = Some(sprint);
                        found_branch_name = Some(branch_name.clone());
                        break;
                    }
                }
            }
        }
        
        // Dacă încă nu am găsit, căutăm în toate sprint-urile
        if found_sprint.is_none() {
            for (branch_name, sprint_metadata) in &all_sprints {
                if let Ok(tasks) = sprint_manager.get_sprint_tasks(branch_name) {
                    if tasks.contains_key(id) {
                        // Found the sprint that contains this task
                        // Create a sprint object with the data we have
                        let (total_points, completed_points) = tasks.values().fold((0, 0), |(total, completed), task| {
                            let task_points = task.story_points.unwrap_or(0);
                            let completed_points = if task.status == TaskStatus::Done {
                                completed + task_points
                            } else {
                                completed
                            };
                            (total + task_points, completed_points)
                        });
                        
                        let sprint = Sprint {
                            name: sprint_metadata.name.clone(),
                            start_date: sprint_metadata.start_timestamp,
                            end_date: sprint_metadata.end_timestamp(),
                            tasks,
                            branch: branch_name.clone(),
                            total_story_points: total_points,
                            completed_story_points: completed_points,
                        };
                        
                        found_sprint = Some(sprint);
                        found_branch_name = Some(branch_name.clone());
                        break;
                    }
                }
            }
        }
        
        // If not found in any sprint, check the active sprint
        if found_sprint.is_none() {
            // Check if there's an active sprint
            if let Some((branch, metadata)) = branch_manager.find_active_sprint()? {
                // Try to load the active sprint
                if let Ok(tasks) = sprint_manager.get_sprint_tasks(&branch) {
                    // See if the task is in this sprint (by task ID)
                    // Create a sprint object with the data we have
                    let (total_points, completed_points) = tasks.values().fold((0, 0), |(total, completed), task| {
                        let task_points = task.story_points.unwrap_or(0);
                        let completed_points = if task.status == TaskStatus::Done {
                            completed + task_points
                        } else {
                            completed
                        };
                        (total + task_points, completed_points)
                    });
                    
                    let sprint = Sprint {
                        name: metadata.name.clone(),
                        start_date: metadata.start_timestamp,
                        end_date: metadata.end_timestamp(),
                        tasks,
                        branch: branch.clone(),
                        total_story_points: total_points,
                        completed_story_points: completed_points,
                    };
                    
                    found_sprint = Some(sprint);
                    found_branch_name = Some(branch);
                }
            }
        }
        
        // If still not found, check task metadata
        let mut task_metadata_manager = CommitMetadataManager::new(root_path);
        let task_metadata_option = task_metadata_manager.get_task_metadata(id)?;
        
        if found_sprint.is_none() && task_metadata_option.is_some() {
            // We have metadata but no sprint, try to find a sprint to assign it to
            if let Some((branch, metadata)) = branch_manager.find_active_sprint()? {
                if let Ok(tasks) = sprint_manager.get_sprint_tasks(&branch) {
                    // Create a sprint object with the data we have
                    let (total_points, completed_points) = tasks.values().fold((0, 0), |(total, completed), task| {
                        let task_points = task.story_points.unwrap_or(0);
                        let completed_points = if task.status == TaskStatus::Done {
                            completed + task_points
                        } else {
                            completed
                        };
                        (total + task_points, completed_points)
                    });
                    
                    let sprint = Sprint {
                        name: metadata.name.clone(),
                        start_date: metadata.start_timestamp,
                        end_date: metadata.end_timestamp(),
                        tasks,
                        branch: branch.clone(),
                        total_story_points: total_points,
                        completed_story_points: completed_points,
                    };
                    
                    found_sprint = Some(sprint);
                    found_branch_name = Some(branch);
                }
            } else if !all_sprints.is_empty() {
                // Use the last sprint
                let last_sprint = all_sprints.last().unwrap(); // Safe because we checked is_empty
                let branch_name = &last_sprint.0;
                let metadata = &last_sprint.1;
                
                if let Ok(tasks) = sprint_manager.get_sprint_tasks(branch_name) {
                    // Create a sprint object with the data we have
                    let (total_points, completed_points) = tasks.values().fold((0, 0), |(total, completed), task| {
                        let task_points = task.story_points.unwrap_or(0);
                        let completed_points = if task.status == TaskStatus::Done {
                            completed + task_points
                        } else {
                            completed
                        };
                        (total + task_points, completed_points)
                    });
                    
                    let sprint = Sprint {
                        name: metadata.name.clone(),
                        start_date: metadata.start_timestamp,
                        end_date: metadata.end_timestamp(),
                        tasks,
                        branch: branch_name.clone(),
                        total_story_points: total_points,
                        completed_story_points: completed_points,
                    };
                    
                    found_sprint = Some(sprint);
                    found_branch_name = Some(branch_name.clone());
                }
            }
        }
        
        // If we still don't have a sprint, error out
        let (mut current_sprint, branch_name) = match (found_sprint, found_branch_name) {
            (Some(sprint), Some(branch)) => (sprint, branch),
            _ => return Err(Error::Generic(format!("Task with ID {} not found in any sprint", id))),
        };
        
        // Get the task and check if it's in progress
        let mut task_in_sprint = current_sprint.tasks.contains_key(id);
        let task = if task_in_sprint {
            let task = current_sprint.tasks.get(id).unwrap(); // Safe because we just checked
            if task.status != TaskStatus::InProgress {
                if task.status == TaskStatus::Todo {
                    println!("Note: Task is not yet in progress. Automatically updating to InProgress before completing.");
                    // Mark it as in progress first (will update later)
                } else if task.status == TaskStatus::Done {
                    return Err(Error::Generic(format!("Task {} is already completed.", id)));
                }
            }
            task.clone()
        } else {
            // Create a new task from metadata if possible
            if let Some(task_metadata) = &task_metadata_option {
                let task = Task::new(
                    task_metadata.id.clone(),
                    task_metadata.description.clone(),
                    task_metadata.story_points
                );
                
                // Add to sprint
                current_sprint.add_task(task.clone())?;
                task_in_sprint = true;
                task
            } else {
                return Err(Error::Generic(format!("Task with ID {} not found in any sprint", id)));
            }
        };
        
        // Construct branch names
        let task_branch_name = format!("{}-task-{}", branch_name, id);
        
        // Verifică dacă avem o referință salvată pentru task branch
        let task_branch_ref = format!("refs/meta/taskbranch/{}", id);
        let task_branch = match repo.refs.read_ref(&task_branch_ref) {
            Ok(Some(branch)) => {
                branch
            },
            _ => task_branch_name,
        };
        
        // Check if we're on the correct branch
        let refs = Refs::new(&git_path);
        let current_ref = refs.current_ref()?;
        
        let current_branch = match current_ref {
            Reference::Symbolic(path) => refs.short_name(&path),
            _ => String::new(), // Detached HEAD state
        };
        
        // Verifică dacă branch-ul curent este un branch de task și determină sprintul corect
        let original_sprint_branch_from_current = if current_branch.contains("-task-") {
            let pattern = "sprint-sprint";
            if let Some(idx) = current_branch.find(pattern) {
                let sprint_part = &current_branch[idx + pattern.len()..];
                if let Some(task_idx) = sprint_part.find("-task-") {
                    let sprint_name = &sprint_part[..task_idx];
                    Some(format!("sprint-sprint{}", sprint_name))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        
        // Dacă am extras un sprint din branch-ul curent și nu am găsit un sprint original din referință,
        // utilizăm sprintul extras din branch-ul curent
        if original_sprint_branch.is_none() && original_sprint_branch_from_current.is_some() {
        }
        
        let correct_sprint_branch = original_sprint_branch
            .or(original_sprint_branch_from_current)
            .unwrap_or_else(|| format!("sprint-{}", branch_name));
        
        // Show an informational message if not on task branch
        if current_branch != task_branch {
            println!("Note: You are not on the task branch '{}'. Switching to it...", task_branch);
            // First try to checkout the task branch
            if let Err(e) = CheckoutCommand::execute(&task_branch) {
                println!("Couldn't switch to task branch: {}. Will complete from current branch.", e);
            }
        }
        
        // Complete the task
        // If task is in Todo status, update it to InProgress first
        if let Some(mut task_metadata) = task_metadata_option.clone() {
            if task_metadata.status == CommitTaskStatus::Todo {
                task_metadata.status = CommitTaskStatus::InProgress;
                task_metadata.started_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );
                task_metadata_manager.store_task_metadata(&task_metadata)?;
                
                // Also update task in current_sprint
                if let Some(task) = current_sprint.tasks.get_mut(id) {
                    task.status = TaskStatus::InProgress;
                    task.started_at = task_metadata.started_at;
                }
            }
        }
        
        // Mark task as complete
        current_sprint.complete_task(id)?;
        
        // Update task in task metadata system
        if let Some(mut task_metadata) = task_metadata_option {
            task_metadata.status = CommitTaskStatus::Done;
            task_metadata.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            );
            task_metadata_manager.store_task_metadata(&task_metadata)?;
        }
        
        // Get task duration
        let duration = match current_sprint.get_task_duration(id)? {
            Some(d) => d,
            None => Duration::from_secs(0),
        };
        
        // Get task metadata for formatting duration
        let task_metadata = task_metadata_manager.get_task_metadata(id)?.unwrap_or_else(|| {
            let mut tm = TaskMetadata::new(id.to_string(), "Unknown".to_string(), None);
            tm.status = CommitTaskStatus::Done;
            tm
        });
        
        // Save the updated sprint
        sprint_manager.save_sprint(&current_sprint)?;
        
        // Display task information
        println!("\nTask completed successfully:");
        println!("  ID: {}", id);
        println!("  Duration: {}", task_metadata.format_duration());
        
        // Display sprint progress
        println!("\nSprint progress:");
        println!("  Sprint: {}", current_sprint.name);
        println!("  Total Story Points: {}", current_sprint.total_story_points);
        println!("  Completed Story Points: {}", current_sprint.completed_story_points);
        println!("  Progress: {:.1}%", current_sprint.get_progress_percentage());
        
        // Always perform the merge (auto_merge is now ignored)
        println!("\nSwitching to sprint branch '{}'...", correct_sprint_branch);
        
        // Ensure branch name has the correct prefix - this is redundant now since we already handle this
        // when constructing correct_sprint_branch, but keeping it for safety
        let sprint_branch_name = if !correct_sprint_branch.starts_with("sprint-") {
            format!("sprint-{}", correct_sprint_branch)
        } else {
            correct_sprint_branch
        };
        
        CheckoutCommand::execute(&sprint_branch_name)?;
        
        // Only attempt merge if the task branch exists
        // Check if branch exists by trying to read it
        let branch_exists = refs.read_ref(&task_branch)?.is_some();
        
        if branch_exists {
            println!("Merging task branch '{}'...", task_branch);
            let merge_message = format!("Merge task/{} into {}", id, sprint_branch_name);
            
            match MergeCommand::execute(&task_branch, Some(&merge_message)) {
                Ok(_) => println!("Successfully merged task branch into sprint branch"),
                Err(e) => println!("Merge failed: {}. You may need to resolve conflicts and merge manually.", e),
            }
        } else {
            println!("Task branch '{}' does not exist, skipping merge.", task_branch);
        }
        
        Ok(())
    }
} 