use crate::daemon::behaviour::{HiBehaviour, HiBehaviourEvent};
use crate::daemon::gossip::HiAnnounce;
use crate::daemon::request::{HiRequest, HiRequestProtocol, HiResponse};
use async_std::task;
use futures::{channel::mpsc, prelude::*, select, sink::SinkExt};
use futures_timer::Delay;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{gossipsub, mdns, request_response, Multiaddr, PeerId, SwarmBuilder};
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

/// Hi swarm handler
struct HiSwarmHandler {
    swarm: Swarm<HiBehaviour>,
    receiver: Receiver<Event>,
    sender: Sender<Event>,

    node_name: String,
    services_tag: u32,
}

impl HiSwarmHandler {
    /// handle event sent to the swarm
    async fn handle_receiver_event(&mut self, event: Event) {
        match event {
            // handle connect address event
            Event::ConnectAddress(addr) => {
                if let Ok(remote) = addr.parse::<Multiaddr>() {
                    println!("connecting to address: {}", addr);
                    if let Err(e) = self.swarm.dial(remote) {
                        error!("error dialing address: {}", e);
                    }
                }
            }

            // handle set name request
            Event::SetName(name) => {
                self.node_name = name.clone();
            }

            // handle set services request
            Event::SetServicesTag(tag) => {
                self.services_tag = tag;
            }

            // handle send file message request
            Event::SendMessage(to_peer, to_client, from_client, service, content) => {
                let peer_id = match PeerId::from_str(&to_peer) {
                    Ok(peer_id) => peer_id,
                    Err(_) => return,
                };
                let msg = HiRequest::Message(to_client, from_client, service, content);
                self.swarm
                    .behaviour_mut()
                    .request
                    .send_request(&peer_id, msg);
            }

            // events (coming from behaviour) not handled here,
            // forward to daemon
            Event::AnnouncePeer(..) | Event::Message(..) => {
                if let Err(e) = self.sender.send(event).await {
                    error!("Error sending swarm event: {}", e);
                };
            }
        }
    }

    /// handle request response "request" message
    pub fn handle_request_response_request(
        &mut self,
        peer: PeerId,
        request: HiRequest,
    ) -> HiResponse {
        match request {
            // handle message
            HiRequest::Message(to_client, from_client, service, content) => {
                debug!("received message: {:?}", content);
                let swarm_event =
                    Event::Message(peer.to_base58(), from_client, to_client, service, content);
                let mut to_swarm = self.sender.clone();
                task::spawn(async move {
                    if let Err(e) = to_swarm.send(swarm_event).await {
                        error!("error sending event to swarm: {}", e);
                    }
                });
                HiResponse::Ok
            }
        }
    }

    /// handle request response event
    async fn handle_request_response_event(
        &mut self,
        event: request_response::Event<HiRequest, HiResponse>,
    ) {
        // handle incoming messages
        if let request_response::Event::Message {
            peer,
            connection_id: _,
            message,
        } = event
        {
            match message {
                // handle incoming request message, send back response
                request_response::Message::Request {
                    channel,
                    request,
                    request_id,
                } => {
                    debug!(
                        "received request {:?} with id {} from {:?}",
                        request, request_id, peer
                    );
                    let response = self.handle_request_response_request(peer, request);
                    self.swarm
                        .behaviour_mut()
                        .request
                        .send_response(channel, response)
                        .unwrap();
                    return;
                }

                // handle incoming response message
                request_response::Message::Response { response, .. } => {
                    debug!("received response {:?} from {:?}", response, peer);
                    return;
                }
            }
        }

        // handle response sent event
        if let request_response::Event::ResponseSent {
            peer,
            connection_id: _,
            request_id,
        } = event
        {
            debug!("sent response for request {:?} to {:?}", request_id, peer);
            return;
        }

        error!("request response error: {:?}", event);
    }

    /// handle gossipsub event
    async fn handle_gossipsub_event(&mut self, event: gossipsub::Event) {
        match event {
            gossipsub::Event::Message { message, .. } => match HiAnnounce::decode(&message.data) {
                Some(msg) => {
                    debug!(
                        "Message: {:?} -> {:?}: {:?}",
                        message.source, message.topic, msg
                    );
                    if let Some(peer) = message.source {
                        let swarm_event =
                            Event::AnnouncePeer(peer.to_string(), msg.name, msg.services_tag);
                        let mut to_swarm = self.sender.clone();
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
            gossipsub::Event::Subscribed { peer_id, topic } => {
                debug!("Subscribed: {:?} {:?}", peer_id, topic);
            }
            gossipsub::Event::Unsubscribed { peer_id, topic } => {
                debug!("Unsubscribed: {:?} {:?}", peer_id, topic);
            }
            gossipsub::Event::GossipsubNotSupported { peer_id } => {
                debug!("Gossipsub not supported: {:?}", peer_id);
            }
            gossipsub::Event::SlowPeer { peer_id, .. } => {
                debug!("Slow peer: {:?}", peer_id);
            }
        }
    }

    /// handle mdns event
    async fn handle_mdns_event(&mut self, event: mdns::Event) {
        match event {
            mdns::Event::Discovered(list) => {
                for (peer, addr) in list {
                    debug!("Peer discovered: {:?} {:?}", peer, addr);
                }
            }
            mdns::Event::Expired(list) => {
                for (peer, addr) in list {
                    if !self
                        .swarm
                        .behaviour()
                        .mdns
                        .discovered_nodes()
                        .any(|x| x == &peer)
                    {
                        debug!("Peer expired: {:?} {:?}", peer, addr);
                    }
                }
            }
        }
    }

    /// handle swarm event
    async fn handle_swarm_event(&mut self, event: SwarmEvent<HiBehaviourEvent>) {
        match event {
            // request response event
            SwarmEvent::Behaviour(HiBehaviourEvent::RequestResponse(event)) => {
                self.handle_request_response_event(event).await;
            }

            // gossipsub event
            SwarmEvent::Behaviour(HiBehaviourEvent::Gossipsub(event)) => {
                self.handle_gossipsub_event(event).await;
            }

            // mdns event
            SwarmEvent::Behaviour(HiBehaviourEvent::Mdns(event)) => {
                self.handle_mdns_event(event).await;
            }

            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Started listening on {:?}", address);
            }

            SwarmEvent::ExpiredListenAddr { address, .. } => {
                println!("Stopped listening on {:?}", address);
            }

            event => debug!("{:?}", event),
        }
    }

    /// handle timer event
    async fn handle_timer_event(&mut self) {
        // check number of peers in gossipsub
        let topic = gossipsub::IdentTopic::new("/hello/world");
        if self
            .swarm
            .behaviour()
            .gossip
            .mesh_peers(&topic.hash())
            .count()
            == 0
        {
            debug!("No nodes in mesh");

            // get peerids of discovered peers
            let mut peer_ids: Vec<PeerId> = Vec::new();
            for peer_id in self.swarm.behaviour().mdns.discovered_nodes() {
                if peer_ids.contains(peer_id) {
                    continue;
                }
                peer_ids.push(peer_id.clone());
            }

            // try connecting to discovered peers
            for peer_id in peer_ids {
                match self.swarm.dial(peer_id) {
                    Ok(_) => (),
                    Err(e) => error!("Dial error: {:?}", e),
                }
            }
        }

        // announce presence
        let mut announce = HiAnnounce::new();
        announce.name = self.node_name.to_string();
        announce.services_tag = self.services_tag;
        if let Some(announce) = announce.encode() {
            match self.swarm.behaviour_mut().gossip.publish(topic, announce) {
                Ok(_) => (),
                Err(e) => error!("publish error: {:?}", e),
            }
        }
    }

    /// main loop for handling events
    async fn handle_events(&mut self) {
        let mut timer = Delay::new(Duration::from_secs(5)).fuse();

        loop {
            select! {
                // handle events sent to the swarm
                event = self.receiver.next().fuse() => {
                    debug!("received hi swarm event");
                    let event = match event {
                        Some(event) => event,
                        None => break,
                    };
                    self.handle_receiver_event(event).await;
                },

                // handle swarm events
                event = self.swarm.select_next_some().fuse() => {
                    self.handle_swarm_event(event).await;
                },

                // handle timer events
                event = timer => {
                    debug!("timer event: {:?}", event);
                    timer = Delay::new(Duration::from_secs(15)).fuse();
                    self.handle_timer_event().await;
                },
            }
        }
    }
}

/// Hi swarm
pub struct HiSwarm {
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}

impl HiSwarm {
    /// create and run swarm
    pub async fn run() -> Result<Self, Box<dyn Error>> {
        // create swarm
        let mut swarm = SwarmBuilder::with_new_identity()
            .with_async_std()
            .with_tcp(
                Default::default(),
                (libp2p::tls::Config::new, libp2p::noise::Config::new),
                libp2p::yamux::Config::default,
            )?
            .with_dns()?
            .with_behaviour(|key| {
                // create mdns
                let mdns =
                    mdns::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;

                // create gossip
                let message_authenticity = gossipsub::MessageAuthenticity::Signed(key.clone());
                let gossipsub_config = gossipsub::Config::default();
                let mut gossip = gossipsub::Behaviour::new(message_authenticity, gossipsub_config)?;

                // subscribe to topic
                let topic = gossipsub::IdentTopic::new("/hello/world");
                gossip.subscribe(&topic).unwrap();

                // create request-response
                let protocols =
                    iter::once((HiRequestProtocol(), request_response::ProtocolSupport::Full));
                let cfg = request_response::Config::default();
                let request = request_response::Behaviour::new(protocols.clone(), cfg.clone());

                // create network behaviour
                let behaviour = HiBehaviour {
                    request,
                    gossip,
                    mdns,
                };

                Ok(behaviour)
            })?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(5)))
            .build();
        println!("Local peer id: {:?}", swarm.local_peer_id());

        // create channel for sending/receiving events to/from the swarm
        let (to_swarm_sender, to_swarm_receiver) = mpsc::unbounded();
        let (from_swarm_sender, from_swarm_receiver) = mpsc::unbounded();

        // listen on all IPs and random ports.
        swarm.listen_on("/ip6/::/tcp/0".parse()?)?;
        swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

        // start main loop
        task::spawn(async {
            let mut handler = HiSwarmHandler {
                swarm,
                receiver: to_swarm_receiver,
                sender: from_swarm_sender,
                node_name: String::from(""),
                services_tag: 0,
            };
            handler.handle_events().await;
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
