use crate::config;
use crate::daemon_message::Message;
use crate::unix_socket;
use async_std::task;

/// run daemon client with config
async fn run_client(config: config::Config, mut client: unix_socket::UnixClient) {
    // handle connect addresses in config
    for addr in config.connect {
        // send connect request
        let msg = Message::ConnectAddress { addr };
        if let Err(e) = client.send_message(msg).await {
            eprintln!("error sending connect message: {}", e);
        }

        // receive reply
        match client.receive_message().await {
            Ok(msg) => println!("reply from server: {:?}", msg),
            Err(e) => eprintln!("error receiving reply: {}", e),
        }
    }
}

/// initialize client connection and run daemon client
pub fn run(config: config::Config) {
    // run unix socket client
    task::block_on(async {
        match unix_socket::UnixClient::connect().await {
            Ok(mut client) => {
                if let Err(e) = client.test().await {
                    eprintln!("unix socket test error: {}", e);
                }
                run_client(config, client).await;
            }
            Err(e) => {
                eprintln!("unix socket client error: {}", e);
            }
        }
        println!("unix socket client stopped");
    });
}
