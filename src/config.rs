use clap::{AppSettings, Clap};
use std::str::FromStr;

/// Configuration option for setting and getting:
/// setting requires name and value, getting only requires name
pub struct ConfigOption {
    pub name: String,
    pub value: String,
}

impl FromStr for ConfigOption {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // parse option string and get name and value
        let parts: Vec<&str> = s.split(":").collect();
        if parts.len() < 1 || parts.len() > 2 {
            return Err(String::from("invalid configuration option"));
        }
        let name = String::from(parts[0]);

        // only name given?
        if parts.len() == 1 {
            let value = String::from("");
            return Ok(ConfigOption { name, value });
        }

        // name and value given
        let value = String::from(parts[1]);
        Ok(ConfigOption { name, value })
    }
}

#[derive(Clap)]
#[clap(version)]
#[clap(setting = AppSettings::ColoredHelp)]
pub struct Config {
    /// Run in daemon mode.
    #[clap(short, long)]
    pub daemon: bool,

    /// Connect to peer addresses.
    #[clap(short, long, name = "address")]
    pub connect: Vec<String>,

    /// Set configuration options
    #[clap(long, name = "option:value")]
    pub set: Vec<ConfigOption>,

    /// Get configuration options
    #[clap(long, name = "option")]
    pub get: Vec<ConfigOption>,
}

/// get config
pub fn get() -> Config {
    Config::parse()
}
