use std::path::{Path, PathBuf};
use crate::core::database::blob::Blob;
use crate::core::database::database::Database;
use crate::core::index::index::Index;
use crate::core::workspace::Workspace;
use crate::errors::error::Error;

pub struct AddCommand;

impl AddCommand {
    pub fn execute(paths: &[String]) -> Result<(), Error> {
        if paths.is_empty() {
            return Err(Error::Generic("No paths specified for add command".into()));
        }

        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        let workspace = Workspace::new(root_path);
        let mut database = Database::new(git_path.join("objects"));
        let mut index = Index::new(git_path.join("index"));
        
        // Load existing index first
        if !index.load_for_update()? {
            return Err(Error::Generic("Could not acquire lock on index".into()));
        }
        
        let mut added_count = 0;
    
    for path_str in paths {
        let path = PathBuf::from(path_str);
        println!("Processing path: {:?}", path);
        
        // Verifică dacă calea există
        let exists = if path.is_absolute() {
            path.exists()
        } else {
            workspace.path_exists(&path)?
        };
        
        if !exists {
            println!("warning: '{}' did not match any files", path_str);
            continue;
        }
        
        // Obține lista de fișiere (recursiv pentru directoare)
        let file_paths = workspace.list_files_from(&path)?;
        println!("Found {} files to add", file_paths.len());
            
        for file_path in &file_paths {
            println!("Adding file: {:?}", file_path);
            
            // Citește conținutul fișierului
            let data = workspace.read_file(file_path)?;
            
            // Obține metadatele fișierului
            let stat = workspace.stat_file(file_path)?;
            
            // Creează și stochează blob-ul
            let mut blob = Blob::new(data);
            database.store(&mut blob)?;
            
            // Obține OID-ul (hash-ul SHA-1)
            let oid = blob.get_oid()
                .ok_or_else(|| Error::Generic("Blob OID not set after storage".into()))?;
            
            // Adaugă în index
            index.add(file_path, oid, &stat)?;
            added_count += 1;
            }
        }
        
        // Write index updates
        if added_count > 0 {
            match index.write_updates()? {
                true => {
                    if added_count == 1 {
                        println!("Added 1 file to index");
                    } else {
                        println!("Added {} files to index", added_count);
                    }
                    
                    // Rulează verificarea completă de integritate
                    match database.verify_repository_integrity(&root_path) {
                        Ok(_) => println!("Verificarea integrității a trecut cu succes."),
                        Err(e) => println!("⚠️ Verificarea integrității a eșuat: {}", e)
                    }
                },
                false => return Err(Error::Generic("Failed to update index".into())),
            }
        }
        
        Ok(())
    }
}