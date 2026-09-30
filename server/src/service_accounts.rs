use crate::db::{self, ServiceAccountRow};
use anyhow::{anyhow, Context, Result};
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::Utc;
use serde::Deserialize;
use std::{collections::HashSet, fs, path::Path, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceScope {
    CatalogRead,
    MediaRead,
    TransferComplete,
    TransferFail,
}

impl ServiceScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CatalogRead => "catalog:read",
            Self::MediaRead => "media:read",
            Self::TransferComplete => "transfer:complete",
            Self::TransferFail => "transfer:fail",
        }
    }
}

impl FromStr for ServiceScope {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "catalog:read" => Ok(Self::CatalogRead),
            "media:read" => Ok(Self::MediaRead),
            "transfer:complete" => Ok(Self::TransferComplete),
            "transfer:fail" => Ok(Self::TransferFail),
            other => Err(format!("unknown service-account scope: {other}")),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ServiceAccountManifest {
    #[serde(default)]
    pub service_accounts: Vec<ServiceAccountDefinition>,
}

#[derive(Debug, Deserialize)]
pub struct ServiceAccountDefinition {
    pub name: String,
    pub token_env: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ServicePrincipal {
    pub id: String,
    pub name: String,
    pub scopes: HashSet<ServiceScope>,
}

impl ServicePrincipal {
    pub fn has_scope(&self, scope: ServiceScope) -> bool {
        self.scopes.contains(&scope)
    }
}

pub struct RequireServiceAccount {
    pub principal: ServicePrincipal,
}

impl RequireServiceAccount {
    pub fn require(&self, scope: ServiceScope) -> Result<(), Response> {
        if self.principal.has_scope(scope) {
            Ok(())
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "service account lacks required scope",
                    "scope": scope.as_str(),
                })),
            )
                .into_response())
        }
    }
}

#[async_trait]
impl FromRequestParts<crate::state::AppState> for RequireServiceAccount {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|value| !value.is_empty());
        let Some(token) = token else {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "missing service account token" })),
            )
                .into_response());
        };

        match authenticate(&state.pool, token).await {
            Ok(Some(principal)) => Ok(Self { principal }),
            Ok(None) => Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid service account token" })),
            )
                .into_response()),
            Err(error) => {
                tracing::error!(error = %error, "service account authentication failed");
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "service account authentication failed" })),
                )
                    .into_response())
            }
        }
    }
}

pub fn parse_manifest_text(text: &str) -> Result<ServiceAccountManifest> {
    Ok(toml::from_str(text).context("service-account manifest is invalid TOML")?)
}

pub async fn sync_manifest(pool: &sqlx::SqlitePool, path: &Path) -> Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read service-account manifest {}", path.display()))?;
    let manifest = parse_manifest_text(&text)?;
    let mut names = HashSet::new();

    for definition in manifest.service_accounts {
        let name = definition.name.trim();
        if name.is_empty() {
            return Err(anyhow!("service-account name cannot be empty"));
        }
        if !names.insert(name.to_string()) {
            return Err(anyhow!("duplicate service-account name: {name}"));
        }
        let token = std::env::var(&definition.token_env).with_context(|| {
            format!("token environment variable is missing: {}", definition.token_env)
        })?;
        if token.is_empty() {
            return Err(anyhow!("service-account token cannot be empty: {name}"));
        }
        let scopes = definition
            .scopes
            .iter()
            .map(|scope| ServiceScope::from_str(scope))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| anyhow!(error))?;
        if scopes.is_empty() {
            return Err(anyhow!("service-account must have at least one scope: {name}"));
        }

        let now = Utc::now().to_rfc3339();
        let account = ServiceAccountRow {
            id: format!("service:{name}"),
            name: name.to_string(),
            token_hash: hash(token, DEFAULT_COST)?,
            scopes_json: serde_json::to_string(
                &scopes.iter().map(|scope| scope.as_str()).collect::<Vec<_>>(),
            )?,
            enabled: definition.enabled,
            managed_by: "manifest".into(),
            last_used_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        db::upsert_service_account(pool, &account).await?;
    }
    Ok(())
}

pub async fn authenticate(
    pool: &sqlx::SqlitePool,
    plaintext: &str,
) -> Result<Option<ServicePrincipal>> {
    let accounts = db::list_service_accounts(pool).await?;
    let plaintext = plaintext.to_string();
    let matched = tokio::task::spawn_blocking(move || {
        accounts.into_iter().find_map(|account| {
            if !account.enabled || !verify(&plaintext, &account.token_hash).unwrap_or(false) {
                return None;
            }
            let scopes = serde_json::from_str::<Vec<String>>(&account.scopes_json)
                .ok()?
                .into_iter()
                .filter_map(|scope| ServiceScope::from_str(&scope).ok())
                .collect();
            Some((account, scopes))
        })
    })
    .await
    .map_err(|error| anyhow!("service-account verification task failed: {error}"))?;

    let Some((account, scopes)) = matched else {
        return Ok(None);
    };
    db::touch_service_account(pool, &account.id, &Utc::now().to_rfc3339()).await?;
    Ok(Some(ServicePrincipal {
        id: account.id,
        name: account.name,
        scopes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::fs;

    #[test]
    fn parses_manifest_with_env_token_reference_and_scopes() {
        let manifest = parse_manifest_text(
            r#"
                [[service_accounts]]
                name = "converttube"
                token_env = "TUBEMIN_CONVERTTUBE_TOKEN"
                scopes = ["catalog:read", "media:read"]
                enabled = true
            "#,
        )
        .unwrap();

        assert_eq!(manifest.service_accounts.len(), 1);
        assert_eq!(manifest.service_accounts[0].name, "converttube");
        assert_eq!(manifest.service_accounts[0].token_env, "TUBEMIN_CONVERTTUBE_TOKEN");
        assert_eq!(manifest.service_accounts[0].scopes, vec!["catalog:read", "media:read"]);
    }

    #[tokio::test]
    async fn sync_manifest_hashes_tokens_and_is_idempotent() {
        std::env::set_var("TUBEMIN_TEST_SERVICE_TOKEN", "test-token");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("service-accounts.toml");
        fs::write(
            &path,
            r#"
                [[service_accounts]]
                name = "converttube"
                token_env = "TUBEMIN_TEST_SERVICE_TOKEN"
                scopes = ["catalog:read"]
                enabled = true
            "#,
        )
        .unwrap();
        let pool = db::init("sqlite::memory:").await.unwrap();

        sync_manifest(&pool, &path).await.unwrap();
        sync_manifest(&pool, &path).await.unwrap();

        let accounts = db::list_service_accounts(&pool).await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_ne!(accounts[0].token_hash, "test-token");
        assert!(bcrypt::verify("test-token", &accounts[0].token_hash).unwrap());
        std::env::remove_var("TUBEMIN_TEST_SERVICE_TOKEN");
    }

    #[tokio::test]
    async fn disabled_account_cannot_authenticate() {
        let account = ServiceAccountRow {
            id: "svc-disabled".into(),
            name: "disabled".into(),
            token_hash: bcrypt::hash("secret", bcrypt::DEFAULT_COST).unwrap(),
            scopes_json: "[\"catalog:read\"]".into(),
            enabled: false,
            managed_by: "manifest".into(),
            last_used_at: None,
            created_at: "2026-09-30T00:00:00Z".into(),
            updated_at: "2026-09-30T00:00:00Z".into(),
        };
        let pool = db::init("sqlite::memory:").await.unwrap();
        db::upsert_service_account(&pool, &account).await.unwrap();

        assert!(authenticate(&pool, "secret").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn scope_check_rejects_missing_permission() {
        let principal = ServicePrincipal {
            id: "svc-convertube".into(),
            name: "converttube".into(),
            scopes: [ServiceScope::CatalogRead].into_iter().collect(),
        };

        assert!(principal.has_scope(ServiceScope::CatalogRead));
        assert!(!principal.has_scope(ServiceScope::MediaRead));
    }
}
