use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::task;
use std::error::Error;

/// get client
struct GetClient {
    config: config::Config,
    client: unix_socket::UnixClient,
    client_id: u16,
}

impl GetClient {
    /// create new get client
    async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        GetClient {
            config,
            client,
            client_id: 0,
        }
    }

    /// register this client
    async fn register_client(&mut self) -> Result<(), Box<dyn Error>> {
        let msg = Message::Register {
            chat: false,
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

    /// run get client
    async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        // register client
        self.register_client().await?;

        // get options to get from config
        let options = match self.config.command {
            Some(config::Command::Get(ref get_opts)) => &get_opts.info,
            _ => return Err("invalid config".into()),
        };

        // handle get configuration options
        for option in options.iter() {
            let msg = match option.as_str() {
                "name" => Message::GetName {
                    name: String::from(""),
                },
                "peers" => Message::GetPeers { peers: Vec::new() },
                _ => {
                    error!("error getting unknown configuration option: {}", option);
                    continue;
                }
            };

            // send message
            self.client.send_message(msg).await?;

            // receive reply
            match self.client.receive_message().await? {
                Message::GetName { name } => println!("Name: {}", name),
                Message::GetPeers { peers } => {
                    println!("Peers:");
                    for peer in peers {
                        println!("  peer_id: {}, name: {:?}, chat_support: {}, file_support: {}, last_update: {}",
                            peer.peer_id,
                            peer.name,
                            peer.chat_support,
                            peer.file_support,
                            peer.last_update
                            );
                    }
                }
                Message::Error { message } => println!("Error: {}", message),
                msg => println!("{:?}", msg),
            }
        }
        Ok(())
    }
}

/// run get client
pub fn run(config: config::Config) {
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => {
                if let Err(e) = GetClient::new(config, client).await.run().await {
                    error!("{}", e);
                }
            }
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("get client stopped");
    });
}
