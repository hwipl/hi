mod behaviour;
mod config;
mod gossip;
mod request;
mod swarm;
mod unix_socket;

use std::error::Error;

pub fn run() -> Result<(), Box<dyn Error>> {
    let config = config::get();
    unix_socket::run(config.daemon);
    swarm::run(config.connect)
}
