use std::fs;
use std::path::Path;
use crate::core::workspace::Workspace;
use crate::core::database::database::{Database, GitObject};
use crate::core::database::blob::Blob;
use crate::errors::error::Error;
use super::myers;

/// Splits a string into lines
pub fn split_lines(content: &str) -> Vec<String> {
    content.lines().map(|s| s.to_string()).collect()
}

/// Reads a file and splits its content into lines
pub fn read_file_lines(path: &Path) -> Result<Vec<String>, std::io::Error> {
    let content = fs::read_to_string(path)?;
    Ok(split_lines(&content))
}

/// Compares two files and returns a formatted diff
pub fn diff_files(file1_path: &Path, file2_path: &Path, context_lines: usize) -> Result<String, Error> {
    let a_lines = read_file_lines(file1_path).map_err(|e| Error::IO(e))?;
    let b_lines = read_file_lines(file2_path).map_err(|e| Error::IO(e))?;
    
    let edits = myers::diff_lines(&a_lines, &b_lines);
    let diff = myers::format_diff(&a_lines, &b_lines, &edits, context_lines);
    
    Ok(diff)
}

/// Compares a file with its version in the database
pub fn diff_with_database(
    workspace: &Workspace, 
    database: &mut Database,  // Changed to mutable reference
    file_path: &Path, 
    oid: &str,
    context_lines: usize
) -> Result<String, Error> {
    // Read the working copy
    let working_content = workspace.read_file(file_path)?;
    let working_lines = split_lines(&String::from_utf8_lossy(&working_content));
    
    // Read the version from the database
    let blob_obj = database.load(oid)?;
    let blob = match blob_obj.as_any().downcast_ref::<Blob>() {
        Some(b) => b,
        None => return Err(Error::Generic(format!("Object {} is not a blob", oid))),
    };
    
    let db_content = blob.to_bytes();
    let db_lines = split_lines(&String::from_utf8_lossy(&db_content));
    
    // Calculate the diff
    let edits = myers::diff_lines(&db_lines, &working_lines);
    let diff = myers::format_diff(&db_lines, &working_lines, &edits, context_lines);
    
    Ok(diff)
}