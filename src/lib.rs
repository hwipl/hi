#[macro_use]
extern crate log;

mod client;
mod config;
mod daemon;
mod message;
mod unix_socket;

pub async fn run() {
    env_logger::init();
    let config = config::get();
    match config.command {
        Some(config::Command::Daemon(..)) => daemon::run(config).await,
        Some(config::Command::Get(..)) => client::get::run(config).await,
        Some(config::Command::Set(..)) => client::set::run(config).await,
        Some(config::Command::Chat(..)) => client::chat::run(config).await,
        Some(config::Command::Files) => client::file::run(config).await,
        None => (),
    }
}
