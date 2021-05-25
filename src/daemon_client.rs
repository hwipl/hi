use crate::config;
use crate::unix_socket;
use async_std::task;

pub fn run(_: config::Config) {
    // run unix socket client
    task::block_on(async {
        match unix_socket::UnixClient::connect().await {
            Ok(mut client) => {
                if let Err(e) = client.test().await {
                    eprintln!("unix socket test error: {}", e);
                }
            }
            Err(e) => {
                eprintln!("unix socket client error: {}", e);
            }
        }
        println!("unix socket client stopped");
    });
}
