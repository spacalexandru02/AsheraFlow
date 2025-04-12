// Place this in an appropriate file (e.g., src/core/metadata.rs or inline where needed)

use std::fs::Metadata;
use std::time::SystemTime;
// We can't implement Default directly on Metadata because it's a foreign type,
// but we can create a new struct that wraps it and implement conversions
#[derive(Debug, Clone)]
pub struct DefaultMetadata {
    size: u64,
    modified: SystemTime,
    created: SystemTime,
    is_dir: bool,
    is_file: bool,
}

impl DefaultMetadata {
    pub fn new() -> Self {
        DefaultMetadata {
            size: 0,
            modified: SystemTime::now(),
            created: SystemTime::now(),
            is_dir: false,
            is_file: true,
        }
    }
}

impl Default for DefaultMetadata {
    fn default() -> Self {
        Self::new()
    }
}