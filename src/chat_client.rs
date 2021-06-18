use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::{io, prelude::*};
use futures::future::FutureExt;
use futures::select;

/// run daemon client in chat mode
pub async fn run_chat_client(mut client: unix_socket::UnixClient, config: config::Config) {
    // set chat destination
    let destination = match config.command {
        Some(config::Command::Chat(opts)) => opts.peer,
        _ => String::from("all"),
    };

    // enable chat mode for this client
    let msg = Message::SetChat { enabled: true };
    if let Err(e) = client.send_message(msg).await {
        error!("error sending set chat message: {}", e);
        return;
    }

    // receive reply
    match client.receive_message().await {
        Ok(msg) => {
            debug!("set chat reply from server: {:?}", msg);
            match msg {
                Message::Ok => (),
                _ => {
                    error!("unexpected reply from server");
                    return;
                }
            }
        }
        Err(e) => {
            error!("error receiving set chat reply: {}", e);
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
                    error!("error sending chat message: {}", e);
                    return;
                }
            },
        }
    }
}
