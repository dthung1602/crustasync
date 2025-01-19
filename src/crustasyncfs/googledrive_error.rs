use std::fmt::Formatter;

pub enum GDError {
    MissingField { field: String },
    InvalidData { field: String, message: String },
    FileNotFound { file: String },
    ParentNotFound { file: String },
}

impl std::error::Error for GDError {}

impl std::fmt::Debug for GDError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self, f)
    }
}

impl std::fmt::Display for GDError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            GDError::MissingField { field } => write!(f, "GDError: Missing field {field}"),
            GDError::InvalidData { field, message } => {
                write!(f, "GDError: Invalid data in {field}, {message}")
            }
            GDError::FileNotFound { file } => {
                write!(f, "GDError: File not found {file}")
            }
            GDError::ParentNotFound { file } => {
                write!(f, "GDError: Cannot find parent of {file}")
            }
        }
    }
}
