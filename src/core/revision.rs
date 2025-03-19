// src/core/revision.rs
use std::collections::HashMap;
use regex::Regex;
use crate::errors::error::Error;
use crate::core::repository::repository::Repository;
use crate::core::database::database::Database;
use crate::core::database::commit::Commit;

// Define the revision node types for AST representation
enum RevisionNode {
    Ref(String),
    Parent(Box<RevisionNode>),
    Ancestor(Box<RevisionNode>, usize),
}

// Structure to hold errors with hints
pub struct HintedError {
    pub message: String,
    pub hint: Vec<String>,
}

// Main Revision class
pub struct Revision<'a> {
    repo: &'a mut Repository,
    expr: String,
    query: Option<RevisionNode>,
    pub errors: Vec<HintedError>,
}

impl<'a> Revision<'a> {
    pub fn new(repo: &'a mut Repository, expression: &str) -> Self {
        let expr = expression.to_string();
        let query = Self::parse(&expr);
        
        Revision {
            repo,
            expr,
            query,
            errors: Vec::new(),
        }
    }
    
    // Parse a revision string into a RevisionNode
    fn parse(revision: &str) -> Option<RevisionNode> {
        // Regex patterns for revision operators
        lazy_static::lazy_static! {
            static ref PARENT_PATTERN: Regex = Regex::new(r"^(.+)\^$").unwrap();
            static ref ANCESTOR_PATTERN: Regex = Regex::new(r"^(.+)~(\d+)$").unwrap();
            static ref INVALID_NAME: Regex = Regex::new(r"(?x)
                ^\.|
                /\.|
                \.\.|
                ^/|
                /$|
                \.lock$|
                @\{|
                [\x00-\x20*:?\[\\\^~\x7f]
            ").unwrap();
            
            static ref REF_ALIASES: HashMap<&'static str, &'static str> = {
                let mut m = HashMap::new();
                m.insert("@", "HEAD");
                m
            };
        }
        
        // Check for parent notation (rev^)
        if let Some(captures) = PARENT_PATTERN.captures(revision) {
            let rev = captures.get(1).unwrap().as_str();
            return Self::parse(rev).map(|node| RevisionNode::Parent(Box::new(node)));
        }
        
        // Check for ancestor notation (rev~N)
        if let Some(captures) = ANCESTOR_PATTERN.captures(revision) {
            let rev = captures.get(1).unwrap().as_str();
            let n = captures.get(2).unwrap().as_str().parse::<usize>().unwrap_or(1);
            
            return Self::parse(rev).map(|node| RevisionNode::Ancestor(Box::new(node), n));
        }
        
        // Check if it's a valid reference name
        if !INVALID_NAME.is_match(revision) {
            let name = REF_ALIASES.get(revision).unwrap_or(&revision);
            return Some(RevisionNode::Ref(name.to_string()));
        }
        
        None
    }
    
    // Resolve a revision to an object ID
    pub fn resolve(&mut self, expected_type: &str) -> Result<String, Error> {
        let query_clone = self.query.clone();
        if let Some(node) = query_clone {
            // Resolve the AST to an object ID
            match self.resolve_node(&node) {
                Ok(oid) => {
                    // Verify the object type if specified
                    if self.verify_object_type(&oid, expected_type)? {
                        Ok(oid)
                    } else {
                        Err(Error::Generic(format!("Not a valid object name: '{}'", self.expr)))
                    }
                },
                Err(_) => Err(Error::Generic(format!("Not a valid object name: '{}'", self.expr))),
            }
        } else {
            Err(Error::Generic(format!("Not a valid object name: '{}'", self.expr)))
        }
    }
    
    // Resolve a node in the AST to an object ID
    fn resolve_node(&mut self, node: &RevisionNode) -> Result<String, Error> {
        match node {
            RevisionNode::Ref(name) => self.read_ref(name),
            RevisionNode::Parent(rev) => {
                let oid = self.resolve_node(rev)?;
                self.commit_parent(&oid)
            },
            RevisionNode::Ancestor(rev, n) => {
                let mut oid = self.resolve_node(rev)?;
                for _ in 0..*n {
                    oid = self.commit_parent(&oid)?;
                }
                Ok(oid)
            },
        }
    }
    
    // Get a reference value or try to match an abbreviated object ID
    fn read_ref(&mut self, name: &str) -> Result<String, Error> {
        // First try to read as a reference
        if let Ok(Some(oid)) = self.repo.refs.read_ref(name) {
            return Ok(oid);
        }
        
        // Then try as an abbreviated object ID
        let candidates = self.repo.database.prefix_match(name)?;
        
        match candidates.len() {
            0 => Err(Error::Generic(format!("Not a valid object name: '{}'", name))),
            1 => Ok(candidates[0].clone()),
            _ => {
                // Log ambiguous SHA1 error
                self.log_ambiguous_sha1(name, &candidates)?;
                Err(Error::Generic(format!("Not a valid object name: '{}'", name)))
            }
        }
    }
    
    // Get the parent of a commit
    fn commit_parent(&mut self, oid: &str) -> Result<String, Error> {
        // Ensure it's a commit
        let commit = self.load_typed_object(oid, "commit")?;
        
        // Get its parent
        if let Some(commit) = commit.as_any().downcast_ref::<Commit>() {
            if let Some(parent) = commit.get_parent() {
                return Ok(parent.clone());
            }
        }
        
        Err(Error::Generic(format!("Commit '{}' has no parent", oid)))
    }
    
    // Load an object and verify its type
    fn load_typed_object(&mut self, oid: &str, expected_type: &str) -> Result<Box<dyn crate::core::database::database::GitObject>, Error> {
        if oid.is_empty() {
            return Err(Error::Generic("Empty object ID".to_string()));
        }
        
        let object = self.repo.database.load(oid)?;
        
        // Check if the object is of the expected type
        if object.get_type() != expected_type {
            // Add an error message
            let message = format!("object {} is a {}, not a {}", 
                                 oid, object.get_type(), expected_type);
            self.errors.push(HintedError { 
                message, 
                hint: Vec::new() 
            });
            
            return Err(Error::Generic(format!("Not a valid {} object: '{}'", expected_type, oid)));
        }
        
        Ok(object)
    }
    
    // Just verify the object type without loading the full object
    fn verify_object_type(&mut self, oid: &str, expected_type: &str) -> Result<bool, Error> {
        let object = self.repo.database.load(oid)?;
        
        if object.get_type() != expected_type {
            let message = format!("object {} is a {}, not a {}", 
                                 oid, object.get_type(), expected_type);
            self.errors.push(HintedError { 
                message, 
                hint: Vec::new() 
            });
            return Ok(false);
        }
        
        Ok(true)
    }
    
    // Log an error for ambiguous SHA1 prefixes
    fn log_ambiguous_sha1(&mut self, name: &str, candidates: &[String]) -> Result<(), Error> {
        let message = format!("short SHA1 {} is ambiguous", name);
        let mut hints = vec![String::from("The candidates are:")];
        
        for oid in candidates {
            let obj = self.repo.database.load(oid)?;
            let short_oid = &oid[0..std::cmp::min(7, oid.len())];
            let obj_type = obj.get_type();
            
            let info_line = if obj_type == "commit" {
                if let Some(commit) = obj.as_any().downcast_ref::<Commit>() {
                    if let Some(author) = commit.get_author() {
                        let date = author.short_date();
                        let title = commit.title_line();
                        format!("{} {} {} - {}", short_oid, obj_type, date, title)
                    } else {
                        format!("{} {}", short_oid, obj_type)
                    }
                } else {
                    format!("{} {}", short_oid, obj_type)
                }
            } else {
                format!("{} {}", short_oid, obj_type)
            };
            
            hints.push(info_line);
        }
        
        self.errors.push(HintedError { message, hint: hints });
        Ok(())
    }
}

// Add Clone for RevisionNode
impl Clone for RevisionNode {
    fn clone(&self) -> Self {
        match self {
            RevisionNode::Ref(name) => RevisionNode::Ref(name.clone()),
            RevisionNode::Parent(rev) => RevisionNode::Parent(rev.clone()),
            RevisionNode::Ancestor(rev, n) => RevisionNode::Ancestor(rev.clone(), *n),
        }
    }
}