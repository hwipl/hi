use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::{io, prelude::*};
use futures::future::FutureExt;
use futures::select;

/// run daemon client in file mode
pub async fn run_file_client(mut client: unix_socket::UnixClient) {
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