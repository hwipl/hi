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

type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

/// Daemon events
enum Event {
    AddClient(usize, Sender<Message>),
    RemoveClient(usize),
    ClientMessage(usize, Message),
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
    let mut clients: HashMap<usize, Sender<Message>> = HashMap::new();

    // information about known peers
    let mut peers: HashMap<String, String> = HashMap::new();

    loop {
        select! {
            // handle events coming from the swarm
            event = swarm.receive().fuse() => {
                println!("received event from swarm: {:?}", event);
                let event = match event {
                    Some(event) => event,
                    None => break,
                };

                match event {
                    // handle peer announcement
                    swarm::Event::AnnouncePeer(peer_id, name) => {
                        // add or update peer entry
                        match peers.entry(peer_id) {
                            Entry::Occupied(mut entry) => {
                                *entry.get_mut() = name;
                            }
                            Entry::Vacant(entry) => {
                                entry.insert(name);
                            }
                        }
                    }
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
                                entry.insert(sender);
                            }
                        }
                    }

                    // handle remove client
                    Event::RemoveClient(id) => {
                        println!("received remove client event with id {}", id);
                        clients.remove(&id);
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
                                let mut peer_infos = Vec::new();
                                for p in &peers {
                                    peer_infos.push(PeerInfo {
                                        peer_id: p.0.clone(),
                                        name: p.1.clone(),
                                        chat_support: false,
                                    });
                                }
                                Message::GetPeers { peers: peer_infos }
                            }

                            // handle set chat request
                            Message::SetChat { enabled } => {
                                let event = swarm::Event::SetChat(enabled);
                                swarm.send(event).await;
                                Message::Ok
                            }
                        };

                        // send reply to client
                        if let Err(e) = client.send(reply).await {
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
