use crate::behaviour::HiBehaviour;
use crate::gossip::HiAnnounce;
use crate::request::{HiCodec, HiRequestProtocol};
use async_std::task;
use futures::{channel::mpsc, executor::block_on, prelude::*, select, sink::SinkExt};
use libp2p::gossipsub::{Gossipsub, GossipsubConfig, IdentTopic, MessageAuthenticity};
use libp2p::mdns::{Mdns, MdnsConfig};
use libp2p::request_response::{ProtocolSupport, RequestResponse, RequestResponseConfig};
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{identity, Multiaddr, PeerId};
use std::error::Error;
use std::iter;
use std::time::Duration;
use wasm_timer::Delay;

type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

/// Hi swarm events
pub enum Event {}

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
        loop {
            select! {
                // handle events sent to the swarm
                event = receiver.next().fuse() => {
                    println!("received hi swarm event");
                    match event {
                        Some(event) => {
                            if let Err(e) = sender.send(event).await {
                                eprintln!("Error sending swarm event: {}", e);
                            }
                        }
                        None => {}
                    }
                }

                // handle swarm events
                event = swarm.next_event().fuse() => {
                    match event {
                        SwarmEvent::Behaviour(event) => {
                            println!("{:?}", event);
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
                    if let Some(announce) = HiAnnounce::new().encode() {
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
    pub async fn run(peer_addrs: Vec<String>) -> Result<Self, Box<dyn Error>> {
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

        // create network behaviour
        let behaviour = HiBehaviour {
            request,
            gossip,
            mdns,
        };

        // create swarm
        let mut swarm = Swarm::new(transport, behaviour, local_peer_id);

        // listen on all IPs and random ports.
        swarm.listen_on("/ip6/::/tcp/0".parse()?)?;
        swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

        // connect to peer address in first command line argument if present
        for addr in peer_addrs {
            let remote: Multiaddr = addr.parse()?;
            swarm.dial_addr(remote.clone())?;
            println!("Connecting to {}", addr);
        }

        // create channel for sending/receiving events to/from the swarm
        let (to_swarm_sender, to_swarm_receiver) = mpsc::unbounded();
        let (from_swarm_sender, from_swarm_receiver) = mpsc::unbounded();

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
