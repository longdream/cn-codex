use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    #[error("App server not initialized")]
    NotInitialized,

    #[error("Request failed: {0}")]
    ServerRequest(String),

    #[error("{0}")]
    Custom(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_initialized_displays_correctly() {
        let err = AppError::NotInitialized;
        assert_eq!(err.to_string(), "App server not initialized");
    }

    #[test]
    fn custom_error_displays_message() {
        let err = AppError::Custom("test error".to_string());
        assert_eq!(err.to_string(), "test error");
    }

    #[test]
    fn server_request_error_displays() {
        let err = AppError::ServerRequest("timeout".to_string());
        assert_eq!(err.to_string(), "Request failed: timeout");
    }

    #[test]
    fn error_serializes_to_string() {
        let err = AppError::NotInitialized;
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"App server not initialized\"");
    }

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let app_err: AppError = io_err.into();
        assert!(app_err.to_string().contains("file missing"));
    }
}
