use std::fmt;

pub const EXIT_USAGE: i32 = 1;
pub const EXIT_MANIFEST: i32 = 2;
pub const EXIT_PIECES: i32 = 3;
pub const EXIT_DIGEST: i32 = 4;
pub const EXIT_IO: i32 = 5;
pub const EXIT_INTERRUPTED: i32 = 130;

#[derive(Debug)]
pub struct Error {
    pub code: i32,
    pub message: String,
}

impl Error {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(EXIT_USAGE, message)
    }

    pub fn manifest(message: impl Into<String>) -> Self {
        Self::new(EXIT_MANIFEST, message)
    }

    pub fn pieces(message: impl Into<String>) -> Self {
        Self::new(EXIT_PIECES, message)
    }

    pub fn digest(message: impl Into<String>) -> Self {
        Self::new(EXIT_DIGEST, message)
    }

    pub fn interrupted(message: impl Into<String>) -> Self {
        Self::new(EXIT_INTERRUPTED, message)
    }

    pub fn io(context: impl fmt::Display, error: std::io::Error) -> Self {
        Self::new(EXIT_IO, format!("{context}: {error}"))
    }

    pub fn io_path(action: &str, path: &std::path::Path, error: std::io::Error) -> Self {
        Self::new(EXIT_IO, format!("{action} '{}': {error}", path.display()))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::new(EXIT_IO, error.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::new(EXIT_MANIFEST, format!("JSON inválido: {error}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
