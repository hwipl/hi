use crate::behaviour::HiBehaviour;
use crate::daemon_message::PeerInfo;
use crate::gossip::HiAnnounce;
use crate::request::{HiCodec, HiRequest, HiRequestProtocol, HiResponse};
use async_std::task;
use futures::{channel::mpsc, executor::block_on, prelude::*, select, sink::SinkExt};
use libp2p::gossipsub::{Gossipsub, GossipsubConfig, IdentTopic, MessageAuthenticity};
use libp2p::mdns::{Mdns, MdnsConfig};
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, ResponseChannel,
};
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{identity, PeerId};
use std::error::Error;
use std::iter;
use std::str::FromStr;
use std::time::Duration;
use wasm_timer::Delay;

type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

/// Hi swarm events
#[derive(Debug)]
pub enum Event {
    /// Connect to peer address: multiaddress
    ConnectAddress(String),
    /// Set node's name: name
    SetName(String),
    /// Set chat support to enabled (true) or disabled (false)
    SetChat(bool),
    /// Send chat message: destination, message
    SendChatMessage(String, String),
    /// Set file support to enabled (true) or disabled (false)
    SetFiles(bool),
    /// Send get files message to destination
    SendGetFiles(String),
    /// Send file list as response to get files list request from other peer
    SendFileList(ResponseChannel<HiResponse>, Vec<(String, u64)>),
    /// Send download file request to other peer: peer id, file, destination
    SendDownloadFile(String, String, String),
    /// Accept download file request from other peer
    AcceptDownloadFile(ResponseChannel<HiResponse>, String),

    /// Peer announcement event
    AnnouncePeer(PeerInfo),
    /// Chat message: sender, message
    ChatMessage(String, String),
    /// File list: sender, list of names and sizes
    FileList(String, Vec<(String, u64)>),
    /// Received get files message from source
    ReceivedGetFiles(String, ResponseChannel<HiResponse>),
    /// Received download file request: peer id, file
    ReceivedDownloadFile(String, String, ResponseChannel<HiResponse>),
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
        let mut chat_support = false;
        let mut file_support = false;

        loop {
            select! {
                // handle events sent to the swarm
                event = receiver.next().fuse() => {
                    println!("received hi swarm event");
                    let event = match event {
                        Some(event) => event,
                        None => break,
                    };
                    match event {
                        // handle connect address event
                        Event::ConnectAddress(addr) => {
                            if let Ok(remote) = addr.parse() {
                                println!("connecting to address: {}", addr);
                                if let Err(e) = swarm.dial_addr(remote) {
                                    eprintln!("error dialing address: {}", e);
                                }
                            }
                        }

                        // handle set name request
                        Event::SetName(name) => {
                            node_name = name.clone();
                        }

                        // handle set chat support request
                        Event::SetChat(enabled) => {
                            chat_support = enabled;
                        }

                        // handle set chat message request
                        Event::SendChatMessage(to, msg) => {
                            let peer_id = match PeerId::from_str(&to) {
                                Ok(peer_id) => peer_id,
                                Err(_) => continue,
                            };
                            let chat_msg = HiRequest::ChatMessage(msg.to_string());
                            swarm.behaviour_mut().request.send_request(&peer_id, chat_msg);
                        }

                        // handle set files message request
                        Event::SetFiles(enabled) => {
                            file_support = enabled;
                        }

                        // handle set get files message request
                        Event::SendGetFiles(to) => {
                            let peer_id = match PeerId::from_str(&to) {
                                Ok(peer_id) => peer_id,
                                Err(_) => continue,
                            };
                            let request = HiRequest::GetFiles;
                            swarm.behaviour_mut().request.send_request(&peer_id, request);
                        }

                        // handle send file list request
                        Event::SendFileList(channel, file_list) => {
                            let response = HiResponse::FileList(file_list.clone());
                            if let Err(_) =
                                swarm.behaviour_mut().request.send_response(channel, response) {
                                eprintln!("Error sending file list response");
                            }
                        }

                        // handle send download file request
                        Event::SendDownloadFile(to, file, ..) => {
                            let peer_id = match PeerId::from_str(&to) {
                                Ok(peer_id) => peer_id,
                                Err(_) => continue,
                            };
                            let request = HiRequest::DownloadFile(file);
                            swarm.behaviour_mut().request.send_request(&peer_id, request);
                        }

                        // handle accept download file request
                        Event::AcceptDownloadFile(channel, file) => {
                            let response = HiResponse::DownloadFile(file, 0);
                            if let Err(_) =
                                swarm.behaviour_mut().request.send_response(channel, response) {
                                eprintln!("Error sending accept download file response");
                            }
                        }

                        // events (coming from behaviour) not handled here,
                        // forward to daemon
                        | Event::AnnouncePeer(..)
                        | Event::ChatMessage(..)
                        | Event::FileList(..)
                        | Event::ReceivedGetFiles(..)
                        | Event::ReceivedDownloadFile(..)
                        => {
                            if let Err(e) = sender.send(event).await {
                                eprintln!("Error sending swarm event: {}", e);
                            };
                        }
                    }
                },

                // handle swarm events
                event = swarm.next_event().fuse() => {
                    match event {
                        SwarmEvent::Behaviour(event) => {
                            println!("Behaviour event: {:?}", event);
                        }
                        SwarmEvent::NewListenAddr(addr) => {
                            println!("Started listing on {:?}", addr);
                        }
                        SwarmEvent::ExpiredListenAddr(addr) => {
                            println!("Stopped listening on {:?}", addr);
                        }
                        event => println!("{:?}", event),
                    }
                },

                // handle timer events
                event = timer => {
                    println!("timer event: {:?}", event);
                    timer = Delay::new(Duration::from_secs(15)).fuse();

                    // check number of peers in gossipsub
                    let topic = IdentTopic::new("/hello/world");
                    if swarm.behaviour().gossip.mesh_peers(&topic.hash()).count() == 0 {
                        println!("No nodes in mesh");

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
                            match swarm.dial(&peer_id) {
                                Ok(_) => (),
                                Err(e) => println!("Dial error: {:?}", e),
                            }
                        }
                    }

                    // announce presence
                    let mut announce = HiAnnounce::new();
                    announce.name = node_name.to_string();
                    announce.chat = chat_support;
                    announce.files = file_support;
                    if let Some(announce) = announce.encode() {
                        match swarm.behaviour_mut().gossip.publish(topic, announce) {
                            Ok(_) => (),
                            Err(e) => println!("publish error: {:?}", e),
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
            println!("swarm stopped");
        });

        Ok(HiSwarm {
            sender: to_swarm_sender,
            receiver: from_swarm_receiver,
        })
    }

    /// send event to the swarm
    pub async fn send(&mut self, event: Event) {
        if let Err(e) = self.sender.send(event).await {
            eprintln!("error sending event to swarm: {}", e);
        }
    }

    /// receive event from the swarm
    pub async fn receive(&mut self) -> Option<Event> {
        self.receiver.next().await
    }
}
