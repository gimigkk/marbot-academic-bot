use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use std::time::Duration;
use moka::future::Cache;
use sqlx::PgPool;

/// In-memory cache: api_key → user_id (5 min TTL)
/// This means last_used_at is effectively updated at most once per 5 min per key (on cache miss),
/// avoiding per-request DB writes.
pub type ApiKeyCache = Arc<Cache<String, String>>;

pub fn new_api_key_cache() -> ApiKeyCache {
    Arc::new(
        Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(300)) // 5 min TTL
            .build(),
    )
}

pub async fn auth_middleware(
    axum::extract::State((pool, cache)): axum::extract::State<(Arc<PgPool>, ApiKeyCache)>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req.headers().get(axum::http::header::AUTHORIZATION);

    let bearer: String = match auth_header {
        Some(header) => match header.to_str() {
            Ok(s) if s.starts_with("Bearer ") => {
                let token = s[7..].trim();
                // Fix #10: reject tokens that are clearly invalid length
                if token.is_empty() || token.len() > 256 {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                token.to_string()
            }
            _ => return Err(StatusCode::UNAUTHORIZED),
        },
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // 1. Check in-memory cache first (zero DB cost)
    let user_id = if let Some(cached_uid) = cache.get(&bearer).await {
        cached_uid
    } else {
        // 2. Cache miss — hit DB and update last_used_at atomically here
        match crate::database::crud::get_and_touch_api_key(&pool, &bearer).await {
            Ok(Some(uid)) => {
                cache.insert(bearer.clone(), uid.clone()).await;
                uid
            }
            _ => return Err(StatusCode::UNAUTHORIZED),
        }
    };

    // 3. Inject validated user_id into request extensions for downstream handlers
    req.extensions_mut().insert(user_id);
    Ok(next.run(req).await)
}
