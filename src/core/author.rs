use chrono::{DateTime, Local};
use std::fmt;

#[derive(Debug)]
pub struct Author {
    name: String,
    email: String,
    time: DateTime<Local>,
}

impl Author {
    pub fn new(name: String, email: String) -> Self {
        Author {
            name,
            email,
            time: Local::now(),
        }
    }
}

impl fmt::Display for Author {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let timestamp = self.time.format("%s %z");
        write!(f, "{} <{}> {}", self.name, self.email, timestamp)
    }
}