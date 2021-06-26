use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::task;

/// get client
struct GetClient {
    config: config::Config,
    client: unix_socket::UnixClient,
}

impl GetClient {
    /// create new get client
    async fn new(config: config::Config, client: unix_socket::UnixClient) -> Self {
        GetClient { config, client }
    }

    /// run get client
    async fn run(&mut self) {
        // get options to get from config
        let options = match self.config.command {
            Some(config::Command::Get(ref get_opts)) => &get_opts.info,
            _ => return,
        };

        // handle get configuration options
        for option in options.iter() {
            let msg = match option.as_str() {
                "name" => Message::GetName {
                    name: String::from(""),
                },
                "peers" => Message::GetPeers { peers: Vec::new() },
                _ => {
                    error!("error getting unknown configuration option: {}", option);
                    continue;
                }
            };

            // send message
            if let Err(e) = self.client.send_message(msg).await {
                error!("error sending get message: {}", e);
            }

            // receive reply
            match self.client.receive_message().await {
                Ok(msg) => {
                    debug!("get reply from server: {:?}", msg);
                    match msg {
                        Message::GetName { name } => println!("Name: {}", name),
                        _ => println!("{:?}", msg),
                    }
                }
                Err(e) => error!("error receiving get reply: {}", e),
            }
        }
    }
}

/// run get client
pub fn run(config: config::Config) {
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => GetClient::new(config, client).await.run().await,
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("get client stopped");
    });
}
