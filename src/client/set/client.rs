use crate::config;
use crate::unix_socket;
use async_std::task;

/// set client
struct SetClient {
    _config: config::Config,
    _client: unix_socket::UnixClient,
}

impl SetClient {
    async fn new(_config: config::Config, _client: unix_socket::UnixClient) -> Self {
        SetClient { _config, _client }
    }

    async fn run(&self) {}
}

/// run set client
pub fn run(config: config::Config) {
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => SetClient::new(config, client).await.run().await,
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("set client stopped");
    });
}
