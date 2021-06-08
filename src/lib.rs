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
    let config = config::get();
    match config.daemon {
        true => daemon::run(config),
        false => daemon_client::run(config),
    }
}
