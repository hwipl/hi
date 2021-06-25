use crate::config;
use crate::unix_socket;
use async_std::task;

/// run daemon client with config
async fn run_client(_config: config::Config, _client: unix_socket::UnixClient) {}

/// initialize client connection and run daemon client
pub fn run(config: config::Config) {
    // run unix socket client
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(mut client) => {
                if let Err(e) = client.test().await {
                    error!("unix socket test error: {}", e);
                }
                run_client(config, client).await;
            }
            Err(e) => {
                error!("unix socket client error: {}", e);
            }
        }
        debug!("unix socket client stopped");
    });
}
