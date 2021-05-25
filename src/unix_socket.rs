use async_std::fs;
use async_std::io;
use async_std::os::unix::net::{UnixListener, UnixStream};
use async_std::path::Path;
use async_std::prelude::*;
use async_std::task;
use std::convert::TryFrom;

const SOCKET_FILE: &str = "hi.sock";

async fn handle_client(stream: UnixStream) {
    let (mut reader, mut writer) = (&stream, &stream);
    if let Err(e) = io::copy(&mut reader, &mut writer).await {
        println!("Error reading from client: {}", e);
    }
}

pub async fn run_server() -> io::Result<()> {
    let socket = Path::new(SOCKET_FILE);
    if socket.exists().await {
        // remove old socket file
        fs::remove_file(&socket).await?;
    }
    let listener = UnixListener::bind(&socket).await?;
    let mut incoming = listener.incoming();

    while let Some(stream) = incoming.next().await {
        task::spawn(handle_client(stream?));
    }

    Ok(())
}

/// Unix socket client
pub struct UnixClient {
    stream: UnixStream,
}

impl UnixClient {
    /// Connect to unix socket server and return UnixClient if successful
    pub async fn connect() -> io::Result<Self> {
        let stream = UnixStream::connect(SOCKET_FILE).await?;
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

    /// Unix client and server test
    pub async fn test(&mut self) -> io::Result<()> {
        // sent request
        let request = b"hello world";
        self.send(request.to_vec()).await?;
        println!(
            "Sent request to server: {}",
            String::from_utf8_lossy(request)
        );

        // read reply
        let mut reply = vec![0; request.len()];
        self.stream.read_exact(&mut reply).await?;
        println!(
            "Read reply from server: {}",
            String::from_utf8_lossy(&reply)
        );

        Ok(())
    }
}
