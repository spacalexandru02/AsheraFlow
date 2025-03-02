use sha1::{Digest, Sha1};
use crate::errors::error::Error;

pub const CHECKSUM_SIZE: usize = 20;

pub struct Checksum {
    digest: Sha1,
}

impl Checksum {
    pub fn new() -> Self {
        Checksum {
            digest: Sha1::new(),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.digest.update(data);
    }

    pub fn verify(&self, expected: &[u8]) -> Result<(), Error> {
        let digest = self.digest.clone().finalize();
        
        if expected != digest.as_slice() {
            return Err(Error::Generic("Checksum does not match value stored on disk".to_string()));
        }
        
        Ok(())
    }

    pub fn finalize(&self) -> Vec<u8> {
        self.digest.clone().finalize().to_vec()
    }
}