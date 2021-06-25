use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::task;

/// set client
struct SetClient {
    config: config::Config,
    client: unix_socket::UnixClient,
}

impl SetClient {
    /// create new set client
    async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        SetClient { config, client }
    }

    /// run set client
    async fn run(&mut self) {
        // get options to set from config
        let options = match self.config.command {
            Some(config::Command::Set(ref set_opts)) => &set_opts.opts,
            _ => return,
        };

        // handle set configuration options
        for option in options.iter() {
            // create message
            let msg = match option.name.as_str() {
                "name" => Message::SetName {
                    name: option.value.to_string(),
                },

                "connect" => Message::ConnectAddress {
                    address: option.value.to_string(),
                },

                _ => {
                    error!(
                        "error setting unknown configuration option: {}",
                        option.name
                    );
                    continue;
                }
            };

            // send message
            if let Err(e) = self.client.send_message(msg).await {
                error!("error sending set message: {}", e);
            }

            // receive reply
            match self.client.receive_message().await {
                Ok(msg) => debug!("set reply from server: {:?}", msg),
                Err(e) => error!("error receiving set reply: {}", e),
            }
        }
    }
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
