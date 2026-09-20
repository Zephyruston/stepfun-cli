use serde::{Deserialize, Serialize};

use crate::Result;
use crate::auth::Credentials;
use crate::error::StepFunError;

const APP_NAME: &str = "stepfun-cli";
const CONFIG_NAME: &str = "credentials";

#[derive(Serialize, Deserialize, Default)]
struct CredentialsFile {
    username: String,
    token: String,
    webid: String,
}

impl From<&Credentials> for CredentialsFile {
    fn from(c: &Credentials) -> Self {
        Self {
            username: c.username.clone(),
            token: c.token.trim().to_string(),
            webid: c.webid.trim().to_string(),
        }
    }
}

impl From<&CredentialsFile> for Credentials {
    fn from(f: &CredentialsFile) -> Self {
        Self {
            username: f.username.clone(),
            token: f.token.clone(),
            webid: f.webid.clone(),
        }
    }
}

/// Persist credentials through confy (XDG config dir, platform-appropriate path)
/// and restrict the file to the current user on Unix.
pub fn store(credentials: &Credentials) -> Result<()> {
    let file: CredentialsFile = credentials.into();
    confy::store(APP_NAME, Some(CONFIG_NAME), &file)
        .map_err(|e| StepFunError::Storage(format!("failed to save credentials: {}", e)))?;

    #[cfg(unix)]
    if let Ok(path) = confy::get_configuration_file_path(APP_NAME, Some(CONFIG_NAME)) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Load the stored credentials.
pub fn load() -> Result<Credentials> {
    let file: CredentialsFile = confy::load(APP_NAME, Some(CONFIG_NAME))
        .map_err(|e| StepFunError::Storage(format!("failed to read credentials: {}", e)))?;
    if file.token.is_empty() || file.webid.is_empty() {
        return Err(StepFunError::NotAuthenticated);
    }
    Ok((&file).into())
}

/// Forget the stored credentials.
pub fn clear() -> Result<()> {
    confy::store(APP_NAME, Some(CONFIG_NAME), CredentialsFile::default())
        .map_err(|e| StepFunError::Storage(format!("failed to clear credentials: {}", e)))?;
    if let Ok(path) = confy::get_configuration_file_path(APP_NAME, Some(CONFIG_NAME)) {
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}

/// Path of the credential file, for error messages.
pub fn path() -> Option<String> {
    confy::get_configuration_file_path(APP_NAME, Some(CONFIG_NAME))
        .ok()
        .map(|p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_round_trip_without_losing_the_token() {
        let stored = Credentials {
            username: "13812348888".into(),
            token: " a.b...c.d ".into(),
            webid: " device-1 ".into(),
        };
        let file: CredentialsFile = (&stored).into();
        // The token halves and the device id are trimmed; the username is kept
        // as it was typed.
        assert_eq!(file.token, "a.b...c.d");
        assert_eq!(file.webid, "device-1");

        let back: Credentials = (&file).into();
        assert_eq!(back.username, "13812348888");
        assert_eq!(back.token, "a.b...c.d");
        assert_eq!(back.webid, "device-1");
    }
}
