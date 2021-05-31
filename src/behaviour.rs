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
                    println!(
                        "received request {:?} with id {} from {:?}",
                        request, request_id, peer
                    );
                    let response = match request {
                        HiRequest::ChatMessage(msg) => {
                            println!("received chat message: {}", msg);
                            let swarm_event = swarm::Event::ChatMessage(peer.to_base58(), msg);
                            let mut to_swarm = self.to_swarm.clone();
                            task::spawn(async move {
                                if let Err(e) = to_swarm.send(swarm_event).await {
                                    eprintln!("error sending event to swarm: {}", e);
                                }
                            });
                            HiResponse::Ok
                        }
                        _ => HiResponse::Error(String::from("unknown request")),
                    };
                    self.request.send_response(channel, response).unwrap();
                    return;
                }

                // handle incoming response message
                RequestResponseMessage::Response { response, .. } => {
                    println!("received response {:?} from {:?}", response, peer);
                    return;
                }
            }
        }

        // handle response sent event
        if let RequestResponseEvent::ResponseSent { peer, request_id } = message {
            println!("sent response for request {:?} to {:?}", request_id, peer);
            return;
        }

        println!("request response error: {:?}", message);
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for HiBehaviour {
    // handle `gossip` events
    fn inject_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message { message, .. } => match HiAnnounce::decode(&message.data) {
                Some(msg) => {
                    println!(
                        "Message: {:?} -> {:?}: {:?}",
                        message.source, message.topic, msg
                    );
                    if let Some(peer) = message.source {
                        let swarm_event = swarm::Event::AnnouncePeer(PeerInfo {
                            peer_id: peer.to_string(),
                            name: msg.name,
                            chat_support: msg.chat,
                        });
                        let mut to_swarm = self.to_swarm.clone();
                        task::spawn(async move {
                            if let Err(e) = to_swarm.send(swarm_event).await {
                                eprintln!("error sending event to swarm: {}", e);
                            }
                        });
                    }
                }
                None => {
                    println!(
                        "Message: {:?} -> {:?}: {:?}",
                        message.source, message.topic, message.data
                    );
                }
            },
            GossipsubEvent::Subscribed { peer_id, topic } => {
                println!("Subscribed: {:?} {:?}", peer_id, topic);
            }
            GossipsubEvent::Unsubscribed { peer_id, topic } => {
                println!("Unsubscribed: {:?} {:?}", peer_id, topic);
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
                    println!("Peer discovered: {:?} {:?}", peer, addr);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, addr) in list {
                    if !self.mdns.has_node(&peer) {
                        println!("Peer expired: {:?} {:?}", peer, addr);
                    }
                }
            }
        }
    }
}
