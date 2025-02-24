use std::fmt;
use std::io;
use std::path::StripPrefixError;
use globset::Error as GlobsetError;

#[derive(Debug)]
pub enum Error {
    PathResolution(String),
    DirectoryCreation(String),
    InvalidPath(String),
    Generic(String),
    IO(io::Error),
    Globset(GlobsetError), // Variantă cu un câmp de tip `GlobsetError`
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::PathResolution(msg) => write!(f, "Path resolution error: {}", msg),
            Error::DirectoryCreation(msg) => write!(f, "Directory creation error: {}", msg),
            Error::InvalidPath(msg) => write!(f, "Invalid path: {}", msg),
            Error::Generic(msg) => write!(f, "Error: {}", msg),
            Error::IO(err) => write!(f, "IO error: {}", err),
            Error::Globset(err) => write!(f, "Globset error: {}", err), // Folosește câmpul `err`
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::IO(error)
    }
}

impl From<StripPrefixError> for Error {
    fn from(error: StripPrefixError) -> Self {
        Error::Generic(format!("Failed to strip path prefix: {}", error))
    }
}

impl From<GlobsetError> for Error {
    fn from(err: GlobsetError) -> Self {
        Error::Globset(err) // Pasează `err` ca argument pentru `Globset`
    }
}