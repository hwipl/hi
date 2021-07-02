use minicbor::{Decode, Encode};

#[derive(Clone, Debug, Encode, Decode)]
pub struct PeerInfo {
    #[n(0)]
    pub peer_id: String,
    #[n(1)]
    pub name: String,
    #[n(2)]
    pub chat_support: bool,
    #[n(3)]
    pub file_support: bool,
    #[n(4)]
    pub last_update: u64,
}

#[derive(Clone, Debug, Encode, Decode)]
pub enum GetSet {
    /// Ok message
    #[n(0)]
    Ok,

    /// Error message
    #[n(1)]
    Error(#[n(0)] String),

    /// Name
    #[n(2)]
    Name(#[n(0)] String),

    /// Known peers
    #[n(3)]
    Peers(#[n(0)] Vec<PeerInfo>),

    /// Connect to peer address
    #[n(4)]
    Connect(#[n(0)] String),
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

    /// Chat message
    #[n(2)]
    ChatMessage {
        #[n(0)]
        to: String,
        #[n(1)]
        from: String,
        #[n(2)]
        from_name: String,
        #[n(3)]
        message: String,
    },

    /// File message
    #[n(3)]
    FileMessage {
        #[n(0)]
        to_peer: String,
        #[n(1)]
        from_peer: String,
        #[n(2)]
        to_client: u16,
        #[n(3)]
        from_client: u16,
        #[n(4)]
        content: Vec<u8>,
    },

    /// Register this client on the daemon
    #[n(4)]
    Register {
        #[n(0)]
        chat: bool,
        #[n(1)]
        files: bool,
    },

    /// Message indicating successful registration of the client
    #[n(5)]
    RegisterOk {
        #[n(0)]
        client_id: u16,
    },

    /// Get information from the daemon
    #[n(6)]
    Get {
        #[n(0)]
        client_id: u16,
        #[n(1)]
        request_id: u32,
        #[n(2)]
        content: GetSet,
    },

    /// Set configuration options on the daemon
    #[n(7)]
    Set {
        #[n(0)]
        client_id: u16,
        #[n(1)]
        request_id: u32,
        #[n(2)]
        content: GetSet,
    },

    /// Message
    #[n(8)]
    Message {
        #[n(0)]
        to_peer: String,
        #[n(1)]
        from_peer: String,
        #[n(2)]
        to_client: u16,
        #[n(3)]
        from_client: u16,
        #[n(4)]
        service: u16,
        #[n(5)]
        #[cbor(with = "minicbor::bytes")]
        content: Vec<u8>,
    },
}

impl Message {
    /// Client ID for all clients on a peer
    pub const ALL_CLIENTS: u16 = u16::MAX;

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        match minicbor::decode(bytes) {
            Ok(msg) => Some(msg),
            Err(e) => {
                error!("daemon message deserialization error: {:?}", e);
                None
            }
        }
    }

    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        let mut buffer = Vec::new();
        match minicbor::encode(self, &mut buffer) {
            Ok(()) => Some(buffer),
            Err(e) => {
                error!("daemon message serialization error: {}", e);
                None
            }
        }
    }
}
