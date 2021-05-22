mod behaviour;
mod gossip;
mod request;
mod swarm;

use std::error::Error;

pub fn run() -> Result<(), Box<dyn Error>> {
    swarm::run()
}
