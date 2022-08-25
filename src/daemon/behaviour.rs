use crate::daemon::request::{HiCodec, HiRequest, HiResponse};
use crate::daemon::swarm;
use async_std::task;
use futures::channel::mpsc;
use futures::sink::SinkExt;
use libp2p::gossipsub::{Gossipsub, GossipsubEvent};
use libp2p::mdns::{Mdns, MdnsEvent};
use libp2p::request_response::{RequestResponse, RequestResponseEvent};
use libp2p::{NetworkBehaviour, PeerId};

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

impl HiBehaviour {
    /// handle request response "request" message
    pub fn handle_request(&mut self, peer: PeerId, request: HiRequest) -> HiResponse {
        match request {
            // handle message
            HiRequest::Message(to_client, from_client, service, content) => {
                debug!("received message: {:?}", content);
                let swarm_event = swarm::Event::Message(
                    peer.to_base58(),
                    from_client,
                    to_client,
                    service,
                    content,
                );
                let mut to_swarm = self.to_swarm.clone();
                task::spawn(async move {
                    if let Err(e) = to_swarm.send(swarm_event).await {
                        error!("error sending event to swarm: {}", e);
                    }
                });
                HiResponse::Ok
            }
        }
    }
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
