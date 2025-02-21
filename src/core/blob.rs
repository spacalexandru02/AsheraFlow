#[derive(Debug)]
pub struct Blob {
    oid: Option<String>,
    data: Vec<u8>,
}

impl Blob {
    pub fn new(data: Vec<u8>) -> Self {
        Blob { oid: None, data }
    }

    pub fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }

    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }

    pub fn get_type(&self) -> &str {
        "blob"
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.data
    }
}