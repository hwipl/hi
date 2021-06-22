use crate::daemon_message::PeerInfo;
use crate::gossip::HiAnnounce;
use crate::request::{HiCodec, HiRequest, HiResponse};
use crate::swarm;
use async_std::task;
use futures::channel::mpsc;
use futures::sink::SinkExt;
use libp2p::gossipsub::{Gossipsub, GossipsubEvent};
use libp2p::mdns::{Mdns, MdnsEvent};
use libp2p::request_response::{RequestResponse, RequestResponseEvent, RequestResponseMessage};
use libp2p::swarm::NetworkBehaviourEventProcess;
use libp2p::NetworkBehaviour;
use std::time::{SystemTime, UNIX_EPOCH};

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
                    let response = match request {
                        // handle chat message
                        HiRequest::ChatMessage(msg) => {
                            debug!("received chat message: {}", msg);
                            let swarm_event = swarm::Event::ChatMessage(peer.to_base58(), msg);
                            let mut to_swarm = self.to_swarm.clone();
                            task::spawn(async move {
                                if let Err(e) = to_swarm.send(swarm_event).await {
                                    error!("error sending event to swarm: {}", e);
                                }
                            });
                            HiResponse::Ok
                        }

                        // handle chat message
                        HiRequest::FileMessage(to_client, content) => {
                            debug!("received file message: {:?}", content);
                            let swarm_event =
                                swarm::Event::FileMessage(peer.to_base58(), to_client, content);
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
                    };
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
                        let swarm_event = swarm::Event::AnnouncePeer(PeerInfo {
                            peer_id: peer.to_string(),
                            name: msg.name,
                            chat_support: msg.chat,
                            file_support: msg.files,
                            last_update: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .expect("timestamp error")
                                .as_secs(),
                        });
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
