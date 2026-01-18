// src/dashboard/auth.rs

use axum::{
    body::Body,
    extract::Request,
    http::{StatusCode, HeaderMap},
    middleware::Next,
    response::{Response},
};
use base64::{Engine as _, engine::general_purpose};

/// Check if credentials are valid
fn check_credentials(headers: &HeaderMap) -> bool {
    // Get credentials from environment
    let expected_user = std::env::var("DASHBOARD_USER")
        .unwrap_or_else(|_| "admin".to_string());
    let expected_pass = std::env::var("DASHBOARD_PASS")
        .unwrap_or_else(|_| "changeme".to_string());

    // Get Authorization header
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok());

    if let Some(auth) = auth_header {
        // Check if it's Basic auth
        if let Some(credentials) = auth.strip_prefix("Basic ") {
            // Decode base64
            if let Ok(decoded) = general_purpose::STANDARD.decode(credentials) {
                if let Ok(credentials_str) = String::from_utf8(decoded) {
                    // Split username:password
                    if let Some((username, password)) = credentials_str.split_once(':') {
                        // Check credentials
                        return username == expected_user && password == expected_pass;
                    }
                }
            }
        }
    }
    
    false
}

/// Create 401 Unauthorized response
fn unauthorized_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("WWW-Authenticate", "Basic realm=\"MARBOT Dashboard\"")
        .body(Body::from("Authentication required"))
        .unwrap()
}

/// Middleware for HTTP Basic Authentication
pub async fn basic_auth_middleware(
    request: Request,
    next: Next,
) -> Response {
    // Check credentials
    if check_credentials(request.headers()) {
        // Valid - proceed with request
        next.run(request).await
    } else {
        // Invalid - return 401
        unauthorized_response()
    }
}