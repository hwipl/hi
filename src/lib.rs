use async_trait::async_trait;
use futures::{executor::block_on, prelude::*, select};
use libp2p::core::{
    upgrade::{read_one, write_one},
    ProtocolName,
};
use libp2p::gossipsub::{
    Gossipsub, GossipsubConfig, GossipsubEvent, IdentTopic, MessageAuthenticity,
};
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseCodec, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};
use libp2p::swarm::{NetworkBehaviourEventProcess, Swarm, SwarmEvent};
use libp2p::{identity, Multiaddr, NetworkBehaviour, PeerId};
use minicbor::{Decode, Encode};
use std::error::Error;
use std::time::Duration;
use std::{io, iter};
use wasm_timer::Delay;

/// Request-response protocol for the request-response behaviour
#[derive(Debug, Clone)]
struct HiRequestProtocol();

impl ProtocolName for HiRequestProtocol {
    fn protocol_name(&self) -> &[u8] {
        "/hi/request/0.0.1".as_bytes()
    }
}

/// Codec for the request-response behaviour
#[derive(Clone)]
struct HiCodec();

#[async_trait]
impl RequestResponseCodec for HiCodec {
    type Protocol = HiRequestProtocol;
    type Request = HiRequest;
    type Response = HiResponse;

    async fn read_request<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_one(io, 1024)
            .map(|res| match res {
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
                Ok(vec) if vec.is_empty() => Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(vec) => Ok(HiRequest(vec)),
            })
            .await
    }

    async fn read_response<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_one(io, 1024)
            .map(|res| match res {
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
                Ok(vec) if vec.is_empty() => Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(vec) => Ok(HiResponse(vec)),
            })
            .await
    }

    async fn write_request<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
        HiRequest(data): HiRequest,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_one(io, data).await
    }

    async fn write_response<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
        HiResponse(data): HiResponse,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_one(io, data).await
    }
}

/// Request message
#[derive(Debug, Clone, PartialEq, Eq)]
struct HiRequest(Vec<u8>);

/// Response message
#[derive(Debug, Clone, PartialEq, Eq)]
struct HiResponse(Vec<u8>);

/// Custom network behaviour with mdns, gossipsub, request-response
#[derive(NetworkBehaviour)]
struct HiBehaviour {
    request: RequestResponse<HiCodec>,
    gossip: Gossipsub,
    mdns: Mdns,
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<HiRequest, HiResponse>> for HiBehaviour {
    // hande `request` events
    fn inject_event(&mut self, message: RequestResponseEvent<HiRequest, HiResponse>) {
        // create messages
        let request = HiRequest("hey".to_string().into_bytes());
        let response = HiResponse("hi".to_string().into_bytes());

        // handle incoming messages
        if let RequestResponseEvent::Message { peer, message } = message {
            match message {
                // handle incoming request message, send back response
                RequestResponseMessage::Request { channel, .. } => {
                    println!("received request {:?} from {:?}", request, peer);
                    self.request
                        .send_response(channel, response.clone())
                        .unwrap();
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
        if let RequestResponseEvent::ResponseSent { peer, .. } = message {
            println!("sent response {:?} to {:?}", response, peer);
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

/// announce message that is sent over gossipsub
#[derive(Debug, Encode, Decode)]
struct HiAnnounce {
    #[n(0)]
    version: u8,
    #[n(1)]
    name: String,
}

impl HiAnnounce {
    fn new() -> Self {
        HiAnnounce {
            version: 0,
            name: String::new(),
        }
    }

    fn encode(&self) -> Option<Vec<u8>> {
        let mut buffer = Vec::new();
        match minicbor::encode(self, &mut buffer) {
            Ok(()) => Some(buffer),
            Err(e) => {
                println!("HiAnnounce encoding error: {:?}", e);
                None
            }
        }
    }

    fn decode(buffer: &[u8]) -> Option<Self> {
        match minicbor::decode(buffer) {
            Ok(msg) => Some(msg),
            Err(e) => {
                println!("HiAnnounce decoding error: {:?}", e);
                None
            }
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
