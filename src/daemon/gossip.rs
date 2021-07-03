use minicbor::{Decode, Encode};

/// announce message that is sent over gossipsub
#[derive(Debug, Encode, Decode)]
pub struct HiAnnounce {
    #[n(0)]
    pub version: u8,
    #[n(1)]
    pub name: String,
    #[n(2)]
    pub services: u32,
    #[n(3)]
    pub chat: bool,
    #[n(4)]
    pub files: bool,
}

impl HiAnnounce {
    pub fn new() -> Self {
        HiAnnounce {
            version: 0,
            name: String::new(),
            services: 0,
            chat: false,
            files: false,
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
