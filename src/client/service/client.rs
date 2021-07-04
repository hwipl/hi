use crate::config;
use crate::unix_socket;
use async_std::task;
use std::error::Error;

/// service client
struct ServiceClient {
    _config: config::Config,
    client: unix_socket::UnixClient,
}

impl ServiceClient {
    /// create new service client
    pub async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        ServiceClient {
            _config: config,
            client,
        }
    }

    /// run service client
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
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
