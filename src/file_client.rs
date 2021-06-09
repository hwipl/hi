use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::{io, prelude::*};
use futures::future::FutureExt;
use futures::select;
use minicbor::{Decode, Encode};

/// file message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
enum FileMessage {
    #[n(0)]
    List,
}

/// handle user command and return daemon message
pub async fn handle_daemon_message(message: Message) {
    let (file_message, from) = match message {
        Message::FileMessage { from, content, .. } => {
            match minicbor::decode::<FileMessage>(&content) {
                Ok(msg) => (msg, from),
                Err(e) => {
                    eprintln!("error decoding file message: {}", e);
                    return;
                }
            }
        }
        _ => return,
    };

    println!("Got file message {:?} from {}", file_message, from);
}

pub async fn handle_user_command(command: String) -> Option<Message> {
    // create file message according to user command
    let file_message = match command.as_str() {
        "ls" => FileMessage::List,
        _ => return None,
    };

    // create and return daemon message
    let message = {
        let mut content = Vec::new();
        if let Err(e) = minicbor::encode(file_message, &mut content) {
            eprintln!("error encoding file message: {}", e);
            return None;
        }
        Message::FileMessage {
            to: String::from("all"),
            from: String::new(),
            content,
        }
    };
    Some(message)
}

/// run daemon client in file mode
pub async fn run_file_client(mut client: unix_socket::UnixClient, _config: config::Config) {
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

    // enter file loop
    println!("File mode:");
    let mut stdin = io::BufReader::new(io::stdin()).lines();
    loop {
        select! {
            // handle message coming from daemon
            msg = client.receive_message().fuse() => {
                if let Ok(msg) = msg {
                    handle_daemon_message(msg).await;
                }
            },

            // handle line read from stdin
            line = stdin.next().fuse() => {
                let line = match line {
                    Some(Ok(line)) if line != "" => line,
                    _ => continue,
                };
                if let Some(msg) = handle_user_command(line).await {
                    if let Err(e) = client.send_message(msg).await {
                        eprintln!("error sending file message: {}", e);
                        return;
                    }
                }
            },
        }
    }
}
