use axum::{
    extract::Request,
    http::{StatusCode, HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Instant, Duration};

const MAX_REQUESTS: u32 = 60; // requests per window
const WINDOW_SECS: u64 = 60;  // 1 minute sliding window

/// Lock-free concurrent rate limit map: user_id → (count, window_reset_instant)
pub type ApiRateLimiter = Arc<DashMap<String, (u32, Instant)>>;

pub fn new_rate_limiter() -> ApiRateLimiter {
    let limiter: ApiRateLimiter = Arc::new(DashMap::new());

    // Background cleanup: evict expired windows every 2 minutes to prevent memory leaks
    let limiter_clone = limiter.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(120));
        loop {
            interval.tick().await;
            let now = Instant::now();
            limiter_clone.retain(|_, (_, reset_time)| now < *reset_time);
        }
    });

    limiter
}

pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<ApiRateLimiter>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Auth middleware has already validated and injected user_id — use it as the rate limit key.
    // This prevents fake-token DoS because only verified users reach this middleware.
    let user_id = match req.extensions().get::<String>() {
        Some(uid) => uid.clone(),
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let now = Instant::now();
    let window = Duration::from_secs(WINDOW_SECS);

    let (count, remaining, reset_secs) = {
        let mut entry = limiter.entry(user_id).or_insert((0, now + window));
        let (count, reset_time) = entry.value_mut();

        if now > *reset_time {
            *count = 1;
            *reset_time = now + window;
        } else {
            *count += 1;
        }

        let remaining = MAX_REQUESTS.saturating_sub(*count);
        let reset_secs = reset_time.duration_since(now).as_secs();
        (*count, remaining, reset_secs)
    };

    if count > MAX_REQUESTS {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("X-RateLimit-Limit", HeaderValue::from_static("60"));
        headers.insert("X-RateLimit-Remaining", HeaderValue::from_static("0"));
        if let Ok(v) = HeaderValue::from_str(&reset_secs.to_string()) {
            headers.insert("X-RateLimit-Reset", v);
        }
        return Ok((StatusCode::TOO_MANY_REQUESTS, headers, "Rate limit exceeded.").into_response());
    }

    let mut res = next.run(req).await;
    res.headers_mut().insert("X-RateLimit-Limit", HeaderValue::from_static("60"));
    if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
        res.headers_mut().insert("X-RateLimit-Remaining", v);
    }
    if let Ok(v) = HeaderValue::from_str(&reset_secs.to_string()) {
        res.headers_mut().insert("X-RateLimit-Reset", v);
    }

    Ok(res)
}
