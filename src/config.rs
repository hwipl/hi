use clap::{AppSettings, Clap};

#[derive(Clap)]
#[clap(version)]
#[clap(setting = AppSettings::ColoredHelp)]
pub struct Config {
    /// Connect to peer addresses.
    #[clap(short, long, name = "address")]
    pub connect: Vec<String>,
}

/// get config
pub fn get() -> Config {
    Config::parse()
}
