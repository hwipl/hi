use crate::config;
use crate::message::{Event, Message, Service};
use crate::unix_socket;
use chrono::Local;
use futures::future::FutureExt;
use minicbor::{Decode, Encode};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use tokio::io;
use tokio::io::AsyncBufReadExt;

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
    name: String,
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
            name: String::new(),
            destination: String::from("all"),
            peers: HashMap::new(),
        }
    }

    /// register this client
    async fn register_client(&mut self) -> Result<(), Box<dyn Error>> {
        let msg = Message::Register {
            services: vec![Service::Chat as u16].into_iter().collect(),
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

    /// handle "message" message coming from daemon
    async fn handle_message_message(
        &mut self,
        from_peer: String,
        from_client: u16,
        service: u16,
        content: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        if service != Service::Chat as u16 {
            return Ok(());
        }
        if let Ok(msg) = minicbor::decode::<ChatMessage>(&content) {
            let now = Local::now();
            println!(
                "{}: {}/{} <{}>: {}",
                now.format("%H:%M:%S"),
                from_peer,
                from_client,
                msg.from,
                msg.message
            );
        }
        Ok(())
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
            Message::Message {
                from_peer,
                from_client,
                service,
                content,
                ..
            } => {
                self.handle_message_message(from_peer, from_client, service, content)
                    .await?;
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
        // create chat message
        let mut content = Vec::new();
        let message = ChatMessage {
            from: self.name.clone(),
            message: line,
        };
        minicbor::encode(message, &mut content)?;

        // send message to everyone
        if self.destination == "all" {
            for (peer, clients) in self.peers.iter() {
                for client in clients.iter() {
                    let msg = Message::Message {
                        to_peer: peer.clone(),
                        from_peer: "".into(),
                        to_client: *client,
                        from_client: self.client_id,
                        service: Service::Chat as u16,
                        content: content.clone(),
                    };
                    self.client.send_message(msg).await?;
                }
            }
            return Ok(());
        }

        // send message to specific peer
        if self.peers.contains_key(&self.destination) {
            let clients = self.peers.get(&self.destination).unwrap();
            for client in clients.iter() {
                let msg = Message::Message {
                    to_peer: self.destination.clone(),
                    from_peer: "".into(),
                    to_client: *client,
                    from_client: self.client_id,
                    service: Service::Chat as u16,
                    content: content.clone(),
                };
                self.client.send_message(msg).await?;
            }
            return Ok(());
        }
        Ok(())
    }

    /// run client
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        // apply user settings
        if let Some(config::Command::Chat(opts)) = &self.config.command {
            // set chat user name
            if let Some(ref name) = opts.name {
                self.name = name.clone();
            } else {
                self.name = whoami::username();
            };

            // set chat destination
            self.destination = opts.peer.clone();
        };

        // register this client and enable chat mode
        self.register_client().await?;

        // enter chat mode
        println!("Chat mode:");
        let mut stdin = io::BufReader::new(io::stdin()).lines();
        loop {
            tokio::select! {
                // handle message coming from daemon
                msg = self.client.receive_message().fuse() => {
                    match msg {
                        Ok(msg) => self.handle_message(msg).await?,
                        Err(e) => return Err(e.into()),
                    }
                },

                // handle line read from stdin
                line = stdin.next_line().fuse() => {
                    if let Ok(Some(line)) = line {
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
pub async fn run(config: config::Config) {
    match unix_socket::UnixClient::connect(&config).await {
        Ok(client) => {
            if let Err(e) = ChatClient::new(config, client).await.run().await {
                error!("{}", e);
            }
        }
        Err(e) => error!("unix socket client error: {}", e),
    }
    debug!("chat client stopped");
}
