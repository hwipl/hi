mod behaviour;
mod config;
mod gossip;
mod request;
mod swarm;
mod unix_socket;

use std::error::Error;

pub fn run() -> Result<(), Box<dyn Error>> {
    let config = config::get();
    match config.daemon {
        true => {
            unix_socket::run(true);
            swarm::run(config.connect)
        }
        false => {
            unix_socket::run(false);
            Ok(())
        }
    }
}
