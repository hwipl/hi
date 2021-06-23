use crate::config;
use crate::daemon_message::{Message, PeerInfo};
use crate::swarm;
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

/// handle client connection identified by its `id`
async fn handle_client(mut server: Sender<Event>, id: u16, mut client: unix_socket::UnixClient) {
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

/// run the server's main loop
async fn run_server_loop(mut server: Receiver<Event>, mut swarm: swarm::HiSwarm) {
    // clients and their channels
    let mut clients: HashMap<u16, ClientInfo> = HashMap::new();

    // information about known peers
    let mut peers: HashMap<String, PeerInfo> = HashMap::new();

    // start timer
    let mut timer = Delay::new(Duration::from_secs(5)).fuse();

    loop {
        select! {
            // handle timer event
            event = timer => {
                debug!("daemon timer event: {:?}", event);
                timer = Delay::new(Duration::from_secs(5)).fuse();

                // remove old entries from peers hash map
                let current_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("timestamp error")
                    .as_secs();
                let mut remove_peers = Vec::new();
                for peer in peers.values() {
                    if current_secs - peer.last_update > 30 {
                        remove_peers.push(peer.peer_id.clone());
                    }
                }
                for peer in remove_peers {
                    peers.remove(&peer);
                }
            }

            // handle events coming from the swarm
            event = swarm.receive().fuse() => {
                debug!("received event from swarm: {:?}", event);
                let event = match event {
                    Some(event) => event,
                    None => break,
                };

                match event {
                    // handle peer announcement
                    swarm::Event::AnnouncePeer(peer_info) => {
                        // add or update peer entry
                        match peers.entry(peer_info.peer_id.clone()) {
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
                        for client in clients.values_mut() {
                            if client.chat_support {
                                // send msg to client
                                let from_name = match peers.get(&from) {
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
                        if to_client == Message::ALL_CLIENTS {
                            for client in clients.values_mut() {
                                if client.file_support {
                                    // send message to client
                                    let msg =  Message::FileMessage {
                                        to_peer: String::new(),
                                        from_peer: from_peer.clone(),
                                        to_client,
                                        from_client,
                                        content: content.clone(),
                                    };
                                    if let Err(e) = client.sender.send(msg).await {
                                        error!("handle client error: {}", e);
                                        return;
                                    }
                                }
                            }
                            continue;
                        }
                        if clients.contains_key(&to_client) {
                            let client = clients.get_mut(&to_client).unwrap();
                            let msg =  Message::FileMessage {
                                to_peer: String::new(),
                                from_peer: from_peer.clone(),
                                to_client,
                                from_client,
                                content: content.clone(),
                            };
                            if let Err(e) = client.sender.send(msg).await {
                                error!("handle client error: {}", e);
                                return;
                            }
                        }
                    }

                    // handle other events
                    _ => (),
                }
            }

            // handle events coming from clients
            event = server.next().fuse() => {
                let event = match event {
                    Some(event) => event,
                    None => break,
                };
                match event {
                    // handle add client
                    Event::AddClient(id, sender) => {
                        debug!("received add client event with id {}", id);
                        match clients.entry(id) {
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
                        clients.remove(&id);

                        // check if there are still clients with chat support
                        // and with file support
                        let mut chat_support = false;
                        let mut file_support = false;
                        for c in clients.values() {
                            chat_support |= c.chat_support;
                            file_support |= c.file_support;
                        }

                        let event = swarm::Event::SetChat(chat_support);
                        swarm.send(event).await;
                        let event = swarm::Event::SetFiles(file_support);
                        swarm.send(event).await;

                    }

                    // handle client message
                    Event::ClientMessage(id, msg) => {
                        debug!("received message from client: {:?}", msg);

                        // get client channel
                        let client = match clients.get_mut(&id) {
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
                                swarm.send(event).await;
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
                                swarm.send(event).await;
                                Message::Ok
                            }

                            // handle get peers request
                            Message::GetPeers { .. } => {
                                let peer_infos = peers.values().cloned().collect();
                                Message::GetPeers { peers: peer_infos }
                            }

                            // handle chat message
                            Message::ChatMessage { to, message, .. } => {
                                debug!("received chat message for {}: {}", to, message);
                                if to == "all" {
                                    // send message to all known peers with chat support
                                    for peer in peers.values() {
                                        if peer.chat_support {
                                            let event = swarm::Event::SendChatMessage(
                                                peer.peer_id.clone(),
                                                message.clone());
                                                swarm.send(event).await;
                                        }
                                    }
                                } else {
                                    // send message to peer specified in `to`
                                    let event = swarm::Event::SendChatMessage(to, message);
                                    swarm.send(event).await;
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
                                    for peer in peers.values() {
                                        if peer.file_support {
                                            let event = swarm::Event::SendFileMessage(
                                                peer.peer_id.clone(),
                                                to_client,
                                                from_client,
                                                content.clone());
                                            swarm.send(event).await;
                                        }
                                    }
                                } else {
                                    // send message to peer specified in `to`
                                    let event = swarm::Event::SendFileMessage(
                                        to_peer,
                                        to_client,
                                        from_client,
                                        content);
                                    swarm.send(event).await;
                                }
                                Message::Ok
                            }

                            // handle register message
                            Message::Register { chat, files } => {
                                client.chat_support = chat;
                                let event = swarm::Event::SetChat(chat);
                                swarm.send(event).await;
                                client.file_support = files;
                                let event = swarm::Event::SetFiles(files);
                                swarm.send(event).await;
                                Message::RegisterOk{ client_id: id }
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
async fn run_server(config: config::Config, server: unix_socket::UnixServer) {
    // create channels and data structure for clients
    let (server_sender, server_receiver) = mpsc::unbounded();

    // handle incoming connections
    task::spawn(async move {
        let mut id = 1;
        while let Some(client) = server.next().await {
            task::spawn(handle_client(server_sender.clone(), id, client));
            id += 1;
            // skip ids for ALL_CLIENTS and 0
            if id == Message::ALL_CLIENTS {
                id = 1;
            }
        }
    });

    // create and run swarm
    let mut swarm = match swarm::HiSwarm::run().await {
        Ok(swarm) => swarm,
        Err(e) => {
            error!("error creating swarm: {}", e);
            return;
        }
    };

    // handle set options in config
    for option in config.set {
        if option.name == "name" {
            swarm.send(swarm::Event::SetName(option.value)).await;
        }
    }

    // handle connect addresses in config
    for addr in config.connect {
        swarm.send(swarm::Event::ConnectAddress(addr)).await;
    }

    // handle server events
    task::block_on(run_server_loop(server_receiver, swarm));
}

/// entry point for running the daemon server
pub fn run(config: config::Config) {
    // run unix socket server
    task::block_on(async {
        match unix_socket::UnixServer::listen(&config).await {
            Ok(server) => run_server(config, server).await,
            Err(e) => error!("unix socket server error: {}", e),
        };
        debug!("unix socket server stopped");
    });
}
