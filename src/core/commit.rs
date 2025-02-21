use crate::core::author::Author;
use crate::errors::error::Error;
use std::fmt;

use super::database::GitObject;

pub struct Commit {
    oid: Option<String>,
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
    pub fn new(tree: String, author: Author, message: String) -> Self {
        Commit {
            oid: None,
            tree,
            author,
            message,
        }
    }

    pub fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }

    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }

    pub fn get_type(&self) -> &str {
        "commit"
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let lines = vec![
            format!("tree {}", self.tree),
            format!("author {}", self.author),
            format!("committer {}", self.author),
            String::new(),
            self.message.clone(),
        ];
        lines.join("\n").into_bytes()
    }
}