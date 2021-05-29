use minicbor::{Decode, Encode};

#[derive(Clone, Debug, Encode, Decode)]
pub struct PeerInfo {
    #[n(0)]
    pub peer_id: String,
    #[n(1)]
    pub name: String,
    #[n(2)]
    pub chat_support: bool,
}

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

    /// Set name of this peer
    #[n(4)]
    SetName {
        #[n(0)]
        name: String,
    },

    /// Get known peers
    #[n(5)]
    GetPeers {
        #[n(0)]
        peers: Vec<PeerInfo>,
    },

    /// Set chat support of this node
    #[n(6)]
    SetChat {
        #[n(0)]
        enabled: bool,
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
