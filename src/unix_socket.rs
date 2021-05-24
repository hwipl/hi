use async_std::fs;
use async_std::io;
use async_std::os::unix::net::{UnixListener, UnixStream};
use async_std::path::Path;
use async_std::prelude::*;
use async_std::task;

const SOCKET_FILE: &str = "hi.sock";

async fn handle_client(stream: UnixStream) {
    let (mut reader, mut writer) = (&stream, &stream);
    if let Err(e) = io::copy(&mut reader, &mut writer).await {
        println!("Error reading from client: {}", e);
    }
}

pub async fn run_server() -> async_std::io::Result<()> {
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

async fn run_client() -> async_std::io::Result<()> {
    let mut stream = UnixStream::connect(SOCKET_FILE).await?;

    // sent request
    let request = b"hello world";
    stream.write_all(request).await?;
    println!(
        "Sent request to server: {}",
        String::from_utf8_lossy(request)
    );

    // read reply
    let mut reply = vec![0; request.len()];
    stream.read_exact(&mut reply).await?;
    println!(
        "Read reply from server: {}",
        String::from_utf8_lossy(&reply)
    );

    Ok(())
}

pub fn run(_: bool) {
    // run client
    task::spawn(async {
        if let Err(e) = run_client().await {
            eprintln!("unix socket client error: {}", e);
        }
        println!("unix socket client stopped");
    });
}
