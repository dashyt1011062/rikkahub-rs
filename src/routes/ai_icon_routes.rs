use axum::extract::Query;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::IntoResponse;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct IconQuery {
    name: Option<String>,
}

pub async fn icon(Query(query): Query<IconQuery>) -> impl IntoResponse {
    let name = query.name.unwrap_or_else(|| "AI".to_string());
    let label = initial(&name);
    let (bg, fg) = colors_for_name(&name);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="{label}">
<rect width="64" height="64" rx="14" fill="#{bg}"/>
<text x="32" y="40" text-anchor="middle" font-family="Inter,Arial,sans-serif" font-size="26" font-weight="700" fill="#{fg}">{label}</text>
</svg>"##
    );
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("image/svg+xml; charset=utf-8"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("public, max-age=604800"));
    (headers, svg)
}

fn initial(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "A".to_string();
    }
    trimmed
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .unwrap_or('A')
        .to_uppercase()
        .collect::<String>()
}

fn colors_for_name(name: &str) -> (&'static str, &'static str) {
    const PALETTE: [(&str, &str); 10] = [
        ("2563EB", "FFFFFF"),
        ("059669", "FFFFFF"),
        ("DC2626", "FFFFFF"),
        ("7C3AED", "FFFFFF"),
        ("0F766E", "FFFFFF"),
        ("D97706", "111827"),
        ("DB2777", "FFFFFF"),
        ("4F46E5", "FFFFFF"),
        ("65A30D", "111827"),
        ("0891B2", "FFFFFF"),
    ];
    let hash = name
        .bytes()
        .fold(0usize, |acc, byte| acc.wrapping_mul(31).wrapping_add(byte as usize));
    PALETTE[hash % PALETTE.len()]
}
