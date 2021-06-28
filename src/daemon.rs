mod behaviour;
mod gossip;
mod request;
mod swarm;

use crate::config;
use crate::daemon_message::{GetSet, Message, PeerInfo};
use crate::unix_socket;
use async_std::prelude::*;
use async_std::task;
use futures::channel::mpsc;
use futures::future::FutureExt;
use futures::select;
use futures::sink::SinkExt;
use std::collections::hash_map::{Entry, HashMap};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wasm_timer::Delay;

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
    chat_support: bool,
    file_support: bool,
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
            select! {
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
        task::spawn(Self::handle_client(
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

    /// run the server's main loop
    async fn run_server_loop(&mut self) {
        // start timer
        let mut timer = Delay::new(Duration::from_secs(5)).fuse();

        loop {
            select! {
                // handle incoming connections
                event = self.server.next().fuse() => {
                    let client = match event {
                        Some(client) => client,
                        None => break,
                    };
                    self.handle_connection(client).await;
                }

                // handle timer event
                event = timer => {
                    debug!("daemon timer event: {:?}", event);
                    timer = Delay::new(Duration::from_secs(5)).fuse();
                    self.handle_timer().await;
                }

                // handle events coming from the swarm
                event = self.swarm.receive().fuse() => {
                    debug!("received event from swarm: {:?}", event);
                    let event = match event {
                        Some(event) => event,
                        None => break,
                    };

                    match event {
                        // handle peer announcement
                        swarm::Event::AnnouncePeer(peer_info) => {
                            // add or update peer entry
                            match self.peers.entry(peer_info.peer_id.clone()) {
                                Entry::Occupied(mut entry) => {
                                    entry.insert(peer_info);
                                }
                                Entry::Vacant(entry) => {
                                    entry.insert(peer_info);
                                }
                            }
                        }

                        // handle chat messages
                        swarm::Event::ChatMessage(from, msg) => {
                            for client in self.clients.values_mut() {
                                if client.chat_support {
                                    // send msg to client
                                    let from_name = match self.peers.get(&from) {
                                        Some(peer) => peer.name.clone(),
                                        None => String::new(),
                                    };
                                    let msg =  Message::ChatMessage {
                                        to: String::new(),
                                        from: from.clone(),
                                        from_name,
                                        message: msg.clone(),
                                    };
                                    if let Err(e) = client.sender.send(msg).await {
                                        error!("handle client error: {}", e);
                                        return;
                                    }
                                }
                            }
                        }

                        // handle file messages
                        swarm::Event::FileMessage(from_peer, from_client, to_client, content) => {
                            // helper for sending message to a client
                            async fn send(
                                client: &mut ClientInfo,
                                from_peer: String,
                                to_client: u16,
                                from_client: u16,
                                content: Vec<u8>,
                            ) {
                                let msg =  Message::FileMessage {
                                    to_peer: String::new(),
                                    from_peer,
                                    to_client,
                                    from_client,
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
                                    if client.file_support {
                                        send(client, from_peer.clone(), to_client, from_client,
                                            content.clone()).await;
                                    }
                                }
                                continue;
                            }

                            // handle message to specific client
                            if self.clients.contains_key(&to_client) {
                                let client = self.clients.get_mut(&to_client).unwrap();
                                send(client, from_peer.clone(), to_client, from_client,
                                    content.clone()).await;
                            }
                        }

                        // handle other events
                        _ => (),
                    }
                }

                // handle events coming from clients
                event = self.from_client_rx.next().fuse() => {
                    let event = match event {
                        Some(event) => event,
                        None => break,
                    };
                    match event {
                        // handle add client
                        Event::AddClient(id, sender) => {
                            debug!("received add client event with id {}", id);
                            match self.clients.entry(id) {
                                Entry::Occupied(..) => (),
                                Entry::Vacant(entry) => {
                                    let client_info = ClientInfo {
                                        sender,
                                        chat_support: false,
                                        file_support: false,
                                    };
                                    entry.insert(client_info);
                                }
                            }
                        }

                        // handle remove client
                        Event::RemoveClient(id) => {
                            debug!("received remove client event with id {}", id);
                            self.clients.remove(&id);

                            // check if there are still clients with chat support
                            // and with file support
                            let mut chat_support = false;
                            let mut file_support = false;
                            for c in self.clients.values() {
                                chat_support |= c.chat_support;
                                file_support |= c.file_support;
                            }

                            let event = swarm::Event::SetChat(chat_support);
                            self.swarm.send(event).await;
                            let event = swarm::Event::SetFiles(file_support);
                            self.swarm.send(event).await;

                        }

                        // handle client message
                        Event::ClientMessage(id, msg) => {
                            debug!("received message from client: {:?}", msg);

                            // get client channel
                            let client = match self.clients.get_mut(&id) {
                                Some(client) => client,
                                None => {
                                    error!("unknown client");
                                    continue;
                                }
                            };

                            // parse message and generate reply message
                            let reply = match msg {
                                // handle OK message
                                Message::Ok => Message::Ok,

                                // handle error message
                                Message::Error { message } => {
                                    debug!("received error message from client: {:?}", message);
                                    continue;
                                }

                                // handle connect address request
                                Message::ConnectAddress { address } => {
                                    let event = swarm::Event::ConnectAddress(address);
                                    self.swarm.send(event).await;
                                    Message::Ok
                                }

                                // handle get name request
                                Message::GetName { .. } => {
                                    let message = String::from("Not yet implemented");
                                    Message::Error { message }
                                }

                                // handle set name request
                                Message::SetName { name } => {
                                    let event = swarm::Event::SetName(name);
                                    self.swarm.send(event).await;
                                    Message::Ok
                                }

                                // handle get peers request
                                Message::GetPeers { .. } => {
                                    let peer_infos = self.peers.values().cloned().collect();
                                    Message::GetPeers { peers: peer_infos }
                                }

                                // handle chat message
                                Message::ChatMessage { to, message, .. } => {
                                    debug!("received chat message for {}: {}", to, message);
                                    if to == "all" {
                                        // send message to all known peers with chat support
                                        for peer in self.peers.values() {
                                            if peer.chat_support {
                                                let event = swarm::Event::SendChatMessage(
                                                    peer.peer_id.clone(),
                                                    message.clone());
                                                    self.swarm.send(event).await;
                                            }
                                        }
                                    } else {
                                        // send message to peer specified in `to`
                                        let event = swarm::Event::SendChatMessage(to, message);
                                        self.swarm.send(event).await;
                                    }
                                    Message::Ok
                                }

                                // handle file message
                                Message::FileMessage {
                                    to_peer,
                                    to_client,
                                    from_client,
                                    content,
                                    ..
                                } => {
                                    debug!("received file message for {}", to_peer);
                                    if to_peer == "all" {
                                        // send message to all known peers with file support
                                        for peer in self.peers.values() {
                                            if peer.file_support {
                                                let event = swarm::Event::SendFileMessage(
                                                    peer.peer_id.clone(),
                                                    to_client,
                                                    from_client,
                                                    content.clone());
                                                self.swarm.send(event).await;
                                            }
                                        }
                                    } else {
                                        // send message to peer specified in `to`
                                        let event = swarm::Event::SendFileMessage(
                                            to_peer,
                                            to_client,
                                            from_client,
                                            content);
                                        self.swarm.send(event).await;
                                    }
                                    Message::Ok
                                }

                                // handle register message
                                Message::Register { chat, files } => {
                                    client.chat_support = chat;
                                    let event = swarm::Event::SetChat(chat);
                                    self.swarm.send(event).await;
                                    client.file_support = files;
                                    let event = swarm::Event::SetFiles(files);
                                    self.swarm.send(event).await;
                                    Message::RegisterOk{ client_id: id }
                                }

                                // handle get message
                                Message::Get { client_id, request_id, content } => {
                                    let content = match content {
                                        GetSet::Name(..) => {
                                            GetSet::Error(String::from("Not yet implemented"))
                                        }
                                        GetSet::Peers(..) => {
                                            GetSet::Peers(self.peers.values().cloned().collect())
                                        }
                                        _ => {
                                            GetSet::Error(String::from("Unknown get request"))
                                        }
                                    };
                                    Message::Get {
                                        client_id,
                                        request_id,
                                        content,
                                    }
                                }

                                // handle set message
                                Message::Set { client_id, request_id, content } => {
                                    let content = match content {
                                        GetSet::Name(name) => {
                                            let event = swarm::Event::SetName(name);
                                            self.swarm.send(event).await;
                                            GetSet::Ok
                                        }
                                        GetSet::Connect(address) => {
                                            let event = swarm::Event::ConnectAddress(address);
                                            self.swarm.send(event).await;
                                            GetSet::Ok
                                        }
                                        _ => {
                                            GetSet::Error(String::from("Unknown set request"))
                                        }
                                    };
                                    Message::Set {
                                        client_id,
                                        request_id,
                                        content,
                                    }
                                }

                                // handle other messages
                                Message::RegisterOk { .. } => continue,
                            };

                            // send reply to client
                            if let Err(e) = client.sender.send(reply).await {
                                error!("handle client error: {}", e);
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    /// run server
    async fn run(&mut self) {
        // get options to set from config
        let options = match self.config.command {
            Some(config::Command::Daemon(ref daemon_opts)) => &daemon_opts.set,
            _ => return,
        };

        // handle set options
        for option in options.iter() {
            match option.name.as_str() {
                "name" => {
                    self.swarm
                        .send(swarm::Event::SetName(option.value.clone()))
                        .await;
                }
                "connect" => {
                    self.swarm
                        .send(swarm::Event::ConnectAddress(option.value.clone()))
                        .await;
                }
                _ => (),
            }
        }

        // handle server events
        self.run_server_loop().await;
    }
}

/// entry point for running the daemon server
pub fn run(config: config::Config) {
    task::block_on(async {
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

        // start daemon
        Daemon::new(config, server, swarm).await.run().await;
        debug!("daemon stopped");
    });
}
