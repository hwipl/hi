#[macro_use]
extern crate log;

mod client;
mod config;
mod daemon;
mod daemon_client;
mod daemon_message;
mod gossip;
mod request;
mod swarm;
mod unix_socket;

pub fn run() {
    env_logger::init();
    let config = config::get();
    match config.command {
        Some(config::Command::Daemon) => daemon::run(config),
        Some(config::Command::Get) => client::get::run(config),
        Some(config::Command::Chat(..)) => client::chat::run(config),
        Some(config::Command::Files) => client::file::run(config),
        None => daemon_client::run(config),
    }
}
