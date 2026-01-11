use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Io(std::io::Error),
    Db(sea_orm::DbErr),
    Json(serde_json::Error),
    NotFound(String),
    InvalidInput(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io(err) => write!(f, "io error: {err}"),
            AppError::Db(err) => write!(f, "database error: {err}"),
            AppError::Json(err) => write!(f, "json error: {err}"),
            AppError::NotFound(message) => write_multiline(f, "Not found", message),
            AppError::InvalidInput(message) => write_multiline(f, "Invalid input", message),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AppError::Io(err) => Some(err),
            AppError::Db(err) => Some(err),
            AppError::Json(err) => Some(err),
            AppError::NotFound(_) | AppError::InvalidInput(_) => None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sea_orm::DbErr> for AppError {
    fn from(value: sea_orm::DbErr) -> Self {
        Self::Db(value)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

fn write_multiline(f: &mut fmt::Formatter<'_>, label: &str, message: &str) -> fmt::Result {
    if message.contains('\n') {
        write!(f, "{label}:\n{message}")
    } else {
        write!(f, "{label}: {message}")
    }
}
