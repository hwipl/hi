mod client;

use crate::config::Config;

pub fn run(config: Config) {
    client::run(config);
}
