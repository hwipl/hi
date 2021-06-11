use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::{fs, io, prelude::*};
use futures::future::FutureExt;
use futures::select;
use minicbor::{Decode, Encode};

/// file message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
enum FileMessage {
    #[n(0)]
    List,
    #[n(1)]
    ListReply(#[n(0)] Vec<(String, u64)>),
}

/// file client
struct FileClient {
    _config: config::Config,
    client: unix_socket::UnixClient,
    shares: Vec<(String, u64)>,
}

impl FileClient {
    /// create new file Client
    pub async fn new(_config: config::Config, client: unix_socket::UnixClient) -> Self {
        FileClient {
            _config,
            client,
            shares: Vec::new(),
        }
    }

    /// run file client
    pub async fn run(&mut self) {
        // enable file mode for this client
        let msg = Message::SetFiles { enabled: true };
        if let Err(e) = self.client.send_message(msg).await {
            eprintln!("error sending set files message: {}", e);
            return;
        }
        if let Err(e) = self.client.receive_message().await {
            eprintln!("error setting file support: {}", e);
            return;
        }

        // enter file loop
        println!("File mode:");
        let mut stdin = io::BufReader::new(io::stdin()).lines();
        loop {
            let mut daemon_message = None;

            select! {
                // handle message coming from daemon
                msg = self.client.receive_message().fuse() => {
                    if let Ok(msg) = msg {
                        daemon_message = self.handle_daemon_message(msg).await;
                    }
                },

                // handle line read from stdin
                line = stdin.next().fuse() => {
                    let line = match line {
                        Some(Ok(line)) if line != "" => line,
                        _ => continue,
                    };
                    daemon_message = self.handle_user_command(line).await;
                },
            }

            // if theres a message for the daemon, send it
            if let Some(msg) = daemon_message {
                if let Err(e) = self.client.send_message(msg).await {
                    eprintln!("error sending file message: {}", e);
                    return;
                }
            }
        }
    }

    /// handle message coming from daemon and return daemon message as reply
    async fn handle_daemon_message(&self, message: Message) -> Option<Message> {
        // get file message and sender
        let (file_message, from) = match message {
            Message::FileMessage { from, content, .. } => {
                match minicbor::decode::<FileMessage>(&content) {
                    Ok(msg) => (msg, from),
                    Err(e) => {
                        eprintln!("error decoding file message: {}", e);
                        return None;
                    }
                }
            }
            _ => return None,
        };

        // handle file message and create response file message
        println!("Got file message {:?} from {}", file_message, from);
        let response = match file_message {
            FileMessage::List => Some(FileMessage::ListReply(self.shares.clone())),
            FileMessage::ListReply(..) => None,
        };

        // if there is a response file message, create daemon message and return it
        if let Some(response) = response {
            let mut content = Vec::new();
            if let Err(e) = minicbor::encode(response, &mut content) {
                eprintln!("error encoding file message: {}", e);
                return None;
            }
            return Some(Message::FileMessage {
                to: from,
                from: String::new(),
                content,
            });
        }

        None
    }

    /// handle user command and return daemon message
    async fn handle_user_command(&self, command: String) -> Option<Message> {
        // split command into its parts
        let cmd: Vec<&str> = command.split_whitespace().collect();
        if cmd.len() == 0 {
            return None;
        }

        // create file message according to user command
        let file_message = match cmd[0] {
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

    /// check if file is shared
    fn is_shared(&self, file: &str) -> bool {
        for s in self.shares.iter() {
            if s.0 == file {
                return true;
            }
        }
        return false;
    }

    /// get size of the file
    async fn get_file_size(file: &str) -> Option<u64> {
        if let Ok(meta) = fs::metadata(&file).await {
            return Some(meta.len());
        }
        None
    }
}

/// run daemon client in file mode
pub async fn run_file_client(client: unix_socket::UnixClient, config: config::Config) {
    FileClient::new(config, client).await.run().await
}
