pub mod auth;
pub mod rate_limit;
pub mod routes;

use axum::{
    routing::get,
    Router,
    middleware,
};
use tower_http::cors::{CorsLayer, Any};
use axum::http::Method;
use crate::AppState;
use std::sync::Arc;
use sqlx::PgPool;

pub fn api_router(pool: PgPool, api_key_cache: auth::ApiKeyCache) -> Router<AppState> {
    let shared_pool = Arc::new(pool);
    let rate_limiter = rate_limit::new_rate_limiter();

    // CORS: allow any origin for API consumers (Notion, scripts, web dashboards, etc.)
    let cors = CorsLayer::new()
        .allow_methods([Method::GET])
        .allow_headers(Any)
        .allow_origin(Any);

    Router::new()
        .route("/assignments", get(routes::get_assignments))
        // Middleware runs in REVERSE order of .route_layer() attachment:
        // 2nd attached = outermost = runs FIRST: auth validates the token
        // 1st attached = innermost = runs SECOND: rate limit on verified user_id only
        .route_layer(middleware::from_fn_with_state(rate_limiter, rate_limit::rate_limit_middleware))
        .route_layer(middleware::from_fn_with_state((shared_pool, api_key_cache), auth::auth_middleware))
        .layer(cors)
}
