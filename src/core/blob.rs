use super::database::GitObject;

#[derive(Debug)]
pub struct Blob {
    oid: Option<String>,
    data: Vec<u8>,
}

impl GitObject for Blob {
    fn get_type(&self) -> &str {
        "blob"
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.data.clone()
    }

    fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }
}

impl Blob {
    pub fn new(data: Vec<u8>) -> Self {
        Blob { oid: None, data }
    }


    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }
}