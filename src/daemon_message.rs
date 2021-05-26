use minicbor::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub enum Message {
    /// Ok message
    #[n(0)]
    Ok,

    /// Error message
    #[n(1)]
    Error {
        #[n(0)]
        message: String,
    },

    /// Connect to peer address
    #[n(2)]
    ConnectAddress {
        #[n(0)]
        address: String,
    },

    /// Get name of this peer; request contains empty name, reply contains name
    #[n(3)]
    GetName {
        #[n(0)]
        name: String,
    },
}

impl Message {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        match minicbor::decode(bytes) {
            Ok(msg) => Some(msg),
            Err(e) => {
                eprintln!("daemon message deserialization error: {:?}", e);
                None
            }
        }
    }

    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        let mut buffer = Vec::new();
        match minicbor::encode(self, &mut buffer) {
            Ok(()) => Some(buffer),
            Err(e) => {
                eprintln!("daemon message serialization error: {}", e);
                None
            }
        }
    }
}
