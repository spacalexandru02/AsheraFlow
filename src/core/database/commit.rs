// src/core/database/commit.rs with clone_box implementation
use super::{author::Author, database::GitObject};
use crate::errors::error::Error;
use std::any::Any;
use std::str;
use std::collections::HashMap;

#[derive(Clone)]
pub struct Commit {
    oid: Option<String>,
    parent: Option<String>,
    tree: String,
    author: Author,
    message: String,
}

impl GitObject for Commit {
    fn get_type(&self) -> &str {
        "commit"
    }

    fn to_bytes(&self) -> Vec<u8> {
        let timestamp = self.author.timestamp.timestamp();
        let author_line = format!(
            "{} <{}> {} +0000", 
            self.author.name, 
            self.author.email, 
            timestamp
        );
    
        let mut lines = Vec::with_capacity(5);
        
        lines.push(format!("tree {}", self.tree));
        
        if let Some(parent) = &self.parent {
            lines.push(format!("parent {}", parent));
        }
        
        lines.push(format!("author {}", author_line));
        lines.push(format!("committer {}", author_line));
    
        lines.push(String::new()); // Empty line before message
        lines.push(self.message.clone());
    
        lines.join("\n").into_bytes()
    }

    fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    // Implementation of clone_box to properly clone the object
    fn clone_box(&self) -> Box<dyn GitObject> {
        Box::new(self.clone())
    }
}

impl Commit {
    pub fn new(parent: Option<String>, tree: String, author: Author, message: String) -> Self {
        Commit {
            oid: None,
            parent,
            tree,
            author,
            message,
        }
    }

    pub fn title_line(&self) -> String {
        self.message.lines().next().unwrap_or("").to_string()
    }
    
    // Ensure these methods are implemented
    pub fn get_parent(&self) -> Option<&String> {
        self.parent.as_ref()
    }
    
    pub fn get_author(&self) -> Option<&Author> {
        Some(&self.author)
    }
    
    pub fn get_message(&self) -> &str {
        &self.message
    }

    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }
    
    pub fn get_tree(&self) -> &str {
        &self.tree
    }
    
    /// Parsează un commit dintr-un șir de bytes
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let content = match str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return Err(Error::Generic("Invalid UTF-8 in commit".to_string())),
        };
        
        let mut lines = content.lines();
        let mut headers = HashMap::new();
        let mut message = String::new();
        let mut reading_message = false;
        
        // Parsează headerele până la linia goală
        while let Some(line) = lines.next() {
            if line.is_empty() {
                reading_message = true;
                continue;
            }
            
            if reading_message {
                if !message.is_empty() {
                    message.push('\n');
                }
                message.push_str(line);
                continue;
            }
            
            // Parsează headerul liniei curente
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() != 2 {
                return Err(Error::Generic(format!("Invalid commit header: {}", line)));
            }
            
            headers.insert(parts[0].to_string(), parts[1].to_string());
        }
        
        // Extrage tree, parent și author
        let tree = headers.get("tree")
            .ok_or_else(|| Error::Generic("Missing tree in commit".to_string()))?
            .clone();
        
        let parent = headers.get("parent").cloned();
        
        let author_str = headers.get("author")
            .ok_or_else(|| Error::Generic("Missing author in commit".to_string()))?;
        
        // Parsează autor - implementare simplificată
        let author = match Author::parse(author_str) {
            Ok(author) => author,
            Err(_) => return Err(Error::Generic("Invalid author format".to_string())),
        };

        Ok(Commit {
            oid: None,
            parent,
            tree,
            author,
            message,
        })
    }
}