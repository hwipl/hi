mod behaviour;
mod config;
mod daemon;
mod gossip;
mod request;
mod swarm;
mod unix_socket;

use std::error::Error;

pub fn run() -> Result<(), Box<dyn Error>> {
    let config = config::get();
    match config.daemon {
        true => daemon::run(config),
        false => unix_socket::run(false),
    }
    Ok(())
}
