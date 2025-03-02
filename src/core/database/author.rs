use chrono::{DateTime, Utc};
use std::fmt;

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