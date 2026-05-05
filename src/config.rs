use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};

pub const DEFAULT_WEB_ACCOUNT_ID: &str = "2819915628";
pub const DEFAULT_ASSISTANT_ID: &str = "0950e2dc-9bd5-4801-afa3-aa887aa36b4e";

#[derive(Clone, Debug)]
pub struct WebAccount {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub web_ui_dir: PathBuf,
    pub jwt_enabled: bool,
    pub access_password: String,
    pub upload_max_bytes: u64,
    pub file_storage: String,
    pub imgpile_key: String,
    pub version: String,
    pub web_accounts: Vec<WebAccount>,
    pub max_remote_file_proxies: usize,
    pub max_remote_file_proxy_bytes: u64,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let host = env_value("HOST", "0.0.0.0");
        let port = env_value("PORT", "8080").parse::<u16>().unwrap_or(8080);
        let data_dir = PathBuf::from(env_value("DATA_DIR", "data"));
        let db_path = PathBuf::from(env_value(
            "DB_PATH",
            data_dir.join("rikka_hub.db").to_string_lossy().as_ref(),
        ));
        let web_ui_dir = PathBuf::from(env_value("WEB_UI_DIR", "dist/web-ui-static"));
        let jwt_enabled = env_bool("JWT_ENABLED", false);
        let access_password = env_value("ACCESS_PASSWORD", "");
        let upload_max_mb = env_value("UPLOAD_MAX_MB", "20").parse::<u64>().unwrap_or(20);
        let file_storage = env_value("FILE_STORAGE", "local");
        let imgpile_key = env_value("IMGPILE_KEY", &env_value("IMGPILE_TOKEN", ""));
        let version = env_value("APP_VERSION", "rust-dev");
        let web_accounts = parse_web_accounts(&access_password, &env_value("WEB_ACCOUNTS", ""));
        let max_remote_file_proxies = env_value("MAX_REMOTE_FILE_PROXIES", "10")
            .parse::<usize>()
            .unwrap_or(10)
            .max(1);
        let max_remote_file_proxy_bytes = env_value("MAX_REMOTE_FILE_PROXY_MB", "64")
            .parse::<u64>()
            .unwrap_or(64)
            .saturating_mul(1024 * 1024);

        Self {
            host,
            port,
            data_dir,
            db_path,
            web_ui_dir,
            jwt_enabled,
            access_password,
            upload_max_bytes: upload_max_mb.saturating_mul(1024 * 1024),
            file_storage,
            imgpile_key,
            version,
            web_accounts,
            max_remote_file_proxies,
            max_remote_file_proxy_bytes,
        }
    }

    pub fn bind_addr(&self) -> AppResult<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse::<SocketAddr>()
            .map_err(|error| AppError::internal(format!("invalid bind address: {error}")))
    }

    pub fn effective_accounts(&self) -> Vec<WebAccount> {
        if !self.web_accounts.is_empty() {
            return self.web_accounts.clone();
        }
        if self.access_password.trim().is_empty() {
            return Vec::new();
        }
        vec![WebAccount {
            username: DEFAULT_WEB_ACCOUNT_ID.to_string(),
            password: self.access_password.clone(),
        }]
    }

    pub fn jwt_secret(&self) -> String {
        if !self.access_password.is_empty() {
            return self.access_password.clone();
        }
        let joined = self
            .effective_accounts()
            .into_iter()
            .map(|account| format!("{}:{}", account.username, account.password))
            .collect::<Vec<_>>()
            .join("|");
        if joined.is_empty() {
            "__missing_password__".to_string()
        } else {
            joined
        }
    }

    pub fn find_account(&self, username: &str) -> Option<WebAccount> {
        self.effective_accounts()
            .into_iter()
            .find(|account| account.username == username)
    }

    pub fn settings_path_for_account(&self, account_id: &str) -> PathBuf {
        let account = account_id.trim();
        if account.is_empty() || account == DEFAULT_WEB_ACCOUNT_ID {
            return self.data_dir.join("settings.json");
        }
        self.data_dir
            .join("accounts")
            .join(sanitize_account_path_segment(account))
            .join("settings.json")
    }
}

fn env_value(key: &str, default: &str) -> String {
    env::var(key).ok().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

fn env_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .map(|value| value.eq_ignore_ascii_case("true") || value == "1" || value.eq_ignore_ascii_case("yes"))
        .unwrap_or(default)
}

fn parse_web_accounts(access_password: &str, raw: &str) -> Vec<WebAccount> {
    let mut accounts = Vec::<WebAccount>::new();
    if !access_password.trim().is_empty() {
        accounts.push(WebAccount {
            username: DEFAULT_WEB_ACCOUNT_ID.to_string(),
            password: access_password.to_string(),
        });
    }

    for entry in raw.split([',', ';', '\n']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some(index) = entry.find([':', '=']) else {
            continue;
        };
        let username = entry[..index].trim();
        let password = entry[index + 1..].trim();
        if username.is_empty() || password.is_empty() {
            continue;
        }
        if let Some(existing) = accounts.iter_mut().find(|account| account.username == username) {
            existing.password = password.to_string();
        } else {
            accounts.push(WebAccount {
                username: username.to_string(),
                password: password.to_string(),
            });
        }
    }

    accounts
}

fn sanitize_account_path_segment(account_id: &str) -> String {
    let sanitized: String = account_id
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    if sanitized.is_empty() {
        DEFAULT_WEB_ACCOUNT_ID.to_string()
    } else {
        sanitized
    }
}
