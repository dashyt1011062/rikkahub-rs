use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::AppState;

const WEB_JWT_ISSUER: &str = "rikkahub-web";
const WEB_JWT_AUDIENCE: &str = "rikkahub-web-client";
const WEB_JWT_SUBJECT: &str = "web-access";
const WEB_JWT_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;
const WEB_ACCESS_TOKEN_QUERY_KEY: &str = "access_token";

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug)]
pub struct AccountId(pub String);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebAuthTokenRequest {
    #[serde(default)]
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebAuthTokenResponse {
    pub token: String,
    pub expires_at: u64,
}

pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> AppResult<Response> {
    if !state.config.jwt_enabled {
        request.extensions_mut().insert(AccountId(crate::config::DEFAULT_WEB_ACCOUNT_ID.to_string()));
        return Ok(next.run(request).await);
    }

    if state.config.effective_accounts().is_empty() {
        return Err(AppError::forbidden("Access password is not configured"));
    }

    let token = extract_access_token(&request).ok_or_else(|| AppError::unauthorized("Unauthorized"))?;
    let account_id = verify_web_jwt(&state.config, token)?;
    request.extensions_mut().insert(AccountId(account_id));
    Ok(next.run(request).await)
}

pub fn create_web_jwt(config: &AppConfig, username: &str) -> AppResult<WebAuthTokenResponse> {
    let now = now_seconds();
    let expires_at_seconds = now + WEB_JWT_TTL_SECONDS;
    let expires_at_millis = expires_at_seconds * 1000;

    let header = json!({
        "alg": "HS256",
        "typ": "JWT",
    });
    let payload = json!({
        "iss": WEB_JWT_ISSUER,
        "aud": WEB_JWT_AUDIENCE,
        "sub": WEB_JWT_SUBJECT,
        "account": username,
        "iat": now,
        "exp": expires_at_seconds,
    });

    let header = encode_segment(&serde_json::to_vec(&header)?);
    let payload = encode_segment(&serde_json::to_vec(&payload)?);
    let signing_input = format!("{header}.{payload}");
    let signature = sign(&config.jwt_secret(), signing_input.as_bytes())?;

    Ok(WebAuthTokenResponse {
        token: format!("{signing_input}.{signature}"),
        expires_at: expires_at_millis,
    })
}

pub fn verify_web_jwt(config: &AppConfig, token: &str) -> AppResult<String> {
    let mut segments = token.split('.');
    let header = segments.next().ok_or_else(|| AppError::unauthorized("Unauthorized"))?;
    let payload = segments.next().ok_or_else(|| AppError::unauthorized("Unauthorized"))?;
    let signature = segments.next().ok_or_else(|| AppError::unauthorized("Unauthorized"))?;
    if segments.next().is_some() {
        return Err(AppError::unauthorized("Unauthorized"));
    }

    let signing_input = format!("{header}.{payload}");
    let expected_signature = sign(&config.jwt_secret(), signing_input.as_bytes())?;
    if !constant_time_eq(signature.as_bytes(), expected_signature.as_bytes()) {
        return Err(AppError::unauthorized("Unauthorized"));
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| AppError::unauthorized("Unauthorized"))?;
    let claims: Value = serde_json::from_slice(&payload_bytes).map_err(|_| AppError::unauthorized("Unauthorized"))?;

    if claims.get("iss").and_then(Value::as_str) != Some(WEB_JWT_ISSUER) {
        return Err(AppError::unauthorized("Unauthorized"));
    }
    if claims.get("sub").and_then(Value::as_str) != Some(WEB_JWT_SUBJECT) {
        return Err(AppError::unauthorized("Unauthorized"));
    }
    if !audience_matches(claims.get("aud")) {
        return Err(AppError::unauthorized("Unauthorized"));
    }
    let exp = claims.get("exp").and_then(Value::as_u64).ok_or_else(|| AppError::unauthorized("Unauthorized"))?;
    if exp <= now_seconds() {
        return Err(AppError::unauthorized("Unauthorized"));
    }

    let account = claims
        .get("account")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::unauthorized("Unauthorized"))?;
    if config.find_account(account).is_none() {
        return Err(AppError::unauthorized("Unauthorized"));
    }

    Ok(account.to_string())
}

pub fn secure_password_eq(left: &str, right: &str) -> bool {
    constant_time_eq(left.as_bytes(), right.as_bytes())
}

fn extract_access_token(request: &Request<Body>) -> Option<&str> {
    extract_bearer(request)
        .or_else(|| extract_query_value(request.uri().query().unwrap_or_default(), WEB_ACCESS_TOKEN_QUERY_KEY))
}

fn extract_bearer(request: &Request<Body>) -> Option<&str> {
    let raw = request.headers().get(AUTHORIZATION)?.to_str().ok()?.trim();
    raw.strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn extract_query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if name == key && !value.is_empty() {
            Some(value)
        } else {
            None
        }
    })
}

fn audience_matches(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => text == WEB_JWT_AUDIENCE,
        Some(Value::Array(items)) => items.iter().any(|item| item.as_str() == Some(WEB_JWT_AUDIENCE)),
        _ => false,
    }
}

fn encode_segment(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn sign(secret: &str, input: &[u8]) -> AppResult<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|error| AppError::internal(format!("failed to create hmac: {error}")))?;
    mac.update(input);
    Ok(encode_segment(&mac.finalize().into_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

