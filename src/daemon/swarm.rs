use crate::daemon::behaviour::HiBehaviour;
use crate::daemon::gossip::HiAnnounce;
use crate::daemon::request::{HiCodec, HiRequest, HiRequestProtocol};
use async_std::task;
use futures::{channel::mpsc, executor::block_on, prelude::*, select, sink::SinkExt};
use futures_timer::Delay;
use libp2p::gossipsub::{Gossipsub, GossipsubConfig, IdentTopic, MessageAuthenticity};
use libp2p::mdns::{Mdns, MdnsConfig};
use libp2p::request_response::{ProtocolSupport, RequestResponse, RequestResponseConfig};
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{identity, Multiaddr, PeerId};
use std::error::Error;
use std::iter;
use std::str::FromStr;
use std::time::Duration;

type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

/// Hi swarm events
#[derive(Debug)]
pub enum Event {
    /// Connect to peer address: multiaddress
    ConnectAddress(String),
    /// Set node's name: name
    SetName(String),
    /// Set tag of the services supported by this node
    SetServicesTag(u32),
    /// Send message: destination peer, destination client, source client, service, content
    SendMessage(String, u16, u16, u16, Vec<u8>),

    /// Peer announcement event: id, name, services tag, file
    AnnouncePeer(String, String, u32),
    /// Message: sender, sender client, destination client, service, message
    Message(String, u16, u16, u16, Vec<u8>),
}

/// Hi swarm
pub struct HiSwarm {
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}

impl HiSwarm {
    /// main loop for handling events
    async fn handle_events(
        swarm: &mut Swarm<HiBehaviour>,
        mut receiver: Receiver<Event>,
        mut sender: Sender<Event>,
    ) {
        let mut timer = Delay::new(Duration::from_secs(5)).fuse();
        let mut node_name = String::from("");
        let mut services_tag = 0;

        loop {
            select! {
                // handle events sent to the swarm
                event = receiver.next().fuse() => {
                    debug!("received hi swarm event");
                    let event = match event {
                        Some(event) => event,
                        None => break,
                    };
                    match event {
                        // handle connect address event
                        Event::ConnectAddress(addr) => {
                            if let Ok(remote) = addr.parse::<Multiaddr>() {
                                println!("connecting to address: {}", addr);
                                if let Err(e) =
                                    swarm.dial(remote)
                                {
                                    error!("error dialing address: {}", e);
                                }
                            }
                        }

                        // handle set name request
                        Event::SetName(name) => {
                            node_name = name.clone();
                        }

                        // handle set services request
                        Event::SetServicesTag(tag) => {
                            services_tag = tag;
                        }

                        // handle send file message request
                        Event::SendMessage(to_peer, to_client, from_client, service, content) => {
                            let peer_id = match PeerId::from_str(&to_peer) {
                                Ok(peer_id) => peer_id,
                                Err(_) => continue,
                            };
                            let msg = HiRequest::Message(to_client, from_client, service, content);
                            swarm.behaviour_mut().request.send_request(&peer_id, msg);
                        }

                        // events (coming from behaviour) not handled here,
                        // forward to daemon
                        | Event::AnnouncePeer(..)
                        | Event::Message(..)
                        => {
                            if let Err(e) = sender.send(event).await {
                                error!("Error sending swarm event: {}", e);
                            };
                        }
                    }
                },

                // handle swarm events
                event = swarm.select_next_some().fuse() => {
                    match event {
                        SwarmEvent::Behaviour(event) => {
                            debug!("Behaviour event: {:?}", event);
                        }
                        SwarmEvent::NewListenAddr{address, ..} => {
                            println!("Started listening on {:?}", address);
                        }
                        SwarmEvent::ExpiredListenAddr{address, ..} => {
                            println!("Stopped listening on {:?}", address);
                        }
                        event => debug!("{:?}", event),
                    }
                },

                // handle timer events
                event = timer => {
                    debug!("timer event: {:?}", event);
                    timer = Delay::new(Duration::from_secs(15)).fuse();

                    // check number of peers in gossipsub
                    let topic = IdentTopic::new("/hello/world");
                    if swarm.behaviour().gossip.mesh_peers(&topic.hash()).count() == 0 {
                        debug!("No nodes in mesh");

                        // get peerids of discovered peers
                        let mut peer_ids: Vec<PeerId> = Vec::new();
                        for peer_id in swarm.behaviour().mdns.discovered_nodes() {
                            if peer_ids.contains(peer_id) {
                                continue;
                            }
                            peer_ids.push(peer_id.clone());
                        }

                        // try connecting to discovered peers
                        for peer_id in peer_ids {
                            match swarm.dial(peer_id) {
                                Ok(_) => (),
                                Err(e) => error!("Dial error: {:?}", e),
                            }
                        }
                    }

                    // announce presence
                    let mut announce = HiAnnounce::new();
                    announce.name = node_name.to_string();
                    announce.services_tag = services_tag;
                    if let Some(announce) = announce.encode() {
                        match swarm.behaviour_mut().gossip.publish(topic, announce) {
                            Ok(_) => (),
                            Err(e) => error!("publish error: {:?}", e),
                        }
                    }
                },
            }
        }
    }

    /// create and run swarm
    pub async fn run() -> Result<Self, Box<dyn Error>> {
        // create key and peer id
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        println!("Local peer id: {:?}", local_peer_id);

        // create transport
        let transport = block_on(libp2p::development_transport(local_key.clone()))?;

        // create mdns
        let mdns = Mdns::new(MdnsConfig::default()).await?;

        // create gossip
        let message_authenticity = MessageAuthenticity::Signed(local_key);
        let gossipsub_config = GossipsubConfig::default();
        let mut gossip: Gossipsub = Gossipsub::new(message_authenticity, gossipsub_config)?;

        // subscribe to topic
        let topic = IdentTopic::new("/hello/world");
        gossip.subscribe(&topic).unwrap();

        // create request-response
        let protocols = iter::once((HiRequestProtocol(), ProtocolSupport::Full));
        let cfg = RequestResponseConfig::default();
        let request = RequestResponse::new(HiCodec(), protocols.clone(), cfg.clone());

        // create channel for sending/receiving events to/from the swarm
        let (to_swarm_sender, to_swarm_receiver) = mpsc::unbounded();
        let (from_swarm_sender, from_swarm_receiver) = mpsc::unbounded();

        // create network behaviour
        let behaviour = HiBehaviour {
            request,
            gossip,
            mdns,

            to_swarm: to_swarm_sender.clone(),
        };

        // create swarm
        let mut swarm = Swarm::new(transport, behaviour, local_peer_id);

        // listen on all IPs and random ports.
        swarm.listen_on("/ip6/::/tcp/0".parse()?)?;
        swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

        // start main loop
        task::spawn(async move {
            Self::handle_events(&mut swarm, to_swarm_receiver, from_swarm_sender).await;
            debug!("swarm stopped");
        });

        Ok(HiSwarm {
            sender: to_swarm_sender,
            receiver: from_swarm_receiver,
        })
    }

    /// send event to the swarm
    pub async fn send(&mut self, event: Event) {
        if let Err(e) = self.sender.send(event).await {
            error!("error sending event to swarm: {}", e);
        }
    }

    /// receive event from the swarm
    pub async fn receive(&mut self) -> Option<Event> {
        self.receiver.next().await
    }
}
