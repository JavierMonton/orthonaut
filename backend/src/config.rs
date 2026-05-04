use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

/// Values loaded once at startup from `ortobot.toml` (or `ORTOBOT_CONFIG_PATH`).
#[derive(Debug, Clone)]
pub struct OrtobotConfig {
    pub path: PathBuf,
    pub wikimedia_contact: String,
}

#[derive(Debug, Deserialize)]
struct OrtobotConfigFile {
    wikimedia_contact: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read config file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("config field `wikimedia_contact` must not be empty (file: {path})")]
    EmptyContact { path: PathBuf },
}

impl OrtobotConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let file: OrtobotConfigFile = toml::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

        let wikimedia_contact = file.wikimedia_contact.trim().to_string();
        if wikimedia_contact.is_empty() {
            return Err(ConfigError::EmptyContact {
                path: path.to_path_buf(),
            });
        }

        Ok(Self {
            path: path.to_path_buf(),
            wikimedia_contact,
        })
    }
}
