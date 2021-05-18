use futures::{executor::block_on, prelude::*, select};
use libp2p::gossipsub::{
    Gossipsub, GossipsubConfig, GossipsubEvent, IdentTopic, MessageAuthenticity,
};
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::swarm::{NetworkBehaviourEventProcess, Swarm, SwarmEvent};
use libp2p::{identity, Multiaddr, NetworkBehaviour, PeerId};
use std::error::Error;
use std::time::Duration;
use wasm_timer::Delay;

/// Custom network behaviour with mdns and gossipsub
#[derive(NetworkBehaviour)]
struct HiBehaviour {
    gossip: Gossipsub,
    mdns: Mdns,
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for HiBehaviour {
    // handle `gossip` events
    fn inject_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message { message, .. } => {
                println!(
                    "Message: {:?} -> {:?}: {:?}",
                    message.source, message.topic, message.data,
                );
            }
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

/// announce message that is sent over gossipsub
struct HiAnnounce {
    version: u8,
}

impl HiAnnounce {
    fn new() -> Self {
        HiAnnounce { version: 0 }
    }

    fn encode(&self) -> Vec<u8> {
        if self.version == 0 {
            b"hi".to_vec()
        } else {
            b"unknown".to_vec()
        }
    }
}

/// main loop for handling events
async fn handle_events(swarm: &mut Swarm<HiBehaviour>) {
    let mut timer = Delay::new(Duration::from_secs(5)).fuse();
    loop {
        select! {
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
                let announce = HiAnnounce::new().encode();
                match swarm.behaviour_mut().gossip.publish(topic, announce) {
                    Ok(_) => (),
                    Err(e) => println!("publish error: {:?}", e),
                }
            },
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // create key and peer id
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    // create transport
    let transport = block_on(libp2p::development_transport(local_key.clone()))?;

    // create mdns
    let mdns = block_on(Mdns::new(MdnsConfig::default()))?;

    // create gossip
    let message_authenticity = MessageAuthenticity::Signed(local_key);
    let gossipsub_config = GossipsubConfig::default();
    let mut gossip: Gossipsub = Gossipsub::new(message_authenticity, gossipsub_config)?;

    // subscribe to topic
    let topic = IdentTopic::new("/hello/world");
    gossip.subscribe(&topic).unwrap();

    // create network behaviour
    let behaviour = HiBehaviour { gossip, mdns };

    // create swarm
    let mut swarm = Swarm::new(transport, behaviour, local_peer_id);

    // listen on all IPs and random ports.
    swarm.listen_on("/ip6/::/tcp/0".parse()?)?;
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // connect to peer address in first command line argument if present
    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial_addr(remote.clone())?;
        println!("Connecting to {}", addr);
    }

    // start main loop
    block_on(handle_events(&mut swarm));

    Ok(())
}
