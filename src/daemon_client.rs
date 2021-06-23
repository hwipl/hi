use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::task;

/// run daemon client with config
async fn run_client(config: config::Config, mut client: unix_socket::UnixClient) {
    // handle connect addresses in config
    for address in config.connect.iter() {
        // send connect request
        let msg = Message::ConnectAddress {
            address: address.clone(),
        };
        if let Err(e) = client.send_message(msg).await {
            error!("error sending connect message: {}", e);
        }

        // receive reply
        match client.receive_message().await {
            Ok(msg) => debug!("reply from server: {:?}", msg),
            Err(e) => error!("error receiving reply: {}", e),
        }
    }

    // handle set configuration options in config
    for option in config.set.iter() {
        // create message
        let msg = match option.name.as_str() {
            "name" => Message::SetName {
                name: option.value.to_string(),
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
        if let Err(e) = client.send_message(msg).await {
            error!("error sending set message: {}", e);
        }

        // receive reply
        match client.receive_message().await {
            Ok(msg) => debug!("set reply from server: {:?}", msg),
            Err(e) => error!("error receiving set reply: {}", e),
        }
    }

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
