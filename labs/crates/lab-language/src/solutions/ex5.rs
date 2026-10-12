use crate::ex5_error::{Config, ConfigError};
use std::error::Error;
use std::fmt;
use std::num::ParseIntError;

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Syntax { line } => write!(f, "line {line}: expected key=value"),
            ConfigError::Missing(key) => write!(f, "missing key `{key}`"),
            ConfigError::InvalidNumber(e) => write!(f, "invalid number: {e}"),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ConfigError::InvalidNumber(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ParseIntError> for ConfigError {
    fn from(e: ParseIntError) -> Self {
        ConfigError::InvalidNumber(e)
    }
}

pub fn parse_config(text: &str) -> Result<Config, ConfigError> {
    let mut port = None;
    let mut workers = None;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(ConfigError::Syntax { line: i + 1 })?;
        match key.trim() {
            "port" => port = Some(value.trim().parse::<u16>()?),
            "workers" => workers = Some(value.trim().parse::<usize>()?),
            _ => {}
        }
    }
    Ok(Config {
        port: port.ok_or(ConfigError::Missing("port"))?,
        workers: workers.ok_or(ConfigError::Missing("workers"))?,
    })
}

pub fn load_port(text: &str) -> Result<u16, Box<dyn Error + Send + Sync>> {
    let config = parse_config(text)?;
    if config.port == 0 {
        return Err("port must not be 0".into());
    }
    Ok(config.port)
}
