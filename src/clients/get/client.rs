use crate::config;
use crate::unix_socket;
use async_std::task;

/// get client
struct GetClient {
    _config: config::Config,
    _client: unix_socket::UnixClient,
}

impl GetClient {
    async fn new(_config: config::Config, _client: unix_socket::UnixClient) -> Self {
        GetClient { _config, _client }
    }

    async fn run(&self) {}
}

/// run get client
pub fn run(config: config::Config) {
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => GetClient::new(config, client).await.run().await,
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("chat client stopped");
    });
}
