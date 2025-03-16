// Actualizare pentru src/core/database/tree.rs
use crate::core::database::entry::Entry;
use crate::core::file_mode::FileMode;
use super::database::GitObject;
use crate::errors::error::Error;
use itertools::Itertools;
use std::collections::HashMap;
use std::any::Any;

#[derive(Debug)]
pub struct Tree {
    oid: Option<String>,
    entries: HashMap<String, TreeEntry>,
}


#[derive(Debug)]
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
        
        for (name, entry) in self.entries.iter().sorted_by_key(|(name, _)| *name) {
            match entry {
                TreeEntry::Blob(oid, mode) => {
                    // Asigură-te că modul este reprezentat corect
                    let mode_str = FileMode::to_octal_string(*mode);
                    let entry_str = format!("{} {}\0", mode_str, name);
                    // ... restul codului
                },
                TreeEntry::Tree(tree) => {
                    // Asigură-te că modul pentru directoare este corect
                    let mode_str = FileMode::to_octal_string(FileMode::DIRECTORY);
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
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Tree {
    pub fn new() -> Self {
        Tree {
            oid: None,
            entries: HashMap::new(),
        }
    }
    
    pub fn get_entries(&self) -> &HashMap<String, TreeEntry> {
        &self.entries
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
    
    // În tree.rs:
    pub fn traverse<F>(&mut self, mut func: F) -> Result<(), Error>
    where
        F: FnMut(&mut Tree) -> Result<(), Error>
    {
        self.traverse_internal(&mut func)
    }
    
    fn traverse_internal<F>(&mut self, func: &mut F) -> Result<(), Error>
    where
        F: FnMut(&mut Tree) -> Result<(), Error>
    {
        // Procesează mai întâi sub-arborii
        let mut subtrees: Vec<&mut Box<Tree>> = Vec::new();
        
        for (_, entry) in &mut self.entries {
            if let TreeEntry::Tree(tree) = entry {
                subtrees.push(tree);
            }
        }
        
        // Procesează fiecare sub-arbore
        for tree in subtrees {
            tree.traverse_internal(func)?;
        }
        
        // Aplică funcția pe acest arbore
        func(self)
    }

    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }
    
    /// Parsează un tree dintr-un șir de bytes
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut tree = Tree::new();
        let mut pos = 0;
        
        while pos < data.len() {
            // Găsește primul spațiu pentru a obține modul
            let mode_end = data[pos..].iter().position(|&b| b == b' ')
                .ok_or_else(|| Error::Generic("Invalid tree format: missing space after mode".to_string()))?;
            
            // Parsează modul ca octal
            let mode_str = std::str::from_utf8(&data[pos..pos+mode_end])
                .map_err(|_| Error::Generic("Invalid UTF-8 in mode".to_string()))?;
        
            let mode = FileMode::parse(mode_str);
            
            pos += mode_end + 1;
            
            // Găsește nul terminator pentru nume
            let name_end = data[pos..].iter().position(|&b| b == 0)
                .ok_or_else(|| Error::Generic("Invalid tree format: missing null terminator after name".to_string()))?;
            
            // Extrage numele
            let name = std::str::from_utf8(&data[pos..pos+name_end])
                .map_err(|_| Error::Generic("Invalid UTF-8 in name".to_string()))?;
            
            pos += name_end + 1;
            
            // Asigură-te că avem suficiente bytes pentru OID (20)
            if pos + 20 > data.len() {
                return Err(Error::Generic("Invalid tree format: truncated SHA-1".to_string()));
            }
            
            // Extrage OID-ul ca hex string
            let oid = hex::encode(&data[pos..pos+20]);
            pos += 20;
            
            // Adaugă intrarea în tree
            if mode == TREE_MODE {
                // Dacă modul este pentru un tree, vom avea nevoie de funcționalitate 
                // pentru a încărca recursiv trees - pentru simplificare, adăugăm doar OID-ul
                let mut subtree = Tree::new();
                subtree.set_oid(oid.clone());
                tree.entries.insert(name.to_string(), TreeEntry::Tree(Box::new(subtree)));
            } else {
                // Pentru blob-uri, adăugăm direct OID-ul și modul
                tree.entries.insert(name.to_string(), TreeEntry::Blob(oid, mode));
            }
        }
        
        Ok(tree)
    }

    pub fn insert_entry(&mut self, name: String, entry: TreeEntry) {
        self.entries.insert(name, entry);
    }
    
    // Dacă e nevoie și de o metodă pentru a obține o intrare după nume
    pub fn get_entry(&self, name: &str) -> Option<&TreeEntry> {
        self.entries.get(name)
    }
    
    // Dacă e nevoie și de acces mutabil la o intrare
    pub fn get_entry_mut(&mut self, name: &str) -> Option<&mut TreeEntry> {
        self.entries.get_mut(name)
    }
}