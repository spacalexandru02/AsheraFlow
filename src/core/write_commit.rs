// src/core/write_commit.rs
use std::path::{Path, PathBuf};
use crate::core::color::Color;
use crate::core::database::commit::Commit;
use crate::core::database::database::Database;
use crate::core::editor::Editor;
use crate::core::index::index::Index;
use crate::core::refs::Refs;
use crate::errors::error::Error;
use std::env;

pub struct WriteCommit<'a> {
    pub database: &'a mut Database,
    pub index: &'a mut Index,
    pub refs: &'a Refs,
    pub root_path: &'a Path,
    pub options: &'a WriteCommitOptions,
}

pub struct WriteCommitOptions {
    pub message: Option<String>,
    pub file: Option<PathBuf>,
    pub edit: EditOption,
}

pub enum EditOption {
    Auto,
    Always,
    Never,
}

impl<'a> WriteCommit<'a> {
    pub fn new(
        database: &'a mut Database,
        index: &'a mut Index,
        refs: &'a Refs,
        root_path: &'a Path,
        options: &'a WriteCommitOptions,
    ) -> Self {
        WriteCommit {
            database,
            index,
            refs,
            root_path,
            options,
        }
    }

    pub fn commit_message_path(&self) -> PathBuf {
        self.root_path.join(".ash").join("COMMIT_EDITMSG")
    }

    pub fn read_message(&self) -> Result<Option<String>, Error> {
        // Check if message is directly provided
        if let Some(message) = &self.options.message {
            return Ok(Some(format!("{}\n", message)));
        }
        
        // Check if file is provided
        if let Some(file_path) = &self.options.file {
            match std::fs::read_to_string(file_path) {
                Ok(content) => return Ok(Some(content)),
                Err(e) => return Err(Error::IO(e)),
            }
        }
        
        // No message provided
        Ok(None)
    }
    
    pub fn compose_message(&self, message: Option<String>, notes: &str) -> Result<Option<String>, Error> {
        let should_edit = match (message.as_ref(), &self.options.edit) {
            (_, EditOption::Always) => true,
            (Some(_), EditOption::Auto) => false,
            (Some(_), EditOption::Never) => false,
            (None, _) => true,
        };
        
        if !should_edit && message.is_some() {
            return Ok(message);
        }
        
        let commit_message_path = self.commit_message_path();
        let editor_command = Editor::get_editor_command();
        
        Editor::edit(commit_message_path, Some(editor_command), |editor| {
            // Add initial message if provided
            if let Some(msg) = &message {
                editor.puts(msg)?;
                editor.puts("")?;
            }
            
            // Add comment instructions
            editor.note(notes)?;
            
            // Don't open editor if we don't need to edit
            if !should_edit {
                editor.close();
            }
            
            Ok(())
        })
    }

    pub fn print_commit(&self, commit: &Commit) {
        let ref_info = self.refs.current_ref().unwrap_or_else(|_| crate::core::refs::Reference::Direct("".to_string()));
        
        let info = match ref_info {
            crate::core::refs::Reference::Direct(_) => "detached HEAD".to_string(),
            crate::core::refs::Reference::Symbolic(path) => self.refs.short_name(&path),
        };
        
        // Create a longer-lived empty string if needed
        let empty_oid = "".to_string();
        let oid = commit.get_oid().unwrap_or(&empty_oid);
        let short_oid = &oid[0..std::cmp::min(7, oid.len())];
        
        let mut display_info = info;
        if commit.get_parent().is_none() {
            display_info.push_str(" (root-commit)");
        }
        display_info.push_str(&format!(" {}", short_oid));
        
        println!("[{}] {}", Color::green(&display_info), commit.title_line());
    }
    
    pub fn write_tree(&mut self) -> Result<String, Error> {
        // Convert index entries to database entries
        let database_entries: Vec<crate::core::database::entry::DatabaseEntry> = self.index.each_entry()
            .filter(|entry| entry.stage == 0)
            .map(|index_entry| {
                crate::core::database::entry::DatabaseEntry::new(
                    index_entry.path.clone(),
                    index_entry.oid.clone(),
                    &index_entry.mode_octal()
                )
            })
            .collect();
        
        // Build the tree
        let mut root = crate::core::database::tree::Tree::build(database_entries.iter())?;
        
        // Store all trees
        root.traverse(|tree| self.database.store(tree).map(|_| ()))?;
        
        // Get the root tree OID
        let tree_oid = root.get_oid()
            .ok_or(Error::Generic("Tree OID not set after storage".into()))?
            .clone();
        
        Ok(tree_oid)
    }
    
    pub fn create_commit(
        &mut self, 
        parents: Vec<String>, 
        message: String
    ) -> Result<Commit, Error> {
        let tree_oid = self.write_tree()?;
        
        // Get author information
        let name = env::var("GIT_AUTHOR_NAME")
            .or_else(|_| env::var("USER"))
            .unwrap_or_else(|_| "Unknown".to_string());
            
        let email = env::var("GIT_AUTHOR_EMAIL")
            .unwrap_or_else(|_| format!("{}@localhost", name));
            
        let author = crate::core::database::author::Author::new(name, email);
        
        // Create the commit
        let parent = if parents.is_empty() { None } else { Some(parents[0].clone()) };
        let mut commit = Commit::new(parent, tree_oid, author, message);
        
        // Store the commit
        self.database.store(&mut commit)?;
        
        // Return the commit
        Ok(commit)
    }
}

// Function to parse command-line options for use with WriteCommit
pub fn define_write_commit_options() -> WriteCommitOptions {
    let mut options = WriteCommitOptions {
        message: None,
        file: None,
        edit: EditOption::Auto,
    };
    
    // Check environment variables (would normally parse from command line args)
    if let Ok(message) = env::var("ASH_COMMIT_MESSAGE") {
        options.message = Some(message);
    }
    
    if let Ok(file) = env::var("ASH_COMMIT_FILE") {
        options.file = Some(PathBuf::from(file));
    }
    
    if let Ok(edit) = env::var("ASH_EDIT") {
        options.edit = match edit.as_str() {
            "1" => EditOption::Always,
            "0" => EditOption::Never,
            _ => options.edit,
        };
    }
    
    options
}