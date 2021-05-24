use crate::config;
use crate::swarm;
use crate::unix_socket;
use async_std::task;

pub fn run(config: config::Config) {
    // run unix socket server
    task::spawn(async {
        if let Err(e) = unix_socket::run_server().await {
            eprintln!("unix socket server error: {}", e);
        }
        println!("unix socket server stopped");
    });

    // run swarm
    if let Err(e) = swarm::run(config.connect) {
        eprintln!("swarm error: {}", e);
    }
    println!("swarm stopped");
}
