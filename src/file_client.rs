use crate::daemon_message::{FileInfo, Message};
use crate::unix_socket;
use async_std::{io, prelude::*};
use futures::future::FutureExt;
use futures::select;

/// run daemon client in file mode
pub async fn run_file_client(
    mut client: unix_socket::UnixClient,
    file_list: Vec<String>,
    get_files: bool,
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
    println!("File mode:");
    let mut stdin = io::BufReader::new(io::stdin()).lines();
    loop {
        select! {
            // handle message coming from daemon
            msg = client.receive_message().fuse() => {
                match msg {
                    Ok(Message::GetFiles { files }) => {
                        for f in files {
                            println!("{}: {} ({})", f.peer_id, f.name, f.size);
                        }
                    }
                    _ => (),
                }
            },

            // handle line read from stdin
            line = stdin.next().fuse() => {
                let line = match line {
                    Some(Ok(line)) if line != "" => line,
                    _ => continue,
                };
                let msg = Message::FileMessage {
                    to: String::from("all"),
                    from: String::new(),
                    content: line.into_bytes(),
                };
                if let Err(e) = client.send_message(msg).await {
                    eprintln!("error sending file message: {}", e);
                    return;
                }
            },
        }
    }
}
