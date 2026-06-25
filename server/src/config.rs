use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub api_port: u16,
    pub metube_url: String,
    pub downloads_dir: PathBuf,
    pub peertube_import_dir: PathBuf,
    pub database_url: String,
    pub oidc_issuer_url: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub oidc_redirect_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, anyhow::Error> {
        dotenvy::dotenv().ok();
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
            oidc_issuer_url: std::env::var("OIDC_ISSUER_URL")?,
            oidc_client_id: std::env::var("OIDC_CLIENT_ID")?,
            oidc_client_secret: std::env::var("OIDC_CLIENT_SECRET")?,
            oidc_redirect_url: std::env::var("OIDC_REDIRECT_URL")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_required_vars() {
        std::env::set_var("DATABASE_URL", "sqlite:///tmp/test.db");
        std::env::set_var("OIDC_ISSUER_URL", "https://auth.example.com");
        std::env::set_var("OIDC_CLIENT_ID", "tubemin");
        std::env::set_var("OIDC_CLIENT_SECRET", "secret");
        std::env::set_var("OIDC_REDIRECT_URL", "https://tubemin.example.com/auth/callback");

        let config = Config::from_env().unwrap();
        assert_eq!(config.api_port, 3000);
        assert_eq!(config.metube_url, "http://metube:8081");
        assert_eq!(config.downloads_dir, std::path::PathBuf::from("/downloads"));
    }
}
