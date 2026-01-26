// src/dashboard/auth.rs

use axum::{
    body::Body,
    extract::Request,
    http::{StatusCode, HeaderMap},
    middleware::Next,
    response::{Response},
};
use base64::{Engine as _, engine::general_purpose};


fn check_credentials(headers: &HeaderMap) -> bool {
  
    let expected_user = std::env::var("DASHBOARD_USER")
        .unwrap_or_else(|_| "admin".to_string());
    let expected_pass = std::env::var("DASHBOARD_PASS")
        .unwrap_or_else(|_| "changeme".to_string());

    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok());

    if let Some(auth) = auth_header {
   
        if let Some(credentials) = auth.strip_prefix("Basic ") {
           
            if let Ok(decoded) = general_purpose::STANDARD.decode(credentials) {
                if let Ok(credentials_str) = String::from_utf8(decoded) {
             
                    if let Some((username, password)) = credentials_str.split_once(':') {
                       
                        return username == expected_user && password == expected_pass;
                    }
                }
            }
        }
    }
    
    false
}

fn unauthorized_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("WWW-Authenticate", "Basic realm=\"MARBOT Dashboard\"")
        .body(Body::from("Authentication required"))
        .unwrap()
}


pub async fn basic_auth_middleware(
    request: Request,
    next: Next,
) -> Response {
    if check_credentials(request.headers()) {
        next.run(request).await
    } else {
        unauthorized_response()
    }
}