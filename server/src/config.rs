use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum AuthMode {
    Oidc,
    Password,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub api_port: u16,
    pub metube_url: String,
    pub downloads_dir: PathBuf,
    pub peertube_import_dir: PathBuf,
    pub database_url: String,
    pub auth_mode: AuthMode,
    pub admin_password: Option<String>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_redirect_url: Option<String>,
    pub peertube_url: Option<String>,
    pub peertube_host: Option<String>,
    pub peertube_username: Option<String>,
    pub peertube_password: Option<String>,
    pub peertube_admin_email: Option<String>,
    pub peertube_admin_username: Option<String>,
    pub peertube_admin_password: Option<String>,
    pub peertube_video_privacy: u8,
}

impl Config {
    pub fn from_env() -> Result<Self, anyhow::Error> {
        let auth_mode = match std::env::var("AUTH_MODE")
            .unwrap_or_else(|_| "oidc".into())
            .to_lowercase()
            .as_str()
        {
            "password" => AuthMode::Password,
            _ => AuthMode::Oidc,
        };

        let admin_password = std::env::var("ADMIN_PASSWORD").ok();
        if auth_mode == AuthMode::Password && admin_password.is_none() {
            return Err(anyhow::anyhow!(
                "ADMIN_PASSWORD must be set when AUTH_MODE=password"
            ));
        }

        let oidc_issuer_url = std::env::var("OIDC_ISSUER_URL").ok();
        let oidc_client_id = std::env::var("OIDC_CLIENT_ID").ok();
        let oidc_client_secret = std::env::var("OIDC_CLIENT_SECRET").ok();
        let oidc_redirect_url = std::env::var("OIDC_REDIRECT_URL").ok();

        if auth_mode == AuthMode::Oidc {
            if oidc_issuer_url.is_none() {
                return Err(anyhow::anyhow!("OIDC_ISSUER_URL required when AUTH_MODE=oidc"));
            }
            if oidc_client_id.is_none() {
                return Err(anyhow::anyhow!("OIDC_CLIENT_ID required when AUTH_MODE=oidc"));
            }
            if oidc_client_secret.is_none() {
                return Err(anyhow::anyhow!("OIDC_CLIENT_SECRET required when AUTH_MODE=oidc"));
            }
            if oidc_redirect_url.is_none() {
                return Err(anyhow::anyhow!("OIDC_REDIRECT_URL required when AUTH_MODE=oidc"));
            }
        }

        Ok(Config {
            api_port: std::env::var("API_PORT")
                .unwrap_or_else(|_| "3000".into())
                .parse()?,
            metube_url: std::env::var("METUBE_URL")
                .unwrap_or_else(|_| "http://metube:8081".into()),
            downloads_dir: PathBuf::from(
                std::env::var("DOWNLOADS_DIR").unwrap_or_else(|_| "/downloads".into()),
            ),
            peertube_import_dir: PathBuf::from(
                std::env::var("PEERTUBE_IMPORT_DIR")
                    .unwrap_or_else(|_| "/peertube-import".into()),
            ),
            database_url: std::env::var("DATABASE_URL")?,
            auth_mode,
            admin_password,
            oidc_issuer_url,
            oidc_client_id,
            oidc_client_secret,
            oidc_redirect_url,
            peertube_url: std::env::var("PEERTUBE_URL").ok(),
            peertube_host: std::env::var("PEERTUBE_HOST").ok(),
            peertube_username: std::env::var("PEERTUBE_USERNAME").ok(),
            peertube_password: std::env::var("PEERTUBE_PASSWORD").ok(),
            peertube_admin_email: std::env::var("PEERTUBE_ADMIN_EMAIL").ok(),
            peertube_admin_username: Some(
                std::env::var("PEERTUBE_ADMIN_USERNAME").unwrap_or_else(|_| "root".into())
            ),
            peertube_admin_password: std::env::var("PEERTUBE_ADMIN_PASSWORD").ok(),
            peertube_video_privacy: std::env::var("PEERTUBE_VIDEO_PRIVACY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Env vars are process-global — tests that mutate them must run serially.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn set_base_vars() {
        std::env::set_var("DATABASE_URL", "sqlite:///tmp/test.db");
        std::env::remove_var("API_PORT");
        std::env::remove_var("METUBE_URL");
        std::env::remove_var("DOWNLOADS_DIR");
        std::env::remove_var("PEERTUBE_IMPORT_DIR");
    }

    #[test]
    fn oidc_mode_loads_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_base_vars();
        std::env::set_var("AUTH_MODE", "oidc");
        std::env::remove_var("ADMIN_PASSWORD");
        std::env::set_var("OIDC_ISSUER_URL", "https://auth.example.com");
        std::env::set_var("OIDC_CLIENT_ID", "tubemin");
        std::env::set_var("OIDC_CLIENT_SECRET", "secret");
        std::env::set_var("OIDC_REDIRECT_URL", "https://tubemin.example.com/auth/callback");

        let config = Config::from_env().unwrap();
        assert_eq!(config.api_port, 3000);
        assert_eq!(config.metube_url, "http://metube:8081");
        assert_eq!(config.downloads_dir, std::path::PathBuf::from("/downloads"));
        assert!(matches!(config.auth_mode, AuthMode::Oidc));
    }

    #[test]
    fn password_mode_requires_admin_password() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_base_vars();
        std::env::set_var("AUTH_MODE", "password");
        std::env::remove_var("ADMIN_PASSWORD");
        assert!(Config::from_env().is_err());
    }

    #[test]
    fn password_mode_succeeds_with_password() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_base_vars();
        std::env::set_var("AUTH_MODE", "password");
        std::env::set_var("ADMIN_PASSWORD", "hunter2");
        std::env::remove_var("OIDC_ISSUER_URL");
        std::env::remove_var("OIDC_CLIENT_ID");
        std::env::remove_var("OIDC_CLIENT_SECRET");
        std::env::remove_var("OIDC_REDIRECT_URL");

        let config = Config::from_env().unwrap();
        assert!(matches!(config.auth_mode, AuthMode::Password));
        assert_eq!(config.admin_password.as_deref(), Some("hunter2"));
        assert!(config.oidc_issuer_url.is_none());
    }

    #[test]
    fn default_video_privacy_is_internal() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_base_vars();
        std::env::set_var("AUTH_MODE", "oidc");
        std::env::set_var("OIDC_ISSUER_URL", "https://auth.example.com");
        std::env::set_var("OIDC_CLIENT_ID", "tubemin");
        std::env::set_var("OIDC_CLIENT_SECRET", "secret");
        std::env::set_var("OIDC_REDIRECT_URL", "https://tubemin.example.com/auth/callback");
        std::env::remove_var("PEERTUBE_VIDEO_PRIVACY");

        let config = Config::from_env().unwrap();
        assert_eq!(config.peertube_video_privacy, 4);
    }

    #[test]
    fn custom_video_privacy_is_read() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_base_vars();
        std::env::set_var("AUTH_MODE", "oidc");
        std::env::set_var("OIDC_ISSUER_URL", "https://auth.example.com");
        std::env::set_var("OIDC_CLIENT_ID", "tubemin");
        std::env::set_var("OIDC_CLIENT_SECRET", "secret");
        std::env::set_var("OIDC_REDIRECT_URL", "https://tubemin.example.com/auth/callback");
        std::env::set_var("PEERTUBE_VIDEO_PRIVACY", "1");

        let config = Config::from_env().unwrap();
        assert_eq!(config.peertube_video_privacy, 1);
    }
}
