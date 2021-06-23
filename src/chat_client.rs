use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::{io, prelude::*};
use futures::future::FutureExt;
use futures::select;

/// chat client
struct ChatClient {
    config: config::Config,
    client: unix_socket::UnixClient,
}

impl ChatClient {
    /// create new chat client
    pub async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        ChatClient { config, client }
    }

    pub async fn run(&mut self) {
        // set chat destination
        let destination = match &self.config.command {
            Some(config::Command::Chat(opts)) => opts.peer.clone(),
            _ => String::from("all"),
        };

        // register this client and enable chat mode
        let msg = Message::Register {
            chat: true,
            files: false,
        };
        if let Err(e) = self.client.send_message(msg).await {
            error!("error sending set chat message: {}", e);
            return;
        }
        let _client_id = match self.client.receive_message().await {
            Ok(Message::RegisterOk { client_id }) => client_id,
            Err(e) => {
                error!("error registering client: {}", e);
                return;
            }
            Ok(msg) => {
                error!("unexpected response during client registration: {:?}", msg);
                return;
            }
        };

        // enter chat mode
        println!("Chat mode:");
        let mut stdin = io::BufReader::new(io::stdin()).lines();
        loop {
            select! {
                // handle message coming from daemon
                msg = self.client.receive_message().fuse() => {
                    match msg {
                        Ok(Message::ChatMessage{ from, from_name, message, .. }) => {
                            println!("{} <{}>: {}", from, from_name, message);
                        }
                        _ => continue,
                    }
                },

                // handle line read from stdin
                line = stdin.next().fuse() => {
                    let line = match line {
                        Some(Ok(line)) if line != "" => line,
                        _ => continue,
                    };
                    let msg = Message::ChatMessage {
                        to: destination.clone(),
                        from: String::new(),
                        from_name: String::new(),
                        message: line,
                    };
                    if let Err(e) = self.client.send_message(msg).await {
                        error!("error sending chat message: {}", e);
                        return;
                    }
                },
            }
        }
    }
}

/// run daemon client in chat mode
pub async fn run_chat_client(client: unix_socket::UnixClient, config: config::Config) {
    ChatClient::new(config, client).await.run().await
}
