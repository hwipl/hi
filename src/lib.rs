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
    match config.command {
        Some(config::Command::Daemon) => daemon::run(config),
        Some(config::Command::Chat(..)) => chat_client::run_chat_client(config),
        Some(config::Command::Files) => file_client::run_file_client(config),
        None => daemon_client::run(config),
    }
}
