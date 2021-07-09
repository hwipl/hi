use crate::config;
use crate::message::{Message, Service};
use crate::unix_socket;
use async_std::{io, prelude::*, task};
use futures::future::FutureExt;
use futures::select;
use std::error::Error;

/// chat client
struct ChatClient {
    config: config::Config,
    client: unix_socket::UnixClient,
    _client_id: u16,
    destination: String,
}

impl ChatClient {
    /// create new chat client
    pub async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        ChatClient {
            config,
            client,
            _client_id: 0,
            destination: String::from("all"),
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
                self._client_id = client_id;
                Ok(())
            }
            _ => Err("unexpected message from daemon".into()),
        }
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
