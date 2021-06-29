use crate::config;
use crate::daemon_message::{GetSet, Message};
use crate::unix_socket;
use async_std::task;
use std::error::Error;

/// set client
struct SetClient {
    config: config::Config,
    client: unix_socket::UnixClient,
    client_id: u16,
    request_id: u32,
}

impl SetClient {
    /// create new set client
    async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        SetClient {
            config,
            client,
            client_id: 0,
            request_id: 0,
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

    /// send set request
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

    /// run set client
    async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        // register client
        self.register_client().await?;

        // get options to set from config
        let options = match self.config.command {
            Some(config::Command::Set(ref set_opts)) => set_opts.opts.clone(),
            _ => return Err("invalid config".into()),
        };

        // handle set configuration options
        for option in options.iter() {
            // create message
            let content = match option.name.as_str() {
                "name" => GetSet::Name(option.value.to_string()),
                "connect" => GetSet::Connect(option.value.to_string()),
                _ => {
                    error!(
                        "error setting unknown configuration option: {}",
                        option.name
                    );
                    continue;
                }
            };

            // send message
            self.send_request(content).await?;

            // receive reply
            let msg = self.client.receive_message().await?;
            debug!("set reply from server: {:?}", msg);
        }
        Ok(())
    }
}

/// run set client
pub fn run(config: config::Config) {
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => {
                if let Err(e) = SetClient::new(config, client).await.run().await {
                    error!("{}", e);
                }
            }
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("set client stopped");
    });
}
