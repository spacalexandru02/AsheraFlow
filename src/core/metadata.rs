/// Provides default metadata and extension traits for file metadata operations in AsheraFlow.
use std::fs::Metadata;
use std::time::SystemTime;

/// Extension trait to add default() to Metadata.
pub trait MetadataExt {
    fn default() -> Self;
}

/// Wrapper struct for default file metadata values.
#[derive(Debug, Clone)]
pub struct DefaultMetadata {
    size: u64,
    modified: SystemTime,
    created: SystemTime,
    is_dir: bool,
    is_file: bool,
}

impl DefaultMetadata {
    /// Creates a new DefaultMetadata instance with default values.
    pub fn new() -> Self {
        DefaultMetadata {
            size: 0,
            modified: SystemTime::now(),
            created: SystemTime::now(),
            is_dir: false,
            is_file: true,
        }
    }
    
    /// Placeholder for converting DefaultMetadata to std::fs::Metadata (not implemented).
    pub fn to_metadata(&self) -> std::io::Result<Metadata> {
        unimplemented!("Cannot convert DefaultMetadata to Metadata directly")
    }
}

impl Default for DefaultMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to get default metadata for index operations.
pub fn default_metadata() -> DefaultMetadata {
    DefaultMetadata::default()
}