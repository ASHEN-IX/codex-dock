use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Msg(String),
    MaintenanceUnavailable(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Msg(m) => write!(f, "{m}"),
            Error::MaintenanceUnavailable(m) => write!(f, "maintenance unavailable: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
