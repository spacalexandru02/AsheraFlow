use crate::core::database::entry::Entry;
use super::database::GitObject;
use crate::errors::error::Error;
use itertools::Itertools;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Tree {
    oid: Option<String>,
    entries: HashMap<String, TreeEntry>,
}

pub enum TreeEntry {
    Blob(String, u32), // oid, mode
    Tree(Box<Tree>),
}

// Constants for mode
pub const TREE_MODE: u32 = 0o040000;
pub const REGULAR_MODE: u32 = 0o100644;
pub const EXECUTABLE_MODE: u32 = 0o100755;

impl GitObject for Tree {
    fn get_type(&self) -> &str {
        "tree"
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::new();
        
        // Sort entries by name
        for (name, entry) in self.entries.iter().sorted_by_key(|(name, _)| *name) {
            match entry {
                TreeEntry::Blob(oid, mode) => {
                    // Format: "<mode> <name>\0<oid>"
                    let mode_str = format!("{:o}", mode);
                    let entry_str = format!("{} {}\0", mode_str, name);
                    result.extend_from_slice(entry_str.as_bytes());
                    
                    // Add binary OID (20 bytes)
                    if let Ok(oid_bytes) = hex::decode(oid) {
                        result.extend_from_slice(&oid_bytes);
                    }
                },
                TreeEntry::Tree(tree) => {
                    // Format: "<mode> <name>\0<oid>"
                    let mode_str = format!("{:o}", TREE_MODE);
                    let entry_str = format!("{} {}\0", mode_str, name);
                    result.extend_from_slice(entry_str.as_bytes());
                    
                    // Add binary OID (20 bytes)
                    if let Some(tree_oid) = &tree.oid {
                        if let Ok(oid_bytes) = hex::decode(tree_oid) {
                            result.extend_from_slice(&oid_bytes);
                        }
                    }
                }
            }
        }
        
        result
    }

    fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }
}

impl Tree {
    pub fn new() -> Self {
        Tree {
            oid: None,
            entries: HashMap::new(),
        }
    }
    
    pub fn build<'a, I>(entries: I) -> Result<Self, Error>
    where
        I: Iterator<Item = &'a Entry>,
    {
        let mut root = Tree::new();
        
        for entry in entries {
            root.add_entry(entry)?;
        }
        
        Ok(root)
    }
    
    pub fn add_entry(&mut self, entry: &Entry) -> Result<(), Error> {
        let parent_dirs = entry.parent_directories();
        let basename = entry.basename();
        
        // Ensure all parent directories exist
        let mut current = self;
        
        for dir in parent_dirs {
            let dir_name = dir.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            
            if dir_name.is_empty() {
                continue;
            }
            
            // Verificăm dacă avem nevoie să adăugăm un nou subdirector sau să folosim unul existent
            let entry_type = {
                if let Some(entry) = current.entries.get(&dir_name) {
                    match entry {
                        TreeEntry::Tree(_) => None, // E deja un arbore, e ok
                        _ => return Err(Error::Generic(format!(
                            "Entry '{}' conflicts with existing blob entry",
                            dir_name
                        ))),
                    }
                } else {
                    // Trebuie să adăugăm un nou subdirector
                    Some(TreeEntry::Tree(Box::new(Tree::new())))
                }
            };
            
            // Adăugăm subdirectorul dacă e necesar
            if let Some(tree_entry) = entry_type {
                current.entries.insert(dir_name.clone(), tree_entry);
            }
            
            // Obținem o referință la subdirector
            if let Some(TreeEntry::Tree(tree)) = current.entries.get_mut(&dir_name) {
                current = tree;
            } else {
                unreachable!("Subdirectory should exist at this point");
            }
        }
        
        // Add the file entry to the current tree
        current.entries.insert(
            basename,
            TreeEntry::Blob(entry.get_oid().to_string(), entry.get_mode().parse().unwrap_or(REGULAR_MODE))
        );
        
        Ok(())
    }
    
    pub fn traverse<F>(&mut self, mut func: F) -> Result<(), Error>
    where
        F: FnMut(&mut Tree) -> Result<(), Error>
    {
        // Process all subtrees first
        let mut sub_trees = Vec::new();
        
        // First collect references to all sub-trees
        for (name, entry) in &mut self.entries {
            if let TreeEntry::Tree(tree) = entry {
                sub_trees.push(tree);
            }
        }
        
        // Then process each sub-tree
        for tree in sub_trees {
            tree.traverse(&mut func)?;
        }
        
        // Then process this tree
        func(self)
    }

    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }
}