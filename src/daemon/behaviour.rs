use crate::daemon::request::{HiCodec, HiRequest, HiResponse};
use libp2p::gossipsub::{Gossipsub, GossipsubEvent};
use libp2p::mdns;
use libp2p::request_response::{RequestResponse, RequestResponseEvent};
use libp2p::swarm::NetworkBehaviour;

/// Custom network behaviour with mdns, gossipsub, request-response
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "HiBehaviourEvent")]
pub struct HiBehaviour {
    pub request: RequestResponse<HiCodec>,
    pub gossip: Gossipsub,
    pub mdns: mdns::async_io::Behaviour,
}

#[derive(Debug)]
pub enum HiBehaviourEvent {
    RequestResponse(RequestResponseEvent<HiRequest, HiResponse>),
    Gossipsub(GossipsubEvent),
    Mdns(mdns::Event),
}

impl From<RequestResponseEvent<HiRequest, HiResponse>> for HiBehaviourEvent {
    fn from(event: RequestResponseEvent<HiRequest, HiResponse>) -> Self {
        HiBehaviourEvent::RequestResponse(event)
    }
}

impl From<GossipsubEvent> for HiBehaviourEvent {
    fn from(event: GossipsubEvent) -> Self {
        HiBehaviourEvent::Gossipsub(event)
    }
}

impl From<mdns::Event> for HiBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        HiBehaviourEvent::Mdns(event)
    }
}
