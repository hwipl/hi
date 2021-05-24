use crate::config;
use crate::unix_socket;
use async_std::task;

pub fn run(_: config::Config) {
    // run unix socket client
    task::block_on(async {
        if let Err(e) = unix_socket::run_client().await {
            eprintln!("unix socket client error: {}", e);
        }
        println!("unix socket client stopped");
    });
}
