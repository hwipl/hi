use crate::config;
use crate::message::{Event, GetSet, Message, PeerInfo, Service};
use crate::unix_socket;
use async_std::task;
use minicbor::{Decode, Encode};
use std::collections::{HashMap, HashSet};
use std::error::Error;

type ClientId = u16;
type ServiceId = u16;

/// service message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
enum ServiceMessage {
    /// request all services supported by other peer
    #[n(0)]
    ServiceRequest,

    /// send all services supported by this node to requesting peer:
    // TODO: swap key/value type?
    #[n(1)]
    ServiceReply {
        /// services tag of this node's services
        #[n(0)]
        services_tag: u32,

        /// mapping of client id to a set of services supported by the client
        #[n(1)]
        services: HashMap<ClientId, HashSet<ServiceId>>,
    },
}

/// services of a node
struct ServiceMap {
    /// services tag of the services
    services_tag: u32,

    /// mapping of client id to a set of services supported by the client
    // TODO: swap key/value type?
    services: HashMap<ClientId, HashSet<ServiceId>>,
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
    client_id: ClientId,
    request_id: u32,
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
            request_id: 0,
            peers: HashMap::new(),
            local: ServiceMap::new(),
        }
    }

    /// get next request id
    fn get_request_id(&mut self) -> u32 {
        self.request_id = self.request_id.wrapping_add(1);
        self.request_id
    }

    /// register this client
    async fn register_client(&mut self) -> Result<(), Box<dyn Error>> {
        let msg = Message::Register {
            services: vec![Service::Service as ServiceId].into_iter().collect(),
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
        client_id: ClientId,
        message: ServiceMessage,
    ) -> Result<(), Box<dyn Error>> {
        let mut content = Vec::new();
        minicbor::encode(message, &mut content)?;
        let msg = Message::Message {
            to_peer: peer_id,
            from_peer: String::new(),
            to_client: client_id,
            from_client: self.client_id,
            service: Service::Service as ServiceId,
            content,
        };
        self.client.send_message(msg).await?;
        Ok(())
    }

    /// update services tag
    async fn update_services_tag(&mut self) -> Result<(), Box<dyn Error>> {
        self.local.services_tag = 0;
        if !self.local.services.is_empty() {
            self.local.services_tag = rand::random();
        }
        let msg = Message::Set {
            client_id: self.client_id,
            request_id: self.get_request_id(),
            content: GetSet::ServicesTag(self.local.services_tag),
        };
        self.client.send_message(msg).await?;
        Ok(())
    }

    /// update services and send service updates to interested clients
    async fn update_services(&mut self) -> Result<(), Box<dyn Error>> {
        // get list of all services local clients are interested in
        // TODO: turn set into a map from service to clients?
        let mut services = HashSet::<ServiceId>::new();
        for local_services in self.local.services.values() {
            for service in local_services {
                services.insert(*service);
            }
        }

        // for every service a local client is interested in...
        for s in services {
            // (1) find peers and their clients
            // TODO: also check local services?
            let mut map = HashMap::<String, HashSet<ClientId>>::new();
            for (peer_id, peer) in self.peers.iter() {
                let mut peer_clients = HashSet::<ClientId>::new();
                for (peer_client, peer_services) in peer.services.iter() {
                    if peer_services.contains(&s) {
                        peer_clients.insert(*peer_client);
                    }
                }

                if !peer_clients.is_empty() {
                    map.insert(peer_id.to_string(), peer_clients);
                }
            }

            // (2) send to local clients
            for (client_id, local_services) in self.local.services.iter() {
                if local_services.contains(&s) {
                    let event = Message::Event {
                        to_client: *client_id,
                        from_client: self.client_id,
                        event: Event::ServiceUpdate(s, map.clone()),
                    };
                    self.client.send_message(event).await?;
                }
            }
        }
        Ok(())
    }

    /// handle ClientUpdate "event" message
    async fn handle_event_client_update(
        &mut self,
        mut add: bool,
        client_id: ClientId,
        services: HashSet<ServiceId>,
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

        // TODO: send service update to local clients?
        self.update_services_tag().await?;
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
                if p.services_tag != peer_info.services_tag {
                    request_update = true;
                }

                // update existing peer entry
                p.services_tag = peer_info.services_tag;
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
            _ => (),
        }
        Ok(())
    }

    /// handle ServiceRequest message
    async fn handle_message_service_request(
        &mut self,
        from_peer: String,
        from_client: ClientId,
    ) -> Result<(), Box<dyn Error>> {
        let reply = ServiceMessage::ServiceReply {
            services_tag: self.local.services_tag,
            services: self.local.services.clone(),
        };
        self.send_message(from_peer, from_client, reply).await?;
        Ok(())
    }

    /// handle ServiceReply message
    async fn handle_message_service_reply(
        &mut self,
        from_peer: String,
        _from_client: ClientId,
        services_tag: u32,
        services: HashMap<ClientId, HashSet<ServiceId>>,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(peer) = self.peers.get_mut(&from_peer) {
            peer.services_tag = services_tag;
            peer.services = services;
        }
        self.update_services().await?;
        Ok(())
    }

    /// handle "message" message
    async fn handle_message(
        &mut self,
        from_peer: String,
        from_client: ClientId,
        content: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        if let Ok(msg) = minicbor::decode::<ServiceMessage>(&content) {
            match msg {
                ServiceMessage::ServiceRequest => {
                    self.handle_message_service_request(from_peer, from_client)
                        .await?;
                }
                ServiceMessage::ServiceReply {
                    services_tag,
                    services,
                } => {
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
