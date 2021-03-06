use core::errors as core;
use std::result;

pub type Result<T> = result::Result<T, Error>;

/// Service errors.
pub enum Error {
    BadRequest(&'static str),
    Other(core::Error),
}

impl<T> From<T> for Error
where
    T: Into<core::Error>,
{
    fn from(value: T) -> Error {
        Error::Other(value.into())
    }
}
