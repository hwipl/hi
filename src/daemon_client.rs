use crate::config;
use crate::daemon_message::{FileInfo, Message};
use crate::unix_socket;
use async_std::{io, prelude::*, task};
use futures::future::FutureExt;
use futures::select;

/// run daemon client in chat mode
async fn run_chat_client(mut client: unix_socket::UnixClient, config: config::Config) {
    // set chat destination
    let destination = match config.command {
        Some(config::Command::Chat(opts)) => opts.peer,
        _ => String::from("all"),
    };

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
    loop {
        select! {
            // handle message coming from daemon
            msg = client.receive_message().fuse() => {
                match msg {
                    Ok(Message::ChatMessage{ from, from_name, message, .. }) => {
                        println!("{} <{}>: {}", from, from_name, message);
                    }
                    _ => continue,
                }
            },

            // handle line read from stdin
            line = stdin.next().fuse() => {
                let line = match line {
                    Some(Ok(line)) if line != "" => line,
                    _ => continue,
                };
                let msg = Message::ChatMessage {
                    to: destination.clone(),
                    from: String::new(),
                    from_name: String::new(),
                    message: line,
                };
                if let Err(e) = client.send_message(msg).await {
                    eprintln!("error sending chat message: {}", e);
                    return;
                }
            },
        }
    }
}

/// run daemon client in file mode
async fn run_file_client(
    mut client: unix_socket::UnixClient,
    file_list: Vec<String>,
    get_files: bool,
    download_from: String,
    download_file: String,
) {
    // enable file mode for this client
    let msg = Message::SetFiles { enabled: true };
    if let Err(e) = client.send_message(msg).await {
        eprintln!("error sending set files message: {}", e);
        return;
    }
    if let Err(e) = client.receive_message().await {
        eprintln!("error setting file support: {}", e);
        return;
    }

    // share files if wanted by user
    if file_list.len() > 0 {
        let mut list = Vec::new();
        for f in file_list.iter() {
            list.push(FileInfo {
                peer_id: String::new(),
                name: f.clone(),
                size: 0,
            });
        }
        let msg = Message::ShareFiles {
            shared: true,
            files: list,
        };
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending share files message: {}", e);
            return;
        }
        if let Err(e) = client.receive_message().await {
            eprintln!("error sharing files: {}", e);
            return;
        }
        println!("Serving files: {:?}", file_list);
    }

    // send download request if wanted by user
    if download_from != "" {
        let msg = Message::DownloadFile {
            peer_id: download_from.clone(),
            file: download_file.clone(),
            destination: String::new(),
        };
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending share files message: {}", e);
            return;
        }
        if let Err(e) = client.receive_message().await {
            eprintln!("error sharing files: {}", e);
            return;
        }
        println!("Downloading file {} from {}", download_file, download_from);
    }

    // send get files request if wanted by user
    if !get_files {
        return;
    }
    let msg = Message::GetFiles { files: Vec::new() };
    if let Err(e) = client.send_message(msg).await {
        eprintln!("error sending get files message: {}", e);
        return;
    }

    // enter file loop
    println!("Files:");
    while let Ok(msg) = client.receive_message().await {
        match msg {
            Message::GetFiles { files } => {
                for f in files {
                    println!("{}: {} ({})", f.peer_id, f.name, f.size);
                }
            }
            _ => (),
        }
    }
}

/// run daemon client with config
async fn run_client(config: config::Config, mut client: unix_socket::UnixClient) {
    // handle connect addresses in config
    for address in config.connect.iter() {
        // send connect request
        let msg = Message::ConnectAddress {
            address: address.clone(),
        };
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
    let mut file_list = Vec::new();
    let mut download_from = String::new();
    let mut download_file = String::new();
    for option in config.set.iter() {
        // create message
        let msg = match option.name.as_str() {
            "name" => Message::SetName {
                name: option.value.to_string(),
            },
            "file" => {
                // set files for file mode below
                file_list.push(option.value.to_string());
                continue;
            }
            "download_from" => {
                download_from = option.value.to_string();
                continue;
            }
            "download_file" => {
                download_file = option.value.to_string();
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
    let mut get_files = false;
    for option in config.get.iter() {
        let msg = match option.name.as_str() {
            "name" => Message::GetName {
                name: String::from(""),
            },
            "peers" => Message::GetPeers { peers: Vec::new() },
            "files" => {
                // enable file getting for file mode below
                get_files = true;
                continue;
            }
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
    if let Some(config::Command::Chat(..)) = config.command {
        run_chat_client(client, config).await;
        return;
    }

    // handle file mode
    if file_list.len() > 0 || get_files || download_from != "" && download_file != "" {
        run_file_client(client, file_list, get_files, download_from, download_file).await;
        return;
    }
}

/// initialize client connection and run daemon client
pub fn run(config: config::Config) {
    // run unix socket client
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
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
