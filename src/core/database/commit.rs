use super::{author::Author, database::GitObject};

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
        self.to_bytes()
    }

    fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
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

    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let timestamp = self.author.timestamp.timestamp();
        let author_line = format!(
            "{} <{}> {} +0000", 
            self.author.name, 
            self.author.email, 
            timestamp
        );
    
        let mut lines = Vec::with_capacity(5);
        
        lines.push(format!("tree {}", self.tree));
        lines.push(format!("author {}", author_line));
        lines.push(format!("committer {}", author_line));
    
        if let Some(parent) = &self.parent {
            lines.push(format!("parent {}", parent));
        }
    
        lines.push(String::new()); // Empty line before message
        lines.push(self.message.clone());
    
        lines.join("\n").into_bytes()
    }
}