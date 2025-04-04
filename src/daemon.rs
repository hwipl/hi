mod behaviour;
mod gossip;
mod request;
mod swarm;

use crate::config;
use crate::message::{self, GetSet, Message, PeerInfo, Service};
use crate::unix_socket;
use futures::channel::mpsc;
use futures::future::FutureExt;
use futures::sink::SinkExt;
use futures::StreamExt;
use std::collections::hash_map::{Entry, HashMap};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{self, Duration, Instant};

type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

/// Daemon events
enum Event {
    AddClient(u16, Sender<Message>),
    RemoveClient(u16),
    ClientMessage(u16, Message),
}

/// Client information
struct ClientInfo {
    sender: Sender<Message>,
    services: HashSet<u16>,
}

/// Daemon
struct Daemon {
    config: config::Config,
    server: unix_socket::UnixServer,
    from_client_rx: Receiver<Event>,
    from_client_tx: Sender<Event>,
    swarm: swarm::HiSwarm,
    client_id: u16,
    clients: HashMap<u16, ClientInfo>,
    peers: HashMap<String, PeerInfo>,
    name: String,
}

impl Daemon {
    pub async fn new(
        config: config::Config,
        server: unix_socket::UnixServer,
        swarm: swarm::HiSwarm,
    ) -> Self {
        let (from_client_tx, from_client_rx) = mpsc::unbounded();
        Daemon {
            config,
            server,
            from_client_rx,
            from_client_tx,
            swarm,
            client_id: 1,
            clients: HashMap::new(),
            peers: HashMap::new(),
            name: String::new(),
        }
    }

    /// handle client connection identified by its `id`
    async fn handle_client(
        mut server: Sender<Event>,
        id: u16,
        mut client: unix_socket::UnixClient,
    ) {
        // create channel for server messages and register this client
        let (client_sender, mut client_receiver) = mpsc::unbounded();
        if let Err(e) = server.send(Event::AddClient(id, client_sender)).await {
            error!("handle client error: {}", e);
            return;
        }

        loop {
            tokio::select! {
                // handle messages from server
                msg = client_receiver.next().fuse() => {
                    match msg {
                        Some(msg) => {
                            // forward message to client
                            debug!("received server message: {:?}", msg);
                            if let Err(e) = client.send_message(msg).await {
                                error!("handle client error: {}", e);
                                break;
                            }
                        }
                        None => break,
                    }
                },

                // handle messages from client
                msg = client.receive_message().fuse() => {
                    match msg {
                        Ok(msg) => {
                            // forward message to server
                            debug!("received client message: {:?}", msg);
                            if let Err(e) = server.send(Event::ClientMessage(id, msg)).await {
                                error!("handle client error: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("error receiving client message: {}", e);
                            break;
                        }
                    }
                },
            }
        }

        // remove this client
        if let Err(e) = server.send(Event::RemoveClient(id)).await {
            error!("error removing client: {}", e);
            return;
        }
    }

    /// handle new client connection
    async fn handle_connection(&mut self, client: unix_socket::UnixClient) {
        // create new client handler
        tokio::spawn(Self::handle_client(
            self.from_client_tx.clone(),
            self.client_id,
            client,
        ));

        // update next client id
        self.client_id += 1;
        // skip ids for ALL_CLIENTS and 0
        if self.client_id == Message::ALL_CLIENTS {
            self.client_id = 1;
        }
    }

    /// handle timer event
    async fn handle_timer(&mut self) {
        // remove old entries from peers hash map
        let current_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp error")
            .as_secs();
        let mut remove_peers = Vec::new();
        for peer in self.peers.values() {
            if current_secs - peer.last_update > 30 {
                remove_peers.push(peer.peer_id.clone());
            }
        }
        for peer in remove_peers {
            self.peers.remove(&peer);
        }
    }

    /// handle "announce peer" swarm event
    async fn handle_swarm_announce_peer(
        &mut self,
        peer_id: String,
        name: String,
        services_tag: u32,
    ) {
        // add or update peer entry
        // TODO: check/update services
        let peer_info = PeerInfo {
            peer_id,
            name,
            services_tag,
            last_update: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("timestamp error")
                .as_secs(),
        };
        match self.peers.entry(peer_info.peer_id.clone()) {
            Entry::Occupied(mut entry) => {
                entry.insert(peer_info.clone());
            }
            Entry::Vacant(entry) => {
                entry.insert(peer_info.clone());
            }
        }

        // forward peer info to service clients
        for (id, client) in self.clients.iter_mut() {
            if client.services.contains(&(Service::Service as u16)) {
                let msg = Message::Event {
                    to_client: *id,
                    from_client: 0,
                    event: message::Event::PeerUpdate(peer_info.clone()),
                };
                if let Err(e) = client.sender.send(msg).await {
                    error!("handle client error: {}", e);
                }
            }
        }
    }

    /// handle "message" swarm event
    async fn handle_swarm_message(
        &mut self,
        from_peer: String,
        from_client: u16,
        to_client: u16,
        service: u16,
        content: Vec<u8>,
    ) {
        // helper for sending message to a client
        async fn send(
            client: &mut ClientInfo,
            from_peer: String,
            to_client: u16,
            from_client: u16,
            service: u16,
            content: Vec<u8>,
        ) {
            let msg = Message::Message {
                to_peer: String::new(),
                from_peer,
                to_client,
                from_client,
                service,
                content,
            };
            if let Err(e) = client.sender.send(msg).await {
                error!("handle client error: {}", e);
                return;
            }
        }

        // handle message to all clients
        if to_client == Message::ALL_CLIENTS {
            for client in self.clients.values_mut() {
                if client.services.contains(&service) {
                    send(
                        client,
                        from_peer.clone(),
                        to_client,
                        from_client,
                        service,
                        content.clone(),
                    )
                    .await;
                }
            }
            return;
        }

        // handle message to specific client
        if self.clients.contains_key(&to_client) {
            let client = self.clients.get_mut(&to_client).unwrap();
            if !client.services.contains(&service) {
                return;
            }
            send(
                client,
                from_peer.clone(),
                to_client,
                from_client,
                service,
                content.clone(),
            )
            .await;
        }
    }

    /// handle swarm event
    async fn handle_swarm_event(&mut self, event: swarm::Event) {
        match event {
            // handle peer announcement
            swarm::Event::AnnouncePeer(peer_id, name, services_tag) => {
                self.handle_swarm_announce_peer(peer_id, name, services_tag)
                    .await;
            }

            // handle messages
            swarm::Event::Message(from_peer, from_client, to_client, service, content) => {
                self.handle_swarm_message(from_peer, from_client, to_client, service, content)
                    .await;
            }

            // handle other events
            _ => (),
        }
    }

    /// handle "add client" client event
    async fn handle_client_add(&mut self, id: u16, sender: Sender<Message>) {
        debug!("received add client event with id {}", id);
        match self.clients.entry(id) {
            Entry::Occupied(..) => (),
            Entry::Vacant(entry) => {
                let client_info = ClientInfo {
                    sender,
                    services: HashSet::new(),
                };
                entry.insert(client_info);
            }
        }
    }

    /// handle "remove client" client event
    async fn handle_client_remove(&mut self, id: u16) {
        debug!("received remove client event with id {}", id);
        self.clients.remove(&id);

        // send event to service client
        for (client_id, client) in self.clients.iter_mut() {
            if client.services.contains(&(Service::Service as u16)) {
                let event = Message::Event {
                    from_client: 0,
                    to_client: *client_id,
                    event: message::Event::ClientUpdate(false, id, HashSet::new()),
                };
                if let Err(e) = client.sender.send(event).await {
                    error!("handle client error: {}", e);
                }
            }
        }
    }

    /// handle "register" client message event
    async fn handle_client_register(&mut self, id: u16, services: HashSet<u16>) -> Message {
        // update client info
        match self.clients.get_mut(&id) {
            Some(client) => {
                client.services = services.clone();
            }
            None => {
                error!("unknown client");
                return Message::Error {
                    message: "unknown client".into(),
                };
            }
        };

        // send event to service client
        for (client_id, client) in self.clients.iter_mut() {
            if *client_id == id {
                // do not send own event to registering client
                continue;
            }
            if client.services.contains(&(Service::Service as u16)) {
                let event = Message::Event {
                    from_client: 0,
                    to_client: *client_id,
                    event: message::Event::ClientUpdate(true, id, services.clone()),
                };
                if let Err(e) = client.sender.send(event).await {
                    error!("handle client error: {}", e);
                }
            }
        }

        // reply with client id
        Message::RegisterOk { client_id: id }
    }

    /// handle "get" client message event
    async fn handle_client_get(
        &mut self,
        client_id: u16,
        request_id: u32,
        content: GetSet,
    ) -> Message {
        let content = match content {
            GetSet::Name(..) => GetSet::Name(self.name.clone()),
            GetSet::Peers(..) => GetSet::Peers(self.peers.values().cloned().collect()),
            _ => GetSet::Error(String::from("Unknown get request")),
        };
        Message::Get {
            client_id,
            request_id,
            content,
        }
    }

    /// handle "set" client message event
    async fn handle_client_set(
        &mut self,
        client_id: u16,
        request_id: u32,
        content: GetSet,
    ) -> Message {
        let content = match content {
            GetSet::Name(name) => {
                self.name = name.clone();
                let event = swarm::Event::SetName(name);
                self.swarm.send(event).await;
                GetSet::Ok
            }
            GetSet::Connect(address) => {
                let event = swarm::Event::ConnectAddress(address);
                self.swarm.send(event).await;
                GetSet::Ok
            }
            GetSet::ServicesTag(tag) => {
                let event = swarm::Event::SetServicesTag(tag);
                self.swarm.send(event).await;
                GetSet::Ok
            }
            _ => GetSet::Error(String::from("Unknown set request")),
        };
        Message::Set {
            client_id,
            request_id,
            content,
        }
    }

    /// handle "message" client message event
    async fn handle_client_message(
        &mut self,
        to_peer: String,
        to_client: u16,
        from_client: u16,
        service: u16,
        content: Vec<u8>,
    ) -> Message {
        debug!("received message for {}", to_peer);
        // send message to specific peer
        let event = swarm::Event::SendMessage(to_peer, to_client, from_client, service, content);
        self.swarm.send(event).await;
        Message::Ok
    }

    /// handle "event" client message event
    async fn handle_client_event_msg(
        &mut self,
        to_client: u16,
        from_client: u16,
        event: message::Event,
    ) {
        // do not forward events for the daemon
        if to_client == 0 {
            return;
        }

        // forward event to client
        if let Some(client) = self.clients.get_mut(&to_client) {
            let msg = Message::Event {
                to_client,
                from_client,
                event,
            };
            if let Err(e) = client.sender.send(msg).await {
                error!("error forwarding event: {}", e);
            }
        }
    }

    /// handle client event
    async fn handle_client_event(&mut self, event: Event) {
        match event {
            // handle add client
            Event::AddClient(id, sender) => self.handle_client_add(id, sender).await,

            // handle remove client
            Event::RemoveClient(id) => self.handle_client_remove(id).await,

            // handle client message
            Event::ClientMessage(id, msg) => {
                debug!("received message from client: {:?}", msg);

                // check if client is valid
                if !self.clients.contains_key(&id) {
                    error!("unknown client");
                    return;
                }

                // parse message and generate reply message
                let reply = match msg {
                    // handle OK message
                    Message::Ok => Message::Ok,

                    // handle error message
                    Message::Error { message } => {
                        debug!("received error message from client: {:?}", message);
                        return;
                    }

                    // handle register message
                    Message::Register { services } => {
                        self.handle_client_register(id, services).await
                    }

                    // handle get message
                    Message::Get {
                        client_id,
                        request_id,
                        content,
                    } => self.handle_client_get(client_id, request_id, content).await,

                    // handle set message
                    Message::Set {
                        client_id,
                        request_id,
                        content,
                    } => self.handle_client_set(client_id, request_id, content).await,

                    // handle message
                    Message::Message {
                        to_peer,
                        to_client,
                        from_client,
                        service,
                        content,
                        ..
                    } => {
                        self.handle_client_message(
                            to_peer,
                            to_client,
                            from_client,
                            service,
                            content,
                        )
                        .await
                    }

                    // handle event
                    Message::Event {
                        to_client,
                        from_client,
                        event,
                    } => {
                        self.handle_client_event_msg(to_client, from_client, event)
                            .await;
                        return;
                    }

                    // handle other messages
                    Message::RegisterOk { .. } => return,
                };

                // send reply to client
                if let Some(client) = self.clients.get_mut(&id) {
                    if let Err(e) = client.sender.send(reply).await {
                        error!("handle client error: {}", e);
                        return;
                    }
                }
            }
        }
    }

    /// run the server's main loop
    async fn run_server_loop(&mut self) {
        // start timer
        let timer = time::sleep(Duration::new(5, 0));
        tokio::pin!(timer);

        loop {
            tokio::select! {
                // handle incoming connections
                event = self.server.next().fuse() => {
                    let client = match event {
                        Some(client) => client,
                        None => break,
                    };
                    self.handle_connection(client).await;
                }

                // handle timer event
                event = &mut timer => {
                    debug!("daemon timer event: {:?}", event);
                    timer.as_mut().reset(Instant::now() + Duration::new(5,0));
                    self.handle_timer().await;
                }

                // handle events coming from the swarm
                event = self.swarm.receive().fuse() => {
                    debug!("received event from swarm: {:?}", event);
                    let event = match event {
                        Some(event) => event,
                        None => break,
                    };
                    self.handle_swarm_event(event).await;
                }

                // handle events coming from clients
                event = self.from_client_rx.next().fuse() => {
                    let event = match event {
                        Some(event) => event,
                        None => break,
                    };
                    self.handle_client_event(event).await;
                }
            }
        }
    }

    /// run server
    async fn run(&mut self) {
        // set default name to hostname
        self.name = whoami::fallible::hostname().unwrap_or("localhost".to_string());

        // get options to set from config
        let options = match self.config.command {
            Some(config::Command::Daemon(ref daemon_opts)) => &daemon_opts.set,
            _ => return,
        };

        // handle set options
        for option in options.iter() {
            match option.name.as_str() {
                "name" => {
                    self.name = option.value.clone();
                }
                "connect" => {
                    self.swarm
                        .send(swarm::Event::ConnectAddress(option.value.clone()))
                        .await;
                }
                _ => (),
            }
        }

        // set name in swarm
        self.swarm
            .send(swarm::Event::SetName(self.name.clone()))
            .await;

        // handle server events
        self.run_server_loop().await;
    }
}

/// entry point for running the daemon server
pub async fn run(config: config::Config) {
    // create and run swarm
    let swarm = match swarm::HiSwarm::run().await {
        Ok(swarm) => swarm,
        Err(e) => {
            error!("error creating swarm: {}", e);
            return;
        }
    };

    // create unix server
    let server = match unix_socket::UnixServer::listen(&config).await {
        Ok(server) => server,
        Err(e) => {
            error!("unix socket server error: {}", e);
            return;
        }
    };

    // start service client
    crate::client::service::run(config.clone()).await;

    // start daemon
    Daemon::new(config, server, swarm).await.run().await;
    debug!("daemon stopped");
}
