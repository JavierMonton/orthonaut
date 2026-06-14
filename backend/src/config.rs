use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

/// Values loaded once at startup from `orthonaut.toml` (or `ORTHONAUT_CONFIG_PATH`).
#[derive(Debug, Clone)]
pub struct OrthonautConfig {
    pub path: PathBuf,
    pub wikimedia_contact: String,
    pub oauth: Option<OAuthConfig>,
    /// When set, Orthonaut reads/writes its word lists from this Wikipedia page title
    /// (e.g. `Usuario:Jmlarraz/Orthonaut/Palabras`) instead of local files. Absent → local-file mode.
    pub wordlist_page: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrthonautConfigFile {
    wikimedia_contact: String,
    oauth: Option<OAuthConfig>,
    wordlist_page: Option<String>,
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

impl OrthonautConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let file: OrthonautConfigFile = toml::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

        let wikimedia_contact = file.wikimedia_contact.trim().to_string();
        if wikimedia_contact.is_empty() {
            return Err(ConfigError::EmptyContact {
                path: path.to_path_buf(),
            });
        }

        let wordlist_page = file
            .wordlist_page
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());

        Ok(Self {
            path: path.to_path_buf(),
            wikimedia_contact,
            oauth: file.oauth,
            wordlist_page,
        })
    }
}
