use minicbor::{Decode, Encode};
use std::collections::{HashMap, HashSet};

/// Service
pub enum Service {
    Service = 1,
    Chat = 1025,
    File,
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct PeerInfo {
    #[n(0)]
    pub peer_id: String,
    #[n(1)]
    pub name: String,
    #[n(2)]
    pub services_tag: u32,
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

    /// Services tag
    #[n(5)]
    ServicesTag(#[n(0)] u32),
}

#[derive(Clone, Debug, Encode, Decode)]
pub enum Event {
    /// client update: add/remove, client id, services
    #[n(0)]
    ClientUpdate(#[n(0)] bool, #[n(1)] u16, #[n(2)] HashSet<u16>),

    /// peer update: peer info
    #[n(1)]
    PeerUpdate(#[n(0)] PeerInfo),

    /// service update: service, map of supporting peers and their clients
    #[n(2)]
    ServiceUpdate(#[n(0)] u16, #[n(1)] HashMap<String, HashSet<u16>>),
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

    /// File message
    #[n(2)]
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
    #[n(3)]
    Register {
        #[n(0)]
        services: HashSet<u16>,
        #[n(1)]
        chat: bool,
        #[n(2)]
        files: bool,
    },

    /// Message indicating successful registration of the client
    #[n(4)]
    RegisterOk {
        #[n(0)]
        client_id: u16,
    },

    /// Get information from the daemon
    #[n(5)]
    Get {
        #[n(0)]
        client_id: u16,
        #[n(1)]
        request_id: u32,
        #[n(2)]
        content: GetSet,
    },

    /// Set configuration options on the daemon
    #[n(6)]
    Set {
        #[n(0)]
        client_id: u16,
        #[n(1)]
        request_id: u32,
        #[n(2)]
        content: GetSet,
    },

    /// Message
    #[n(7)]
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

    /// Event
    #[n(8)]
    Event {
        #[n(0)]
        to_client: u16,
        #[n(1)]
        from_client: u16,
        #[n(2)]
        event: Event,
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
