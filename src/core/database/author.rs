// Actualizare pentru src/core/database/author.rs
use chrono::{DateTime, TimeZone, Utc};
use std::fmt;
use regex::Regex;

#[derive(Debug, Clone)]
pub struct Author {
    pub name: String,
    pub email: String,
    pub timestamp: DateTime<Utc>,
}

impl Author {
    pub fn new(name: String, email: String) -> Self {
        Author {
            name,
            email,
            timestamp: Utc::now(),
        }
    }
    
    /// Parsează un autor din formatul "Name <email> timestamp timezone"
    pub fn parse(author_str: &str) -> Result<Self, String> {
        // Folosim un regex pentru a parsa formatul
        let re = Regex::new(r"^(.*) <(.*)> (\d+) (.*)$").unwrap();
        
        match re.captures(author_str) {
            Some(caps) => {
                let name = caps.get(1).unwrap().as_str().to_string();
                let email = caps.get(2).unwrap().as_str().to_string();
                let timestamp_str = caps.get(3).unwrap().as_str();
                
                // Parsează timestamp-ul ca i64
                let timestamp_i64 = match timestamp_str.parse::<i64>() {
                    Ok(ts) => ts,
                    Err(_) => return Err(format!("Invalid timestamp: {}", timestamp_str)),
                };
                
                // Creează DateTime din timestamp
                let timestamp = match Utc.timestamp_opt(timestamp_i64, 0) {
                    chrono::LocalResult::Single(dt) => dt,
                    _ => return Err(format!("Invalid timestamp value: {}", timestamp_i64)),
                };
                
                Ok(Author {
                    name,
                    email,
                    timestamp,
                })
            },
            None => Err(format!("Invalid author format: {}", author_str)),
        }
    }
}

impl fmt::Display for Author {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} <{}> {} +0000",
            self.name,
            self.email,
            self.timestamp.timestamp()
        )
    }
}