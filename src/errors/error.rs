use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    PathResolution(String),
    DirectoryCreation(String),
    InvalidPath(String),
    Generic(String),
    IO(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::PathResolution(msg) => write!(f, "Path resolution error: {}", msg),
            Error::DirectoryCreation(msg) => write!(f, "Directory creation error: {}", msg),
            Error::InvalidPath(msg) => write!(f, "Invalid path: {}", msg),
            Error::Generic(msg) => write!(f, "Error: {}", msg),
            Error::IO(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::IO(error)
    }
}