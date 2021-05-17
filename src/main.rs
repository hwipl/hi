use futures::executor::block_on;
use futures::prelude::*;
use libp2p::gossipsub::{
    Gossipsub, GossipsubConfig, GossipsubEvent, IdentTopic, MessageAuthenticity,
};
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::swarm::{NetworkBehaviourEventProcess, Swarm, SwarmEvent};
use libp2p::{identity, Multiaddr, NetworkBehaviour, PeerId};
use std::error::Error;

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

/// main loop for handling events
async fn handle_events(swarm: &mut Swarm<HiBehaviour>) {
    loop {
        futures::select! {
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
            }
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
