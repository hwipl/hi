use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::task;

/// run daemon client with config
async fn run_client(config: config::Config, mut client: unix_socket::UnixClient) {
    // handle connect addresses in config
    for address in config.connect {
        // send connect request
        let msg = Message::ConnectAddress { address };
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending connect message: {}", e);
        }

        // receive reply
        match client.receive_message().await {
            Ok(msg) => println!("reply from server: {:?}", msg),
            Err(e) => eprintln!("error receiving reply: {}", e),
        }
    }

    // handle set configuration options in config
    for option in config.set {
        // create message
        let msg = match option.name.as_str() {
            "name" => Message::SetName { name: option.value },
            _ => {
                eprintln!(
                    "error setting unknown configuration option: {}",
                    option.name
                );
                continue;
            }
        };

        // send message
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending set message: {}", e);
        }

        // receive reply
        match client.receive_message().await {
            Ok(msg) => println!("set reply from server: {:?}", msg),
            Err(e) => eprintln!("error receiving set reply: {}", e),
        }
    }

    // handle get configuration options in config
    for option in config.get {
        let msg = match option.name.as_str() {
            "name" => Message::GetName {
                name: String::from(""),
            },
            _ => {
                eprintln!(
                    "error getting unknown configuration option: {}",
                    option.name
                );
                continue;
            }
        };

        // send message
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending get message: {}", e);
        }

        // receive reply
        match client.receive_message().await {
            Ok(msg) => println!("get reply from server: {:?}", msg),
            Err(e) => eprintln!("error receiving get reply: {}", e),
        }
    }
}

/// initialize client connection and run daemon client
pub fn run(config: config::Config) {
    // run unix socket client
    task::block_on(async {
        match unix_socket::UnixClient::connect().await {
            Ok(mut client) => {
                if let Err(e) = client.test().await {
                    eprintln!("unix socket test error: {}", e);
                }
                run_client(config, client).await;
            }
            Err(e) => {
                eprintln!("unix socket client error: {}", e);
            }
        }
        println!("unix socket client stopped");
    });
}
