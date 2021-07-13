use crate::daemon::gossip::HiAnnounce;
use crate::daemon::request::{HiCodec, HiRequest, HiResponse};
use crate::daemon::swarm;
use async_std::task;
use futures::channel::mpsc;
use futures::sink::SinkExt;
use libp2p::gossipsub::{Gossipsub, GossipsubEvent};
use libp2p::mdns::{Mdns, MdnsEvent};
use libp2p::request_response::{RequestResponse, RequestResponseEvent, RequestResponseMessage};
use libp2p::swarm::NetworkBehaviourEventProcess;
use libp2p::{NetworkBehaviour, PeerId};

/// Custom network behaviour with mdns, gossipsub, request-response
#[derive(NetworkBehaviour)]
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
    fn handle_request(&mut self, peer: PeerId, request: HiRequest) -> HiResponse {
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

            // handle other requests
            _ => HiResponse::Error(String::from("unknown request")),
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<HiRequest, HiResponse>> for HiBehaviour {
    // hande `request` events
    fn inject_event(&mut self, message: RequestResponseEvent<HiRequest, HiResponse>) {
        // handle incoming messages
        if let RequestResponseEvent::Message { peer, message } = message {
            match message {
                // handle incoming request message, send back response
                RequestResponseMessage::Request {
                    channel,
                    request,
                    request_id,
                } => {
                    debug!(
                        "received request {:?} with id {} from {:?}",
                        request, request_id, peer
                    );
                    let response = self.handle_request(peer, request);
                    self.request.send_response(channel, response).unwrap();
                    return;
                }

                // handle incoming response message
                RequestResponseMessage::Response { response, .. } => {
                    debug!("received response {:?} from {:?}", response, peer);
                    return;
                }
            }
        }

        // handle response sent event
        if let RequestResponseEvent::ResponseSent { peer, request_id } = message {
            debug!("sent response for request {:?} to {:?}", request_id, peer);
            return;
        }

        error!("request response error: {:?}", message);
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for HiBehaviour {
    // handle `gossip` events
    fn inject_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message { message, .. } => match HiAnnounce::decode(&message.data) {
                Some(msg) => {
                    debug!(
                        "Message: {:?} -> {:?}: {:?}",
                        message.source, message.topic, msg
                    );
                    if let Some(peer) = message.source {
                        let swarm_event = swarm::Event::AnnouncePeer(
                            peer.to_string(),
                            msg.name,
                            msg.services_tag,
                        );
                        let mut to_swarm = self.to_swarm.clone();
                        task::spawn(async move {
                            if let Err(e) = to_swarm.send(swarm_event).await {
                                error!("error sending event to swarm: {}", e);
                            }
                        });
                    }
                }
                None => {
                    debug!(
                        "Message: {:?} -> {:?}: {:?}",
                        message.source, message.topic, message.data
                    );
                }
            },
            GossipsubEvent::Subscribed { peer_id, topic } => {
                debug!("Subscribed: {:?} {:?}", peer_id, topic);
            }
            GossipsubEvent::Unsubscribed { peer_id, topic } => {
                debug!("Unsubscribed: {:?} {:?}", peer_id, topic);
            }
        }
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for HiBehaviour {
    // handle `mdns` events
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, addr) in list {
                    debug!("Peer discovered: {:?} {:?}", peer, addr);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, addr) in list {
                    if !self.mdns.has_node(&peer) {
                        debug!("Peer expired: {:?} {:?}", peer, addr);
                    }
                }
            }
        }
    }
}
