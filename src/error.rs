use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::string::FromUtf8Error;

use crate::crustasyncfs::googledrive::GDError;

pub enum Error {
    // Generic errors
    ExpectDirectory(PathBuf),
    ExpectFile(PathBuf),
    Serde(serde_json::Error),
    Utf8(FromUtf8Error),
    Request(reqwest::Error),
    Io(std::io::Error),
    Unknown(anyhow::Error),

    // module specific errors
    GoogleDrive(GDError),
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::error::Error for Error {}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self, f)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            // generic errors
            Error::ExpectDirectory(path) => {
                write!(
                    f,
                    "ExpectDirectory: Expect '{}' to be a directory, found a file",
                    path.display()
                )
            }
            Error::ExpectFile(path) => {
                write!(
                    f,
                    "ExpectFile: Expect '{}' to be a file, found a directory",
                    path.display()
                )
            }
            Error::Serde(e) => std::fmt::Display::fmt(&e, f),
            Error::Utf8(e) => std::fmt::Display::fmt(&e, f),
            Error::Request(e) => std::fmt::Display::fmt(&e, f),
            Error::Io(e) => std::fmt::Display::fmt(&e, f),
            Error::Unknown(e) => write!(f, "UnknownError: {e}"),
            // module specific errors
            Error::GoogleDrive(e) => std::fmt::Display::fmt(&e, f),
        }
    }
}

// ------------------------------
// region Implement From<T>
// ------------------------------

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Serde(value)
    }
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Error::Utf8(value)
    }
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Error::Request(value)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::Io(value)
    }
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Error::Unknown(value)
    }
}

impl From<GDError> for Error {
    fn from(value: GDError) -> Self {
        Error::GoogleDrive(value)
    }
}

// endregion
