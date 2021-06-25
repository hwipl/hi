use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::task;

/// run daemon client with config
async fn run_client(config: config::Config, mut client: unix_socket::UnixClient) {
    // handle get configuration options in config
    for option in config.get.iter() {
        let msg = match option.name.as_str() {
            "name" => Message::GetName {
                name: String::from(""),
            },
            "peers" => Message::GetPeers { peers: Vec::new() },
            _ => {
                error!(
                    "error getting unknown configuration option: {}",
                    option.name
                );
                continue;
            }
        };

        // send message
        if let Err(e) = client.send_message(msg).await {
            error!("error sending get message: {}", e);
        }

        // receive reply
        match client.receive_message().await {
            Ok(msg) => debug!("get reply from server: {:?}", msg),
            Err(e) => error!("error receiving get reply: {}", e),
        }
    }
}

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
