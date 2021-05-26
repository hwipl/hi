use crate::config;
use crate::daemon_message::Message;
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
    ClientMessage(usize, Message),
}

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
                            return;
                        }
                    }
                    None => return,
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
                            return;
                        }
                    }
                    Err(e) => {
                        eprintln!("error receiving client message: {}", e);
                        return;
                    }
                }
            },
        }
    }
}

async fn run_server_loop(mut server: Receiver<Event>) {
    // clients and their channels
    let mut clients: HashMap<usize, Sender<Message>> = HashMap::new();

    while let Some(msg) = server.next().await {
        match msg {
            Event::AddClient(id, sender) => match clients.entry(id) {
                Entry::Occupied(..) => (),
                Entry::Vacant(entry) => {
                    entry.insert(sender);
                }
            },
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

                match msg {
                    // handle OK message
                    Message::Ok => {
                        if let Err(e) = client.send(Message::Ok).await {
                            eprintln!("handle client error: {}", e);
                            return;
                        }
                    }

                    // handle error message
                    Message::Error { message } => {
                        println!("received error message from client: {:?}", message);
                    }

                    // handle connect address request
                    Message::ConnectAddress { .. } => {
                        let message = String::from("Not yet implemented");
                        let error = Message::Error { message };
                        if let Err(e) = client.send(error).await {
                            eprintln!("handle client error: {}", e);
                            return;
                        }
                    }

                    // handle get name request
                    Message::GetName { .. } => {
                        let message = String::from("Not yet implemented");
                        let error = Message::Error { message };
                        if let Err(e) = client.send(error).await {
                            eprintln!("handle client error: {}", e);
                            return;
                        }
                    }

                    // handle set name request
                    Message::SetName { .. } => {
                        let message = String::from("Not yet implemented");
                        let error = Message::Error { message };
                        if let Err(e) = client.send(error).await {
                            eprintln!("handle client error: {}", e);
                            return;
                        }
                    }
                }
            }
        }
    }
}

async fn run_server(config: config::Config, server: unix_socket::UnixServer) {
    // create channels and data structure for clients
    let (server_sender, server_receiver) = mpsc::unbounded();

    // handle incoming connections
    task::spawn(async move {
        let mut id = 0;
        while let Some(client) = server.next().await {
            task::spawn(handle_client(server_sender.clone(), id, client));
            id += 1;
        }
    });

    // handle server events
    task::spawn(run_server_loop(server_receiver));

    // run swarm
    if let Err(e) = swarm::run(config.connect) {
        eprintln!("swarm error: {}", e);
    }
    println!("swarm stopped");
}

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
