use crate::daemon_message::{FileInfo, Message};
use crate::unix_socket;

/// run daemon client in file mode
pub async fn run_file_client(
    mut client: unix_socket::UnixClient,
    file_list: Vec<String>,
    get_files: bool,
    download_from: String,
    download_file: String,
) {
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

    // share files if wanted by user
    if file_list.len() > 0 {
        let mut list = Vec::new();
        for f in file_list.iter() {
            list.push(FileInfo {
                peer_id: String::new(),
                name: f.clone(),
                size: 0,
            });
        }
        let msg = Message::ShareFiles {
            shared: true,
            files: list,
        };
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending share files message: {}", e);
            return;
        }
        if let Err(e) = client.receive_message().await {
            eprintln!("error sharing files: {}", e);
            return;
        }
        println!("Serving files: {:?}", file_list);
    }

    // send download request if wanted by user
    if download_from != "" {
        let msg = Message::DownloadFile {
            peer_id: download_from.clone(),
            file: download_file.clone(),
            destination: String::new(),
        };
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending share files message: {}", e);
            return;
        }
        if let Err(e) = client.receive_message().await {
            eprintln!("error sharing files: {}", e);
            return;
        }
        println!("Downloading file {} from {}", download_file, download_from);
    }

    // send get files request if wanted by user
    if !get_files {
        return;
    }
    let msg = Message::GetFiles { files: Vec::new() };
    if let Err(e) = client.send_message(msg).await {
        eprintln!("error sending get files message: {}", e);
        return;
    }

    // enter file loop
    println!("Files:");
    while let Ok(msg) = client.receive_message().await {
        match msg {
            Message::GetFiles { files } => {
                for f in files {
                    println!("{}: {} ({})", f.peer_id, f.name, f.size);
                }
            }
            _ => (),
        }
    }
}
