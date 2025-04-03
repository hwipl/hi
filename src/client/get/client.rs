use crate::config;
use crate::message::{GetSet, Message};
use crate::unix_socket;
use std::collections::HashSet;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

/// get client
struct GetClient {
    config: config::Config,
    client: unix_socket::UnixClient,
    client_id: u16,
    request_id: u32,
}

impl GetClient {
    /// create new get client
    async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        GetClient {
            config,
            client,
            client_id: 0,
            request_id: 0,
        }
    }

    /// register this client
    async fn register_client(&mut self) -> Result<(), Box<dyn Error>> {
        let msg = Message::Register {
            services: HashSet::new(),
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

    /// send get request
    async fn send_request(&mut self, content: GetSet) -> Result<(), Box<dyn Error>> {
        let msg = Message::Get {
            client_id: self.client_id,
            request_id: self.request_id,
            content,
        };
        self.client.send_message(msg).await?;
        self.request_id = self.request_id.wrapping_add(1);
        Ok(())
    }

    /// handle content of a get reply
    async fn handle_reply_content(&self, content: GetSet) {
        let current_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp error")
            .as_secs();
        match content {
            GetSet::Name(name) => println!("Name: {}", name),
            GetSet::Peers(peers) => {
                println!("Peers:");
                for peer in peers {
                    println!(
                        "  \
                        peer_id: {}, \
                        name: {:?}, \
                        services_tag: {}, \
                        last_update: {}s",
                        peer.peer_id,
                        peer.name,
                        peer.services_tag,
                        current_secs - peer.last_update,
                    );
                }
            }
            GetSet::Error(e) => eprintln!("Error: {}", e),
            _ => println!("{:?}", content),
        }
    }

    /// handle get reply
    async fn handle_reply(&mut self) -> Result<(), Box<dyn Error>> {
        match self.client.receive_message().await? {
            Message::Get { content, .. } => self.handle_reply_content(content).await,
            msg => println!("{:?}", msg),
        }
        Ok(())
    }

    /// run get client
    async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        // register client
        self.register_client().await?;

        // get options to get from config
        let options = match self.config.command {
            Some(config::Command::Get(ref get_opts)) => get_opts.info.clone(),
            _ => return Err("invalid config".into()),
        };

        // handle get configuration options
        for option in options.iter() {
            let content = match option.as_str() {
                "name" => GetSet::Name(String::new()),
                "peers" => GetSet::Peers(Vec::new()),
                _ => {
                    error!("error getting unknown configuration option: {}", option);
                    continue;
                }
            };
            self.send_request(content).await?;
            self.handle_reply().await?;
        }
        Ok(())
    }
}

/// run get client
pub async fn run(config: config::Config) {
    match unix_socket::UnixClient::connect(&config).await {
        Ok(client) => {
            if let Err(e) = GetClient::new(config, client).await.run().await {
                error!("{}", e);
            }
        }
        Err(e) => error!("unix socket client error: {}", e),
    }
    debug!("get client stopped");
}
