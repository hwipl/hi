mod client;

use crate::config::Config;

pub async fn run(config: Config) {
    client::run(config).await;
}
