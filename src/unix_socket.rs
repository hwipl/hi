use crate::config::Config;
use crate::daemon_message::Message;
use async_std::fs;
use async_std::io;
use async_std::os::unix::net::{UnixListener, UnixStream};
use async_std::prelude::*;
use std::convert::TryFrom;

const SOCKET_FILE: &str = "hi.sock";

/// Unix socket server
pub struct UnixServer {
    listener: UnixListener,
}

impl UnixServer {
    /// Listen on unix socket
    pub async fn listen(config: &Config) -> io::Result<Self> {
        let mut socket = config.dir.clone().unwrap();
        socket.push(SOCKET_FILE);
        if socket.exists() {
            // remove old socket file
            fs::remove_file(&socket).await?;
        }
        let listener = UnixListener::bind(&socket).await?;
        Ok(UnixServer { listener })
    }

    /// Wait for next client connecting to the unix socket
    pub async fn next(&self) -> Option<UnixClient> {
        if let Some(Ok(stream)) = self.listener.incoming().next().await {
            let client = UnixClient { stream };
            return Some(client);
        }
        None
    }
}

/// Unix socket client
pub struct UnixClient {
    stream: UnixStream,
}

impl UnixClient {
    /// Connect to unix socket server and return UnixClient if successful
    pub async fn connect(config: &Config) -> io::Result<Self> {
        let mut socket = config.dir.clone().unwrap();
        socket.push(SOCKET_FILE);
        let stream = UnixStream::connect(socket).await?;
        Ok(UnixClient { stream })
    }

    /// Send bytes with prefixed length
    async fn send(&mut self, bytes: Vec<u8>) -> io::Result<()> {
        let len = match u16::try_from(bytes.len()) {
            Ok(len) => len.to_be_bytes(),
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
        };
        self.stream.write_all(&len).await?;
        self.stream.write_all(&bytes).await?;
        Ok(())
    }

    /// Receive bytes with prefixed length
    async fn receive(&mut self) -> io::Result<Vec<u8>> {
        let mut len = [0; 2];
        self.stream.read_exact(&mut len).await?;
        let len = u16::from_be_bytes(len).into();
        let mut bytes = vec![0; len];
        self.stream.read_exact(&mut bytes).await?;
        Ok(bytes)
    }

    /// Send daemon message
    pub async fn send_message(&mut self, message: Message) -> io::Result<()> {
        if let Some(bytes) = message.to_bytes() {
            self.send(bytes).await?;
        }
        Ok(())
    }

    /// Receive daemon message
    pub async fn receive_message(&mut self) -> io::Result<Message> {
        let bytes = self.receive().await?;
        match Message::from_bytes(&bytes) {
            Some(msg) => Ok(msg),
            None => Err(io::Error::new(
                io::ErrorKind::Other,
                "error receiving message",
            )),
        }
    }
}
