use thiserror::Error;

#[derive(Error, Debug)]
pub enum StepFunError {
    #[error("Not authenticated. Run `stepfun login` first.")]
    NotAuthenticated,

    #[error("Session token rejected by the server. Run `stepfun login` again.")]
    TokenExpired,

    #[error("Login failed: {0}")]
    LoginFailed(String),

    #[error("API error {code}: {msg}")]
    Api { code: u16, msg: String },

    #[error("HTTP request failed: {0}")]
    Http(String),

    #[error("Request timed out")]
    Timeout,

    #[error("Failed to parse API response: {0}")]
    Parse(String),

    #[error("{0}")]
    InvalidInput(String),

    #[error("Credential storage error: {0}")]
    Storage(String),

    #[error("Interactive prompt failed: {0}")]
    Prompt(String),

    #[error("Canceled.")]
    Canceled,
}

impl From<ureq::Error> for StepFunError {
    fn from(e: ureq::Error) -> Self {
        let msg = e.to_string();
        if msg.contains("timed out") || msg.contains("timeout") {
            StepFunError::Timeout
        } else {
            StepFunError::Http(msg)
        }
    }
}

pub type Result<T> = std::result::Result<T, StepFunError>;
