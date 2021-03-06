use clap::Parser;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::str::FromStr;

/// Configuration option for setting and getting:
/// setting requires name and value, getting only requires name
#[derive(Clone)]
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

#[derive(Clone, Parser)]
#[clap(version)]
pub struct DaemonOpts {
    /// Set configuration options
    #[clap(long, name = "option:value")]
    pub set: Vec<ConfigOption>,
}

#[derive(Clone, Parser)]
#[clap(version)]
pub struct GetOpts {
    /// Information to get from the daemon
    pub info: Vec<String>,
}

#[derive(Clone, Parser)]
#[clap(version)]
pub struct SetOpts {
    /// Option:value pairs to set on the daemon
    #[clap(name = "option:value")]
    pub opts: Vec<ConfigOption>,
}

#[derive(Clone, Parser)]
#[clap(version)]
pub struct ChatOpts {
    /// Peer ID of chat partner
    #[clap(long, default_value = "all")]
    pub peer: String,

    /// User name shown in chat
    #[clap(long)]
    pub name: Option<String>,
}

#[derive(Clone, Parser)]
pub enum Command {
    /// Run daemon
    Daemon(DaemonOpts),
    /// Get information from running daemon
    Get(GetOpts),
    /// Set configuration options on running daemon
    Set(SetOpts),
    /// Run in chat mode
    Chat(ChatOpts),
    /// Run in file mode
    Files,
}

#[derive(Clone, Parser)]
#[clap(version)]
pub struct Config {
    /// Set directory
    #[clap(long)]
    pub dir: Option<PathBuf>,

    /// Run command
    #[clap(subcommand)]
    pub command: Option<Command>,
}

/// get config
pub fn get() -> Config {
    let mut config = Config::parse();

    // check working directory
    if let None = config.dir {
        if let Some(mut dir) = dirs::config_dir() {
            dir.push("hi");
            create_dir_all(&dir).expect("could not create directory");
            config.dir = Some(dir);
        } else {
            config.dir = Some(PathBuf::from(""));
        }
    }
    config
}
