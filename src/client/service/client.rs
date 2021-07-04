use crate::config;
use crate::message::{Message, Service};
use crate::unix_socket;
use async_std::task;
use std::error::Error;

/// service client
struct ServiceClient {
    _config: config::Config,
    client: unix_socket::UnixClient,
    client_id: u16,
}

impl ServiceClient {
    /// create new service client
    pub async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        ServiceClient {
            _config: config,
            client,
            client_id: 0,
        }
    }

    /// register this client
    async fn register_client(&mut self) -> Result<(), Box<dyn Error>> {
        let msg = Message::Register {
            services: vec![Service::Service as u16].into_iter().collect(),
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

    /// run service client
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.register_client().await?;
        loop {
            let msg = self.client.receive_message().await?;
            debug!("received message {:?}", msg);
        }
    }
}

/// run daemon client in service mode
pub fn run(config: config::Config) {
    task::spawn(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => {
                if let Err(e) = ServiceClient::new(config, client).await.run().await {
                    error!("{}", e);
                }
            }
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("service client stopped");
    });
}
