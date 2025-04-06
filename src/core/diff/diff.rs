use std::path::Path;
use crate::core::workspace::Workspace;
use crate::core::database::database::{Database, GitObject};
use crate::core::database::blob::Blob;
use crate::errors::error::Error;
use crate::core::color::Color;
use super::myers;

/// Dimensiunea maximă a unui fișier pentru diff (pentru a evita probleme de performanță)
const MAX_DIFF_SIZE: usize = 10 * 1024 * 1024; // 10 MB

/// Împarte un șir în linii
pub fn split_lines(content: &str) -> Vec<String> {
    content.lines().map(|s| s.to_string()).collect()
}

/// Compară un fișier cu versiunea sa din baza de date
pub fn diff_with_database(
    workspace: &Workspace, 
    database: &mut Database,
    file_path: &Path, 
    oid: &str,
    context_lines: usize
) -> Result<String, Error> {
    // Citește copia de lucru
    let working_content = workspace.read_file(file_path)?;
    
    // Citește versiunea din baza de date
    let blob_obj = database.load(oid)?;
    let blob = match blob_obj.as_any().downcast_ref::<Blob>() {
        Some(b) => b,
        None => return Err(Error::Generic(format!("Object {} is not a blob", oid))),
    };
    
    let db_content = blob.to_bytes();
    
    // Verifică dacă conținutul este binar
    let working_is_binary = myers::is_binary_content(&working_content);
    let db_is_binary = myers::is_binary_content(&db_content);
    
    if working_is_binary || db_is_binary {
        return Ok(format!("Binary files differ"));
    }
    
    // Verifică dacă fișierele sunt prea mari pentru diff
    if working_content.len() > MAX_DIFF_SIZE || db_content.len() > MAX_DIFF_SIZE {
        return Ok(format!("File too large for diff: maximum size is {} bytes", MAX_DIFF_SIZE));
    }
    
    // Calculează hash-ul pentru conținutul fișierului de lucru
    let working_hash = database.hash_file_data(&working_content);
    
    // Convertește conținutul în text și calculează diff-ul
    let diff_content = match (String::from_utf8(working_content.to_vec()), String::from_utf8(db_content.to_vec())) {
        (Ok(working_text), Ok(db_text)) => {
            // Ambele sunt UTF-8 valid
            let working_lines = split_lines(&working_text);
            let db_lines = split_lines(&db_text);
            
            let edits = myers::diff_lines(&db_lines, &working_lines);
            myers::format_diff(&db_lines, &working_lines, &edits, context_lines)
        },
        _ => {
            // Cel puțin unul dintre fișiere nu este UTF-8 valid
            // Le tratăm ca text non-UTF-8, folosind from_utf8_lossy
            let working_text = String::from_utf8_lossy(&working_content);
            let db_text = String::from_utf8_lossy(&db_content);
            
            let working_lines = split_lines(&working_text);
            let db_lines = split_lines(&db_text);
            
            let edits = myers::diff_lines(&db_lines, &working_lines);
            myers::format_diff(&db_lines, &working_lines, &edits, context_lines)
        }
    };
    
    // Verifică dacă diff-ul este gol (fișierele sunt identice)
    if diff_content.trim().is_empty() {
        return Ok(format!("Files are identical"));
    }
    
    // Adaugă antetul git-style cu informații despre index
    let path_str = file_path.to_string_lossy();
    
    // Generează hash-uri scurte pentru a simula formatul git
    let db_hash_short = if oid.len() >= 7 { &oid[0..7] } else { oid };
    let working_hash_short = if working_hash.len() >= 7 { &working_hash[0..7] } else { &working_hash };
    
    // Creează antetul în stil git
    let mut result = String::new();
    result.push_str(&format!("index {}..{} 100644\n", db_hash_short, working_hash_short));
    result.push_str(&format!("--- a/{}\n", path_str));
    result.push_str(&format!("+++ b/{}\n", path_str));
    result.push_str(&diff_content);
    
    Ok(result)
}