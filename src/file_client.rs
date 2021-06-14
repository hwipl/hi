use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::{fs, io, path, prelude::*};
use futures::future::FutureExt;
use futures::select;
use minicbor::{Decode, Encode};
use std::collections::HashMap;

/// size of data in a chunk in bytes
const CHUNK_SIZE: usize = 512;

/// file message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
enum FileMessage {
    #[n(0)]
    List,
    #[n(1)]
    ListReply(#[n(0)] Vec<(String, u64)>),
    #[n(2)]
    Get(#[n(0)] u32, #[n(1)] String),
    #[n(3)]
    Chunk(#[n(0)] u32, #[n(1)] Vec<u8>),
    #[n(4)]
    ChunkAck(#[n(0)] u32),
}

/// file transfer state
#[derive(Debug)]
enum FTState {
    New,
    SendChunk,
    SendAck,
    WaitChunk,
    WaitAck,
    WaitLastAck,
    Done,
}

/// file transfer
#[derive(Debug)]
struct FileTransfer {
    id: u32,
    from: String,
    to: String,
    file: String,

    state: FTState,
    io: Option<fs::File>,
}

impl FileTransfer {
    /// create new file transfer:
    /// upload is from "" to other peer id
    /// download is from other peer id to ""
    fn new(id: u32, from: String, to: String, file: String) -> Self {
        FileTransfer {
            id,
            from,
            to,
            file,
            state: FTState::New,
            io: None,
        }
    }

    /// is file transfer an upload?
    fn is_upload(&self) -> bool {
        if self.from == "" {
            return true;
        }
        false
    }

    /// handle incoming file messages for this file upload
    async fn handle_upload(&mut self, message: FileMessage) {
        match message {
            FileMessage::ChunkAck(..) => (),
            _ => return,
        }

        match self.state {
            FTState::WaitAck => {
                self.state = FTState::SendChunk;
            }
            FTState::WaitLastAck => {
                self.state = FTState::Done;
            }
            _ => (),
        }
    }

    /// handle incoming file messages for this file download
    async fn handle_download(&mut self, message: FileMessage) {
        let data = match message {
            FileMessage::Chunk(.., data) => data,
            _ => return,
        };

        match self.state {
            FTState::WaitChunk => (),
            _ => return,
        }

        self.write_next_chunk(data).await;
        self.state = FTState::SendAck;
    }

    /// handle incoming file message for this transfer and get next message
    async fn handle(&mut self, message: FileMessage) {
        if self.is_upload() {
            self.handle_upload(message).await;
            return;
        }
        self.handle_download(message).await;
    }

    /// open file for reading
    async fn open_read_file(&mut self) {
        if let None = self.io {
            match fs::File::open(self.file.clone()).await {
                Err(e) => eprintln!("error opening file: {}", e),
                Ok(io) => self.io = Some(io),
            }
        };
    }

    /// open file for writing
    async fn open_write_file(&mut self) {
        if let None = self.io {
            let file_name = match path::Path::new(&self.file).file_name() {
                None => return,
                Some(file_name) => file_name,
            };
            if path::Path::new(&file_name).exists().await {
                eprintln!("file already exists");
                return;
            }
            match fs::File::create(self.file.clone()).await {
                Err(e) => eprintln!("error opening file: {}", e),
                Ok(io) => self.io = Some(io),
            }
        };
    }

    /// read next chunk to send in file upload
    async fn read_next_chunk(&mut self) -> Vec<u8> {
        match self.io {
            Some(ref mut io) => {
                let mut buf = Vec::new();
                match io.take(CHUNK_SIZE as u64).read_to_end(&mut buf).await {
                    Ok(_) => {
                        return buf;
                    }
                    Err(e) => {
                        eprintln!("error reading file: {}", e);
                    }
                }
            }
            None => (),
        }
        Vec::new()
    }

    /// write next chunk received in file download
    async fn write_next_chunk(&mut self, chunk: Vec<u8>) {
        match self.io {
            Some(ref mut io) => match io.write_all(&chunk).await {
                Ok(_) => {
                    return;
                }
                Err(e) => {
                    eprintln!("error writing file: {}", e);
                }
            },
            None => (),
        }
    }

    /// get next outgoing message for this transfer
    async fn next(&mut self) -> Option<FileMessage> {
        match self.state {
            // new file transfer
            FTState::New => {
                if self.is_upload() {
                    self.open_read_file().await;
                    self.state = FTState::WaitAck;
                    let data = self.read_next_chunk().await;
                    if data.len() < CHUNK_SIZE {
                        self.state = FTState::WaitLastAck;
                    }
                    return Some(FileMessage::Chunk(self.id, data));
                } else {
                    self.open_write_file().await;
                    self.state = FTState::WaitChunk;
                    return Some(FileMessage::Get(self.id, self.file.clone()));
                }
            }

            // send next chunk
            FTState::SendChunk => {
                self.state = FTState::WaitAck;
                let data = self.read_next_chunk().await;
                if data.len() < CHUNK_SIZE {
                    self.state = FTState::WaitLastAck;
                }
                return Some(FileMessage::Chunk(self.id, data));
            }

            // send ack for received chunk
            FTState::SendAck => {
                self.state = FTState::WaitChunk;
                return Some(FileMessage::ChunkAck(self.id));
            }

            // handle other states
            FTState::WaitChunk => (),
            FTState::WaitAck => (),
            FTState::WaitLastAck => (),
            FTState::Done => (),
        }
        None
    }
}

/// file client
struct FileClient {
    _config: config::Config,
    client: unix_socket::UnixClient,
    shares: Vec<(String, u64)>,
    transfers: HashMap<u32, FileTransfer>,
}

impl FileClient {
    /// create new file Client
    pub async fn new(_config: config::Config, client: unix_socket::UnixClient) -> Self {
        FileClient {
            _config,
            client,
            shares: Vec::new(),
            transfers: HashMap::new(),
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
    async fn handle_daemon_message(&mut self, message: Message) -> Option<Message> {
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
            FileMessage::Get(id, file) => {
                self.handle_get_request(file, id, from.clone()).await;
                if self.transfers.contains_key(&id) {
                    self.transfers.get_mut(&id).unwrap().next().await
                } else {
                    None
                }
            }
            FileMessage::Chunk(id, ..) | FileMessage::ChunkAck(id, ..) => {
                if self.transfers.contains_key(&id) {
                    self.transfers
                        .get_mut(&id)
                        .unwrap()
                        .handle(file_message)
                        .await;
                    self.transfers.get_mut(&id).unwrap().next().await
                } else {
                    None
                }
            }
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
    async fn handle_user_command(&mut self, command: String) -> Option<Message> {
        // split command into its parts
        let cmd: Vec<&str> = command.split_whitespace().collect();
        if cmd.len() == 0 {
            return None;
        }

        // create file message and destination according to user command
        let (file_message, to) = match cmd[0] {
            "ls" => (FileMessage::List, String::from("all")),
            "share" => {
                self.share_files(&cmd[1..]).await;
                return None;
            }
            "get" => {
                if cmd.len() < 3 {
                    return None;
                }

                // create new download file transfer
                let id = self.new_id();
                let peer = String::from(cmd[1]);
                let file = String::from(cmd[2]);
                let file_transfer = FileTransfer::new(id, peer.clone(), String::new(), file);
                self.transfers.insert(id, file_transfer);
                let next = self.transfers.get_mut(&id).unwrap().next().await;

                (next.unwrap(), peer)
            }
            "show" => {
                println!("Shared files: {:?}", self.shares);
                println!("Transfers: {:?}", self.transfers);
                return None;
            }
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
                to,
                from: String::new(),
                content,
            }
        };
        Some(message)
    }

    /// get new file transfer id
    fn new_id(&self) -> u32 {
        let mut id = 0;
        while self.transfers.contains_key(&id) {
            id += 1;
        }
        return id;
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

    /// share files
    async fn share_files(&mut self, files: &[&str]) {
        for f in files {
            if self.is_shared(f) {
                continue;
            }
            if let Some(size) = Self::get_file_size(f).await {
                self.shares.push((f.to_string(), size));
            }
        }
    }

    /// handle get request from other peer
    async fn handle_get_request(&mut self, file: String, id: u32, from: String) {
        // only accept new transfers
        if self.transfers.contains_key(&id) {
            return;
        }

        // only accept shared files
        if !self.is_shared(&file) {
            return;
        }

        // create new upload file transfer to request sender (from)
        let file_transfer = FileTransfer::new(id, String::new(), from, file);
        self.transfers.insert(id, file_transfer);
    }
}

/// run daemon client in file mode
pub async fn run_file_client(client: unix_socket::UnixClient, config: config::Config) {
    FileClient::new(config, client).await.run().await
}
