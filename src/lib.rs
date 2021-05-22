mod behaviour;
mod gossip;
mod request;

use behaviour::HiBehaviour;
use futures::{executor::block_on, prelude::*, select};
use gossip::HiAnnounce;
use libp2p::gossipsub::{Gossipsub, GossipsubConfig, IdentTopic, MessageAuthenticity};
use libp2p::mdns::{Mdns, MdnsConfig};
use libp2p::request_response::{ProtocolSupport, RequestResponse, RequestResponseConfig};
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{identity, Multiaddr, PeerId};
use request::{HiCodec, HiRequestProtocol};
use std::error::Error;
use std::iter;
use std::time::Duration;
use wasm_timer::Delay;

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

pub fn run() -> Result<(), Box<dyn Error>> {
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
    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial_addr(remote.clone())?;
        println!("Connecting to {}", addr);
    }

    // start main loop
    block_on(handle_events(&mut swarm));

    Ok(())
}
