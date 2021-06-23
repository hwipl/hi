#[macro_use]
extern crate log;

mod behaviour;
mod chat_client;
mod config;
mod daemon;
mod daemon_client;
mod daemon_message;
mod file_client;
mod gossip;
mod request;
mod swarm;
mod unix_socket;

pub fn run() {
    env_logger::init();
    let config = config::get();
    match config.daemon {
        true => daemon::run(config),
        false => {
            if let Some(config::Command::Files) = config.command {
                file_client::run_file_client(config);
                return;
            }
            daemon_client::run(config);
        }
    }
}
