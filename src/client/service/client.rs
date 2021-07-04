use crate::config;
use crate::unix_socket;
use async_std::task;

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

    pub async fn run(&mut self) {
        while let Ok(msg) = self.client.receive_message().await {
            debug!("received message {:?}", msg);
        }
    }
}

/// run daemon client in service mode
pub fn run(config: config::Config) {
    task::spawn(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => ServiceClient::new(config, client).await.run().await,
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("service client stopped");
    });
}
