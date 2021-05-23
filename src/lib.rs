mod behaviour;
mod gossip;
mod request;
mod swarm;
mod unix_socket;

use std::error::Error;

pub fn run() -> Result<(), Box<dyn Error>> {
    unix_socket::run(true);
    swarm::run(Vec::new())
}
