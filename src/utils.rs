use std::fmt;
use std::error::Error;

#[derive(Debug)]
pub struct StringError {
    error: String
}

pub fn serr(error: String) -> StringError {
    StringError {
        error,
    }
}

impl Error for StringError {
    fn description(&self) -> &str {
        &self.error
    }
}

impl fmt::Display for StringError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error: {}", self.error)
    }
}
