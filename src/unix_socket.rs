use async_std::io;
use async_std::os::unix::net::{UnixListener, UnixStream};
use async_std::prelude::*;
use async_std::task;

async fn handle_client(stream: UnixStream) {
    let (mut reader, mut writer) = (&stream, &stream);
    if let Err(e) = io::copy(&mut reader, &mut writer).await {
        println!("Error reading from client: {}", e);
    }
}

async fn run_server() -> async_std::io::Result<()> {
    let listener = UnixListener::bind("sockfile.sock").await?;
    let mut incoming = listener.incoming();

    while let Some(stream) = incoming.next().await {
        task::spawn(handle_client(stream?));
    }

    Ok(())
}

async fn run_client() -> async_std::io::Result<()> {
    let mut stream = UnixStream::connect("sockfile.sock").await?;

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

pub fn run(server: bool) {
    // run server
    if server {
        task::spawn(async {
            if let Err(e) = run_server().await {
                eprintln!("unix socket server error: {}", e);
            }
        });
        return;
    }

    // run client
    task::spawn(async {
        if let Err(e) = run_client().await {
            eprintln!("unix socket client error: {}", e);
        }
    });
}
