use minicbor::{Decode, Encode};

/// announce message that is sent over gossipsub
#[derive(Debug, Encode, Decode)]
pub struct HiAnnounce {
    #[n(0)]
    pub version: u8,
    #[n(1)]
    pub name: String,
    #[n(2)]
    pub services_tag: u32,
}

impl HiAnnounce {
    pub fn new() -> Self {
        HiAnnounce {
            version: 0,
            name: String::new(),
            services_tag: 0,
        }
    }

    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut buffer = Vec::new();
        match minicbor::encode(self, &mut buffer) {
            Ok(()) => Some(buffer),
            Err(e) => {
                error!("HiAnnounce encoding error: {:?}", e);
                None
            }
        }
    }

    pub fn decode(buffer: &[u8]) -> Option<Self> {
        match minicbor::decode(buffer) {
            Ok(msg) => Some(msg),
            Err(e) => {
                error!("HiAnnounce decoding error: {:?}", e);
                None
            }
        }
    }
}
