use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::{io, prelude::*, task};

/// run daemon client in chat mode
async fn run_chat_client(mut client: unix_socket::UnixClient, destination: String) {
    // enable chat mode for this client
    let msg = Message::SetChat { enabled: true };
    if let Err(e) = client.send_message(msg).await {
        eprintln!("error sending set chat message: {}", e);
        return;
    }

    // receive reply
    match client.receive_message().await {
        Ok(msg) => {
            println!("set chat reply from server: {:?}", msg);
            match msg {
                Message::Ok => (),
                _ => {
                    eprintln!("unexpected reply from server");
                    return;
                }
            }
        }
        Err(e) => {
            eprintln!("error receiving set chat reply: {}", e);
            return;
        }
    }

    // enter chat mode
    println!("Chat mode:");
    let mut stdin = io::BufReader::new(io::stdin()).lines();
    while let Some(Ok(line)) = stdin.next().await {
        let msg = Message::ChatMessage {
            to: destination.clone(),
            from: String::new(),
            message: line,
        };
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending chat message: {}", e);
            return;
        }
    }
}

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
    let mut chat_partner = String::new();
    for option in config.set {
        // create message
        let msg = match option.name.as_str() {
            "name" => Message::SetName { name: option.value },
            "chat" => {
                // set chat partner for chat mode below
                chat_partner = option.value;
                continue;
            }
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
            "peers" => Message::GetPeers { peers: Vec::new() },
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

    // handle chat mode
    if chat_partner != "" {
        run_chat_client(client, chat_partner).await;
        return;
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
