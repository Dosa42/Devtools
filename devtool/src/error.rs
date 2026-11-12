use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{action} '{}': {source}", path.display())]
    Io {
        action: String,
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Message(String),
    #[error("Configuration: {0}")]
    Config(String),
    #[error("Archive: {0}")]
    Archive(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn io(action: &str, path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        action: action.to_owned(),
        path: path.to_owned(),
        source,
    }
}
