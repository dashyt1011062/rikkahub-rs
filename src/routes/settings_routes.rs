use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Extension, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::Stream;
use serde_json::{json, Value};

use crate::auth::AccountId;
use crate::error::AppResult;
use crate::settings_store;
use crate::AppState;

pub async fn replace(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(value): Json<Value>,
) -> AppResult<Json<Value>> {
    settings_store::write_settings(&state.config, &account.0, &value).await?;
    Ok(Json(json!({ "status": "ok" })))
}

pub async fn stream(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let config = state.config.clone();
    let account_id = account.0;

    let stream = stream! {
        let mut last_modified = None;
        loop {
            let modified = settings_store::settings_modified_at(&config, &account_id).await;
            if last_modified.is_none() || modified != last_modified {
                last_modified = modified;
                if let Ok(settings) = settings_store::read_settings(&config, &account_id).await {
                    let data = serde_json::to_string(&settings).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("update").data(data));
                }
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

