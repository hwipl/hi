use crate::config;
use crate::message::{Event, Message, Service};
use crate::unix_socket;
use async_std::{io, prelude::*, task};
use futures::future::FutureExt;
use futures::select;
use minicbor::{Decode, Encode};
use std::collections::{HashMap, HashSet};
use std::error::Error;

/// chat message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
struct ChatMessage {
    #[n(0)]
    from: String,
    #[n(1)]
    message: String,
}

/// chat client
struct ChatClient {
    config: config::Config,
    client: unix_socket::UnixClient,
    client_id: u16,
    destination: String,
    peers: HashMap<String, HashSet<u16>>,
}

impl ChatClient {
    /// create new chat client
    pub async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        ChatClient {
            config,
            client,
            client_id: 0,
            destination: String::from("all"),
            peers: HashMap::new(),
        }
    }

    /// register this client
    async fn register_client(&mut self) -> Result<(), Box<dyn Error>> {
        let msg = Message::Register {
            services: vec![Service::Chat as u16].into_iter().collect(),
            chat: true,
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

    /// handle "event" message coming from daemon
    async fn handle_message_event(
        &mut self,
        to_client: u16,
        _from_client: u16,
        event: Event,
    ) -> Result<(), Box<dyn Error>> {
        // make sure event is for us
        if to_client != self.client_id {
            error! {"received event for other client"};
            return Ok(());
        }

        // handle events
        match event {
            Event::ServiceUpdate(service, peers) => {
                // check if service is correct and update peers
                if service == Service::Chat as u16 {
                    self.peers = peers;
                }
            }
            _ => (),
        }
        Ok(())
    }

    /// handle message coming from daemon
    async fn handle_message(&mut self, message: Message) -> Result<(), Box<dyn Error>> {
        match message {
            Message::ChatMessage {
                from,
                from_name,
                message,
                ..
            } => {
                println!("{} <{}>: {}", from, from_name, message);
            }
            Message::Message {
                from_peer, content, ..
            } => {
                if let Ok(msg) = minicbor::decode::<ChatMessage>(&content) {
                    println!("{} <{}>: {}", from_peer, msg.from, msg.message);
                }
            }
            Message::Event {
                to_client,
                from_client,
                event,
            } => {
                self.handle_message_event(to_client, from_client, event)
                    .await?;
            }
            _ => (),
        }
        Ok(())
    }

    /// handle line entered by user
    async fn handle_line(&mut self, line: String) -> Result<(), Box<dyn Error>> {
        let msg = Message::ChatMessage {
            to: self.destination.clone(),
            from: String::new(),
            from_name: String::new(),
            message: line,
        };
        self.client.send_message(msg).await?;
        Ok(())
    }

    /// run client
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        // set chat destination
        self.destination = match &self.config.command {
            Some(config::Command::Chat(opts)) => opts.peer.clone(),
            _ => String::from("all"),
        };

        // register this client and enable chat mode
        self.register_client().await?;

        // enter chat mode
        println!("Chat mode:");
        let mut stdin = io::BufReader::new(io::stdin()).lines();
        loop {
            select! {
                // handle message coming from daemon
                msg = self.client.receive_message().fuse() => {
                    match msg {
                        Ok(msg) => self.handle_message(msg).await?,
                        Err(e) => return Err(e.into()),
                    }
                },

                // handle line read from stdin
                line = stdin.next().fuse() => {
                    if let Some(Ok(line)) = line {
                        if line != "" {
                            self.handle_line(line).await?;
                        }
                    };
                },
            }
        }
    }
}

/// run daemon client in chat mode
pub fn run(config: config::Config) {
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => {
                if let Err(e) = ChatClient::new(config, client).await.run().await {
                    error!("{}", e);
                }
            }
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("chat client stopped");
    });
}
