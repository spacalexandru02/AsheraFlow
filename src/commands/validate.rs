use std::path::Path;
use crate::errors::error::Error;
use crate::tests::integrity_checker::IntegrityChecker;

pub struct ValidateCommand;

impl ValidateCommand {
    pub fn execute() -> Result<(), Error> {
        let root_path = Path::new(".");
        
        println!("\n=== ASH REPOSITORY VALIDATION ===\n");
        println!("Running comprehensive integrity check...");
        
        // Use the existing IntegrityChecker for thorough repository validation
        let mut checker = IntegrityChecker::new(root_path);
        match checker.check_repository() {
            Ok(report) => {
                println!("\n=== VALIDATION SUMMARY ===\n");
                
                println!("Repository path: {}", report.repo_path.display());
                println!("Repository structure: {}", if report.ash_dirs_valid { "Valid ✓" } else { "Invalid ✗" });
                println!("Index entries: {}", report.index_entries);
                println!("Index state: {}", if report.index_valid { "Valid ✓" } else { "Invalid ✗" });
                
                if let Some(commit) = &report.head_commit {
                    println!("HEAD commit: {}", commit);
                } else {
                    println!("HEAD commit: None (no commits yet)");
                }
                
                println!("\nObject statistics:");
                println!("  Total objects: {}", report.object_stats.total);
                println!("  Blobs: {}", report.object_stats.blobs);
                println!("  Trees: {}", report.object_stats.trees);
                println!("  Commits: {}", report.object_stats.commits);
                
                if report.object_stats.unknown > 0 {
                    println!("  Unknown objects: {} (!)", report.object_stats.unknown);
                }
                
                if !report.issues.is_empty() {
                    println!("\n!!! ISSUES DETECTED !!!");
                    for (i, issue) in report.issues.iter().enumerate() {
                        println!("{:3}. {}", i+1, issue);
                    }
                    
                    println!("\nRecommendations:");
                    if !report.ash_dirs_valid {
                        println!("- Repository structure is invalid. Consider re-initializing with 'ash init'.");
                    }
                    if !report.index_valid {
                        println!("- Index is corrupted. You may need to rebuild it by running 'ash add' on your files.");
                    }
                } else {
                    println!("\n✓ No issues found. Repository is in good health.");
                }
                
                if report.is_valid {
                    Ok(())
                } else {
                    Err(Error::Generic("Repository validation failed. See above for details.".into()))
                }
            },
            Err(e) => {
                println!("Error during validation: {}", e);
                Err(e)
            }
        }
    }
}