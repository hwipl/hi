use crate::daemon::request::{HiCodec, HiRequest, HiResponse};
use libp2p::gossipsub;
use libp2p::mdns;
use libp2p::request_response;
use libp2p::swarm::NetworkBehaviour;

/// Custom network behaviour with mdns, gossipsub, request-response
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "HiBehaviourEvent")]
pub struct HiBehaviour {
    pub request: request_response::Behaviour<HiCodec>,
    pub gossip: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
}

#[derive(Debug)]
pub enum HiBehaviourEvent {
    RequestResponse(request_response::Event<HiRequest, HiResponse>),
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
}

impl From<request_response::Event<HiRequest, HiResponse>> for HiBehaviourEvent {
    fn from(event: request_response::Event<HiRequest, HiResponse>) -> Self {
        HiBehaviourEvent::RequestResponse(event)
    }
}

impl From<gossipsub::Event> for HiBehaviourEvent {
    fn from(event: gossipsub::Event) -> Self {
        HiBehaviourEvent::Gossipsub(event)
    }
}

impl From<mdns::Event> for HiBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        HiBehaviourEvent::Mdns(event)
    }
}
