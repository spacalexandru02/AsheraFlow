use std::path::Path;
use chrono::NaiveDate;

use crate::errors::error::Error;
use crate::commands::branch::BranchCommand;
use crate::commands::checkout::CheckoutCommand;
use crate::core::branch_metadata::{SprintMetadata, BranchMetadataManager};

/// Handles advanced sprint creation with custom start and end dates in AsheraFlow.
pub struct SprintAdvanceCommand;

impl SprintAdvanceCommand {
    /// Creates a new sprint with a custom start and end date.
    /// Validates date formats, checks for overlapping sprints, and manages sprint branch creation.
    pub fn execute(name: &str, start_date: &str, end_date: &str) -> Result<(), Error> {
        println!("Creating advanced sprint: {} (from {} to {})", name, start_date, end_date);
        
        // Initialize the repository path
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository: .ash directory not found".into()));
        }
        
        // Parse the dates
        let start = match NaiveDate::parse_from_str(start_date, "%Y-%m-%d") {
            Ok(date) => date,
            Err(_) => return Err(Error::Generic("Invalid start date format. Use YYYY-MM-DD".into())),
        };
        
        let end = match NaiveDate::parse_from_str(end_date, "%Y-%m-%d") {
            Ok(date) => date,
            Err(_) => return Err(Error::Generic("Invalid end date format. Use YYYY-MM-DD".into())),
        };
        
        // Validate dates (end date should be after start date)
        if end <= start {
            return Err(Error::Generic("End date must be after start date".into()));
        }
        
        // Calculate duration in days
        let duration_days = (end.signed_duration_since(start).num_seconds() / 86400) as u32;
        
        // Convert start date to timestamp (seconds since epoch)
        let start_timestamp = match start.and_hms_opt(0, 0, 0) {
            Some(dt) => dt.timestamp() as u64,
            None => return Err(Error::Generic("Failed to convert start date to timestamp".into())),
        };
        
        // Create branch metadata manager
        let branch_manager = BranchMetadataManager::new(root_path);
        
        // Check if there's already an active sprint (doar pentru avertizare, nu pentru blocare)
        if let Some((active_branch, active_meta)) = branch_manager.find_active_sprint()? {
            println!("Warning: There is already an active sprint '{}' that will end on {}.", 
                active_meta.name, 
                SprintMetadata::format_date(active_meta.end_timestamp()));
            println!("You can still create a future sprint that will start on {}.", 
                SprintMetadata::format_date(start_timestamp));
            
            // Verificăm dacă noul sprint se suprapune cu cel activ
            if start_timestamp < active_meta.end_timestamp() {
                println!("Warning: The new sprint will overlap with the active sprint!");
            }
        }
        
        // Create a new sprint metadata with the specified start date
        let mut sprint_metadata = SprintMetadata::new(name.to_string(), duration_days);
        sprint_metadata.start_timestamp = start_timestamp;  // Override the default start time
        
        let branch_name = sprint_metadata.to_branch_name();
        
        // Format dates for display
        let formatted_start_date = SprintMetadata::format_date(sprint_metadata.start_timestamp);
        let formatted_end_date = SprintMetadata::format_date(sprint_metadata.end_timestamp());
        
        // Display sprint information
        println!("Sprint information:");
        println!("  Name: {}", sprint_metadata.name);
        println!("  Start date: {}", formatted_start_date);
        println!("  End date: {}", formatted_end_date);
        println!("  Duration: {} days", duration_days);
        println!("  Branch: {}", branch_name);
        
        // Create the sprint branch
        println!("Creating sprint branch: {}", branch_name);
        
        // Create branch using BranchCommand
        match BranchCommand::execute(&branch_name, None) {
            Ok(_) => {},
            Err(e) => {
                // Skip error if branch already exists
                if !e.to_string().contains("already exists") {
                    return Err(e);
                }
                println!("Branch already exists, using existing branch.");
            }
        }
        
        // Checkout the branch
        println!("Checking out sprint branch...");
        CheckoutCommand::execute(&branch_name)?;
        println!("Successfully switched to branch '{}'", branch_name);
        
        // Store the sprint metadata
        branch_manager.store_sprint_metadata(&branch_name, &sprint_metadata)?;
        
        println!("\nSprint '{}' created successfully!", name);
        println!("You can now create tasks with: ash task create <id> <description> [story_points]");
        println!("The sprint will start on: {} and end on: {}", formatted_start_date, formatted_end_date);
        
        Ok(())
    }
} 