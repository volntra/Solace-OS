use std::io;
use std::path::{Path, PathBuf};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Malformed input: a recipe, manifest, index, name, version, or path.
    #[error("{0}")]
    Invalid(String),

    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// A signature is missing, malformed, from an untrusted key, or wrong.
    #[error("signature check failed: {0}")]
    Signature(String),

    /// A package or index does not match what was signed or indexed.
    #[error("verification failed: {0}")]
    Verify(String),

    #[error("could not fetch {url}: {reason}")]
    Fetch { url: String, reason: String },

    #[error("build failed: {0}")]
    Build(String),

    #[error("{0}")]
    Solve(String),

    /// The requested change would break the installed system.
    #[error("{0}")]
    Conflict(String),

    #[error("package database: {0}")]
    Db(#[from] rusqlite::Error),
}

impl Error {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid(msg.into())
    }
}

/// Attaches a path to I/O errors.
pub trait IoContext<T> {
    /// # Errors
    /// Wraps the underlying I/O error with `path`.
    fn at(self, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.as_ref().to_path_buf(),
            source,
        })
    }
}
