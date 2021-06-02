use crate::config;
use crate::daemon_message::{FileInfo, Message, PeerInfo};
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
    AddClient(usize, Sender<Message>),
    RemoveClient(usize),
    ClientMessage(usize, Message),
}

/// Client information
struct ClientInfo {
    sender: Sender<Message>,
    chat_support: bool,
    file_support: bool,
}

/// handle client connection identified by its `id`
async fn handle_client(mut server: Sender<Event>, id: usize, mut client: unix_socket::UnixClient) {
    // create channel for server messages and register this client
    let (client_sender, mut client_receiver) = mpsc::unbounded();
    if let Err(e) = server.send(Event::AddClient(id, client_sender)).await {
        eprintln!("handle client error: {}", e);
        return;
    }

    loop {
        select! {
            // handle messages from server
            msg = client_receiver.next().fuse() => {
                match msg {
                    Some(msg) => {
                        // forward message to client
                        println!("received server message: {:?}", msg);
                        if let Err(e) = client.send_message(msg).await {
                            eprintln!("handle client error: {}", e);
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
                        println!("received client message: {:?}", msg);
                        if let Err(e) = server.send(Event::ClientMessage(id, msg)).await {
                            eprintln!("handle client error: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("error receiving client message: {}", e);
                        break;
                    }
                }
            },
        }
    }

    // remove this client
    if let Err(e) = server.send(Event::RemoveClient(id)).await {
        eprintln!("error removing client: {}", e);
        return;
    }
}

/// run the server's main loop
async fn run_server_loop(mut server: Receiver<Event>, mut swarm: swarm::HiSwarm) {
    // clients and their channels
    let mut clients: HashMap<usize, ClientInfo> = HashMap::new();

    // information about known peers
    let mut peers: HashMap<String, PeerInfo> = HashMap::new();

    // shared files
    let mut shared_files = Vec::new();

    // start timer
    let mut timer = Delay::new(Duration::from_secs(5)).fuse();

    loop {
        select! {
            // handle timer event
            event = timer => {
                println!("daemon timer event: {:?}", event);
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
                println!("received event from swarm: {:?}", event);
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
                                    eprintln!("handle client error: {}", e);
                                    return;
                                }
                            }
                        }
                    }

                    // handle file lists
                    swarm::Event::FileList(from, list) => {
                        let mut files = Vec::new();
                        // create file list
                        for file in list {
                            let file = FileInfo {
                                peer_id: from.clone(),
                                name: file.0,
                                size: file.1,
                            };
                            files.push(file);
                        }

                        // send file list to each file client
                        for client in clients.values_mut() {
                            if client.file_support {
                                // send file list to client
                                let msg = Message::GetFiles { files: files.clone() };
                                if let Err(e) = client.sender.send(msg).await {
                                    eprintln!("handle client error: {}", e);
                                    return;
                                }
                            }
                        }
                    }

                    // handle get file list request from other peer
                    swarm::Event::ReceivedGetFiles(from, channel) => {
                        println!("received get files request from peer {}", from);
                        let response = swarm::Event::SendFileList(channel, shared_files.clone());
                        swarm.send(response).await;
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
                        println!("received add client event with id {}", id);
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
                        println!("received remove client event with id {}", id);
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
                        println!("received message from client: {:?}", msg);

                        // get client channel
                        let client = match clients.get_mut(&id) {
                            Some(client) => client,
                            None => {
                                eprintln!("unknown client");
                                continue;
                            }
                        };

                        // parse message and generate reply message
                        let reply = match msg {
                            // handle OK message
                            Message::Ok => Message::Ok,

                            // handle error message
                            Message::Error { message } => {
                                println!("received error message from client: {:?}", message);
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

                            // handle set chat request
                            Message::SetChat { enabled } => {
                                client.chat_support = true;
                                let event = swarm::Event::SetChat(enabled);
                                swarm.send(event).await;
                                Message::Ok
                            }

                            // handle chat message
                            Message::ChatMessage { to, message, .. } => {
                                println!("received chat message for {}: {}", to, message);
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

                            // handle set files request
                            Message::SetFiles { enabled } => {
                                client.file_support = enabled;
                                let event = swarm::Event::SetFiles(enabled);
                                swarm.send(event).await;
                                Message::Ok
                            }

                            // handle get files message
                            Message::GetFiles { .. } => {
                                for peer in peers.values() {
                                    if peer.file_support {
                                        let event = swarm::Event::SendGetFiles(
                                            peer.peer_id.clone());
                                            swarm.send(event).await;
                                    }
                                }
                                Message::Ok
                            }

                            // handle shared files message
                            Message::ShareFiles { files, .. } => {
                                shared_files = Vec::new();
                                for f in files {
                                    shared_files.push((f.name, f.size));
                                }
                                Message::Ok
                            }
                        };

                        // send reply to client
                        if let Err(e) = client.sender.send(reply).await {
                            eprintln!("handle client error: {}", e);
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
        let mut id = 0;
        while let Some(client) = server.next().await {
            task::spawn(handle_client(server_sender.clone(), id, client));
            id = id.wrapping_add(1);
        }
    });

    // create and run swarm
    let mut swarm = match swarm::HiSwarm::run().await {
        Ok(swarm) => swarm,
        Err(e) => {
            eprintln!("error creating swarm: {}", e);
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
        match unix_socket::UnixServer::listen().await {
            Ok(server) => run_server(config, server).await,
            Err(e) => eprintln!("unix socket server error: {}", e),
        };
        println!("unix socket server stopped");
    });
}
