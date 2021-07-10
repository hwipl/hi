use crate::config;
use crate::message::{Message, Service};
use crate::unix_socket;
use async_std::{fs, io, path, prelude::*, task};
use futures::future::FutureExt;
use futures::select;
use minicbor::{Decode, Encode};
use std::collections::HashMap;
use std::error::Error;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wasm_timer::Delay;

/// size of data in a chunk in bytes
const CHUNK_SIZE: usize = 512;

/// idle timeout of a transfer in seconds
const IDLE_TIMEOUT: u64 = 30;

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
    Chunk(
        #[n(0)] u32,
        #[n(1)]
        #[cbor(with = "minicbor::bytes")]
        Vec<u8>,
    ),
    #[n(4)]
    ChunkAck(#[n(0)] u32),
}

/// file transfer state
#[derive(Debug)]
enum FTState {
    New,
    SendChunk,
    SendAck,
    SendLastAck,
    WaitChunk,
    WaitAck,
    WaitLastAck,
    Done,
    Error(String),
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
    created_at: u64,
    last_active: u64,
    completed_at: u64,
    num_bytes: u64,
}

impl FileTransfer {
    /// create new file transfer:
    /// upload is from "" to other peer id
    /// download is from other peer id to ""
    fn new(id: u32, from: String, to: String, file: String) -> Self {
        let current_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp error")
            .as_secs();
        FileTransfer {
            id,
            from,
            to,
            file,
            state: FTState::New,
            io: None,
            created_at: current_secs,
            last_active: current_secs,
            completed_at: 0,
            num_bytes: 0,
        }
    }

    /// is file transfer done?
    fn is_done(&self) -> bool {
        match self.state {
            FTState::Done => true,
            _ => false,
        }
    }

    /// is file transfer in error state?
    fn is_error(&self) -> bool {
        match self.state {
            FTState::Error(..) => true,
            _ => false,
        }
    }

    /// reset timeout of the transfer
    fn reset_timeout(&mut self) {
        let current_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp error")
            .as_secs();
        self.last_active = current_secs;
    }

    /// check timeout of the transfer and set error state accordingly
    fn check_timeout(&mut self) {
        if self.is_done() || self.is_error() {
            return;
        }
        let current_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp error")
            .as_secs();
        if current_secs - self.last_active > IDLE_TIMEOUT {
            error!("transfer timed out");
            self.complete(Some("Timeout".into()));
        }
    }

    /// complete transfer and set optional error state/message
    fn complete(&mut self, error: Option<String>) {
        if self.is_done() || self.is_error() {
            return;
        }
        self.io = None;
        let current_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp error")
            .as_secs();
        self.completed_at = current_secs;
        match error {
            None => self.state = FTState::Done,
            Some(error) => self.state = FTState::Error(error),
        }
    }

    /// cancel the transfer
    fn cancel(&mut self) {
        self.complete(Some("Canceled by user".into()));
    }

    /// get the data rate of the transfer
    fn get_data_rate(&self) -> u64 {
        let time = match self.completed_at {
            x if x > 0 => x,
            _ => self.last_active,
        };
        if time <= self.created_at {
            return 0;
        }
        let secs = time - self.created_at;
        self.num_bytes / secs
    }

    /// is file transfer an upload?
    fn is_upload(&self) -> bool {
        if self.from == "" {
            return true;
        }
        false
    }

    /// is `from` a valid sender for this transfer?
    fn is_valid_sender(&self, from: String) -> bool {
        // upload
        if self.is_upload() {
            if from == self.to {
                return true;
            }
            return false;
        }

        // download
        if from == self.from {
            return true;
        }
        return false;
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
                self.complete(None);
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

        self.state = FTState::SendAck;
        if data.len() < CHUNK_SIZE {
            self.state = FTState::SendLastAck;
        }
        if let None = self.write_next_chunk(data).await {
            self.state = FTState::Error("Error writing file".into());
        }
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
    async fn open_read_file(&self) -> Option<fs::File> {
        if let None = self.io {
            return fs::File::open(self.file.clone()).await.ok();
        };
        None
    }

    /// open file for writing
    async fn open_write_file(&self) -> Option<fs::File> {
        if let None = self.io {
            let file_name = path::Path::new(&self.file).file_name()?;
            if path::Path::new(&file_name).exists().await {
                error!("file already exists");
                return None;
            }
            return fs::File::create(file_name.clone()).await.ok();
        };
        None
    }

    /// read next chunk to send in file upload
    async fn read_next_chunk(&mut self) -> Option<Vec<u8>> {
        self.reset_timeout();
        if let Some(ref mut io) = self.io {
            let mut buf = Vec::new();
            io.take(CHUNK_SIZE as u64)
                .read_to_end(&mut buf)
                .await
                .ok()?;
            self.num_bytes += buf.len() as u64;
            return Some(buf);
        };
        None
    }

    /// write next chunk received in file download
    async fn write_next_chunk(&mut self, chunk: Vec<u8>) -> Option<()> {
        self.reset_timeout();
        self.num_bytes += chunk.len() as u64;
        if let Some(ref mut io) = self.io {
            io.write_all(&chunk).await.ok()?;
            return Some(());
        };
        None
    }

    /// get next chunk message
    async fn next_chunk_message(&mut self) -> Option<FileMessage> {
        self.state = FTState::WaitAck;
        if let Some(data) = self.read_next_chunk().await {
            if data.len() < CHUNK_SIZE {
                self.state = FTState::WaitLastAck;
            }
            return Some(FileMessage::Chunk(self.id, data));
        };

        self.state = FTState::Error("Error reading file".into());
        None
    }

    /// get next outgoing message for this transfer
    async fn next(&mut self) -> Option<FileMessage> {
        match self.state {
            // new file transfer
            FTState::New => {
                if self.is_upload() {
                    self.io = self.open_read_file().await;
                    if let None = self.io {
                        self.state = FTState::Error("Error opening file".into());
                        return None;
                    }
                    return self.next_chunk_message().await;
                } else {
                    self.io = self.open_write_file().await;
                    if let None = self.io {
                        self.state = FTState::Error("Error opening file".into());
                        return None;
                    }
                    self.state = FTState::WaitChunk;
                    return Some(FileMessage::Get(self.id, self.file.clone()));
                }
            }

            // send next chunk
            FTState::SendChunk => {
                return self.next_chunk_message().await;
            }

            // send ack for received chunk
            FTState::SendAck => {
                self.state = FTState::WaitChunk;
                return Some(FileMessage::ChunkAck(self.id));
            }

            // send last ack for received chunk
            FTState::SendLastAck => {
                self.complete(None);
                return Some(FileMessage::ChunkAck(self.id));
            }

            // handle other states
            FTState::WaitChunk => (),
            FTState::WaitAck => (),
            FTState::WaitLastAck => (),
            FTState::Done => (),
            FTState::Error(..) => (),
        }
        None
    }
}

/// file client
struct FileClient {
    _config: config::Config,
    client: unix_socket::UnixClient,
    client_id: u16,
    shares: Vec<(String, u64)>,
    transfers: HashMap<u32, FileTransfer>,
}

impl FileClient {
    /// create new file Client
    pub async fn new(_config: config::Config, client: unix_socket::UnixClient) -> Self {
        FileClient {
            _config,
            client,
            client_id: 0,
            shares: Vec::new(),
            transfers: HashMap::new(),
        }
    }

    /// register this client
    async fn register_client(&mut self) -> Result<(), Box<dyn Error>> {
        let msg = Message::Register {
            services: vec![Service::File as u16].into_iter().collect(),
            files: true,
        };
        self.client.send_message(msg).await?;
        match self.client.receive_message().await? {
            Message::RegisterOk { client_id } => {
                self.client_id = client_id;
                Ok(())
            }
            _ => Err("unexpected message from daemon".into()),
        }
    }

    /// run file client
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        // register this client and enable file mode
        self.register_client().await?;

        // enter file loop
        println!("File mode:");
        let mut stdin = io::BufReader::new(io::stdin()).lines();
        let mut timer = Delay::new(Duration::from_secs(5)).fuse();
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

                // handle timer event
                _ = timer => {
                    timer = Delay::new(Duration::from_secs(5)).fuse();
                    for transfer in self.transfers.values_mut() {
                        transfer.check_timeout();
                    }
                }
            }

            // if theres a message for the daemon, send it
            if let Some(msg) = daemon_message {
                self.client.send_message(msg).await?;
            }
        }
    }

    /// handle file message coming from daemon
    async fn handle_daemon_message_file(
        &mut self,
        from_peer: String,
        from_client: u16,
        file_message: FileMessage,
    ) -> Option<Message> {
        // handle file message and create response file message
        let response = match file_message {
            FileMessage::List => Some(FileMessage::ListReply(self.shares.clone())),
            FileMessage::ListReply(list) => {
                for (file, size) in list {
                    println!("{}/{}: {} ({} bytes)", from_peer, from_client, file, size);
                }
                None
            }
            FileMessage::Get(id, file) => {
                self.handle_get_request(file, id, from_peer.clone()).await;
                if self.transfers.contains_key(&id) {
                    self.transfers.get_mut(&id).unwrap().next().await
                } else {
                    None
                }
            }
            FileMessage::Chunk(id, ..) | FileMessage::ChunkAck(id, ..) => {
                if self.transfers.contains_key(&id) {
                    if !self
                        .transfers
                        .get(&id)
                        .unwrap()
                        .is_valid_sender(from_peer.clone())
                    {
                        error!(
                            "got message for transfer {} from invalid sender {}",
                            id, from_peer
                        );
                        return None;
                    }
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
                error!("error encoding file message: {}", e);
                return None;
            }
            return Some(Message::FileMessage {
                to_peer: from_peer,
                from_peer: String::new(),
                to_client: from_client,
                from_client: self.client_id,
                content,
            });
        }
        None
    }

    /// handle message coming from daemon and return daemon message as reply
    async fn handle_daemon_message(&mut self, message: Message) -> Option<Message> {
        match message {
            Message::FileMessage {
                from_peer,
                from_client,
                content,
                ..
            } => match minicbor::decode::<FileMessage>(&content) {
                Ok(msg) => {
                    self.handle_daemon_message_file(from_peer.clone(), from_client, msg)
                        .await
                }
                Err(e) => {
                    error!("error decoding file message: {}", e);
                    None
                }
            },
            _ => None,
        }
    }

    /// handle user command and return daemon message
    async fn handle_user_command(&mut self, command: String) -> Option<Message> {
        // split command into its parts
        let cmd: Vec<&str> = command.split_whitespace().collect();
        if cmd.len() == 0 {
            return None;
        }

        // create file message and destination according to user command
        let (file_message, to_peer, to_client) = match cmd[0] {
            "ls" => (FileMessage::List, String::from("all"), Message::ALL_CLIENTS),
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
                let (peer, client) = {
                    let (p, c) = cmd[1].split_once("/")?;
                    (String::from(p), c.parse().ok()?)
                };
                let file = String::from(cmd[2]);
                let file_transfer = FileTransfer::new(id, peer.clone(), String::new(), file);
                self.transfers.insert(id, file_transfer);
                let next = self.transfers.get_mut(&id).unwrap().next().await?;

                (next, peer, client)
            }
            "show" => {
                println!("Shared files:");
                for share in self.shares.iter() {
                    println!("  {} ({} bytes)", share.0, share.1);
                }
                println!("Transfers:");
                for transfer in self.transfers.values() {
                    println!(
                        "  {}: {:?} -> {:?}: {} ({} bytes, {} bytes/s) [{:?}]",
                        transfer.id,
                        transfer.from,
                        transfer.to,
                        transfer.file,
                        transfer.num_bytes,
                        transfer.get_data_rate(),
                        transfer.state,
                    );
                }
                return None;
            }
            "cancel" => {
                if cmd.len() < 2 {
                    return None;
                }

                // cancel file transfer
                let id = cmd[1].parse().ok()?;
                if let Some(transfer) = self.transfers.get_mut(&id) {
                    transfer.cancel();
                };
                return None;
            }
            _ => return None,
        };

        // create and return daemon message
        let message = {
            let mut content = Vec::new();
            if let Err(e) = minicbor::encode(file_message, &mut content) {
                error!("error encoding file message: {}", e);
                return None;
            }
            Message::FileMessage {
                to_peer,
                from_peer: String::new(),
                to_client,
                from_client: self.client_id,
                content,
            }
        };
        Some(message)
    }

    /// get new file transfer id
    fn new_id(&self) -> u32 {
        let mut id = rand::random();
        while self.transfers.contains_key(&id) {
            id = rand::random();
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
pub fn run(config: config::Config) {
    task::block_on(async {
        match unix_socket::UnixClient::connect(&config).await {
            Ok(client) => {
                if let Err(e) = FileClient::new(config, client).await.run().await {
                    error!("{}", e);
                }
            }
            Err(e) => error!("unix socket client error: {}", e),
        }
        debug!("file client stopped");
    });
}
