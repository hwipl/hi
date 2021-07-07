use crate::config;
use crate::message::{Event, Message, PeerInfo, Service};
use crate::unix_socket;
use async_std::task;
use minicbor::{Decode, Encode};
use std::collections::{HashMap, HashSet};
use std::error::Error;

/// service message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
enum ServiceMessage {
    /// request all services supported by other peer
    #[n(0)]
    ServiceRequest,

    /// send all services supported by this node to requesting peer:
    /// services_tag of this node's services, map of clients and their services
    #[n(1)]
    ServiceReply(#[n(0)] u32, #[n(1)] HashMap<u16, HashSet<u16>>),
}

/// services of a node
struct ServiceMap {
    services_tag: u32,
    services: HashMap<u16, HashSet<u16>>,
}

impl ServiceMap {
    fn new() -> Self {
        ServiceMap {
            services_tag: 0,
            services: HashMap::new(),
        }
    }
}

/// service client
struct ServiceClient {
    _config: config::Config,
    client: unix_socket::UnixClient,
    client_id: u16,
    local: ServiceMap,
    peers: HashMap<String, ServiceMap>,
}

impl ServiceClient {
    /// create new service client
    pub async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        ServiceClient {
            _config: config,
            client,
            client_id: 0,
            peers: HashMap::new(),
            local: ServiceMap::new(),
        }
    }

    /// register this client
    async fn register_client(&mut self) -> Result<(), Box<dyn Error>> {
        let msg = Message::Register {
            services: vec![Service::Service as u16].into_iter().collect(),
            chat: false,
            files: false,
        };
        self.client.send_message(msg).await?;
        match self.client.receive_message().await? {
            Message::RegisterOk { client_id } => {
                self.client_id = client_id;
                Ok(())
            }
            _ => Err("unexpected message from daemon".into()),
        }
    }

    /// send service message to other peer
    async fn send_message(
        &mut self,
        peer_id: String,
        client_id: u16,
        message: ServiceMessage,
    ) -> Result<(), Box<dyn Error>> {
        let mut content = Vec::new();
        minicbor::encode(message, &mut content)?;
        let msg = Message::Message {
            to_peer: peer_id,
            from_peer: String::new(),
            to_client: client_id,
            from_client: self.client_id,
            service: Service::Service as u16,
            content,
        };
        self.client.send_message(msg).await?;
        Ok(())
    }

    /// handle ClientUpdate "event" message
    async fn handle_event_client_update(
        &mut self,
        mut add: bool,
        client_id: u16,
        services: HashSet<u16>,
    ) -> Result<(), Box<dyn Error>> {
        // treat empty services as remove
        if services.is_empty() {
            add = false;
        }

        if add {
            // add/update entry
            match self.local.services.get_mut(&client_id) {
                None => {
                    self.local.services.insert(client_id, services);
                }
                Some(s) => {
                    *s = services;
                }
            }
        } else {
            // remove entry
            self.local.services.remove(&client_id);
        }
        Ok(())
    }

    /// handle PeerUpdate "event" message
    async fn handle_event_peer_update(
        &mut self,
        peer_info: PeerInfo,
    ) -> Result<(), Box<dyn Error>> {
        // check/update peer entry
        let peer_id = peer_info.peer_id.clone();
        let mut request_update = false;
        match self.peers.get_mut(&peer_id) {
            None => {
                // add new peer entry
                self.peers.insert(peer_id.clone(), ServiceMap::new());
                request_update = true;
            }
            Some(p) => {
                // check if we need to update services
                if p.services_tag != peer_info.service_id {
                    request_update = true;
                }

                // update existing peer entry
                p.services_tag = peer_info.service_id;
            }
        }

        // request service update from peer
        if request_update {
            let request = ServiceMessage::ServiceRequest;
            self.send_message(peer_id, Message::ALL_CLIENTS, request)
                .await?;
        }
        Ok(())
    }

    /// handle "event" message
    async fn handle_event(&mut self, event: Event) -> Result<(), Box<dyn Error>> {
        match event {
            Event::ClientUpdate(add, client_id, services) => {
                self.handle_event_client_update(add, client_id, services)
                    .await?
            }
            Event::PeerUpdate(peer_info) => self.handle_event_peer_update(peer_info).await?,
        }
        Ok(())
    }

    /// handle ServiceRequest message
    async fn handle_message_service_request(
        &mut self,
        from_peer: String,
        from_client: u16,
    ) -> Result<(), Box<dyn Error>> {
        let reply =
            ServiceMessage::ServiceReply(self.local.services_tag, self.local.services.clone());
        self.send_message(from_peer, from_client, reply).await?;
        Ok(())
    }

    /// handle ServiceReply message
    async fn handle_message_service_reply(
        &mut self,
        from_peer: String,
        _from_client: u16,
        services_tag: u32,
        services: HashMap<u16, HashSet<u16>>,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(peer) = self.peers.get_mut(&from_peer) {
            peer.services_tag = services_tag;
            peer.services = services;
        }
        Ok(())
    }

    /// handle "message" message
    async fn handle_message(
        &mut self,
        from_peer: String,
        from_client: u16,
        content: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        if let Ok(msg) = minicbor::decode::<ServiceMessage>(&content) {
            match msg {
                ServiceMessage::ServiceRequest => {
                    self.handle_message_service_request(from_peer, from_client)
                        .await?;
                }
                ServiceMessage::ServiceReply(services_tag, services) => {
                    self.handle_message_service_reply(
                        from_peer,
                        from_client,
                        services_tag,
                        services,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    /// run service client
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.register_client().await?;
        loop {
            let msg = self.client.receive_message().await?;
            debug!("received message {:?}", msg);
            match msg {
                Message::Event { event, .. } => self.handle_event(event).await?,
                Message::Message {
                    from_peer,
                    from_client,
                    content,
                    ..
                } => self.handle_message(from_peer, from_client, content).await?,
                _ => (),
            }
        }
    }
}

/// run daemon client in service mode
pub fn run(config: config::Config) {
    task::spawn(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => {
                if let Err(e) = ServiceClient::new(config, client).await.run().await {
                    error!("{}", e);
                }
            }
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("service client stopped");
    });
}
