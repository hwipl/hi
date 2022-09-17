use crate::daemon::request::{HiCodec, HiRequest, HiResponse};
use crate::daemon::swarm;
use futures::channel::mpsc;
use libp2p::gossipsub::{Gossipsub, GossipsubEvent};
use libp2p::mdns::{Mdns, MdnsEvent};
use libp2p::request_response::{RequestResponse, RequestResponseEvent};
use libp2p::NetworkBehaviour;

/// Custom network behaviour with mdns, gossipsub, request-response
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "HiBehaviourEvent")]
pub struct HiBehaviour {
    pub request: RequestResponse<HiCodec>,
    pub gossip: Gossipsub,
    pub mdns: Mdns,

    // channel for sending events to the swarm
    #[behaviour(ignore)]
    pub to_swarm: mpsc::UnboundedSender<swarm::Event>,
}

#[derive(Debug)]
pub enum HiBehaviourEvent {
    RequestResponse(RequestResponseEvent<HiRequest, HiResponse>),
    Gossipsub(GossipsubEvent),
    Mdns(MdnsEvent),
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

impl From<MdnsEvent> for HiBehaviourEvent {
    fn from(event: MdnsEvent) -> Self {
        HiBehaviourEvent::Mdns(event)
    }
}
