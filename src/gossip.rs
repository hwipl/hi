use minicbor::{Decode, Encode};

/// announce message that is sent over gossipsub
#[derive(Debug, Encode, Decode)]
pub struct HiAnnounce {
    #[n(0)]
    pub version: u8,
    #[n(1)]
    pub name: String,
}

impl HiAnnounce {
    pub fn new() -> Self {
        HiAnnounce {
            version: 0,
            name: String::new(),
        }
    }

    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut buffer = Vec::new();
        match minicbor::encode(self, &mut buffer) {
            Ok(()) => Some(buffer),
            Err(e) => {
                println!("HiAnnounce encoding error: {:?}", e);
                None
            }
        }
    }

    pub fn decode(buffer: &[u8]) -> Option<Self> {
        match minicbor::decode(buffer) {
            Ok(msg) => Some(msg),
            Err(e) => {
                println!("HiAnnounce decoding error: {:?}", e);
                None
            }
        }
    }
}
