use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Msg(String),
    MaintenanceUnavailable(String),
    PartialRemoval {
        profile_backup: std::path::PathBuf,
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Msg(m) => write!(f, "{m}"),
            Error::MaintenanceUnavailable(m) => write!(f, "maintenance unavailable: {m}"),
            Error::PartialRemoval {
                profile_backup,
                message,
            } => {
                write!(
                    f,
                    "partial removal failure: profile moved to '{}', but user data move failed: {message}; profile is safely preserved in backup",
                    profile_backup.display()
                )
            }
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
