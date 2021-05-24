mod behaviour;
mod config;
mod daemon;
mod daemon_client;
mod gossip;
mod request;
mod swarm;
mod unix_socket;

use std::error::Error;

pub fn run() -> Result<(), Box<dyn Error>> {
    let config = config::get();
    match config.daemon {
        true => daemon::run(config),
        false => daemon_client::run(config),
    }
    Ok(())
}
