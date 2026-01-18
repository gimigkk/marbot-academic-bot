// src/dashboard/mod.rs

pub mod handlers;
pub mod auth;

pub use handlers::{serve_dashboard_page, get_dashboard_data};
pub use auth::basic_auth_middleware;