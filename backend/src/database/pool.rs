// src/database.rs

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::env;

/// Create database connection pool
pub async fn create_pool() -> Result<PgPool, sqlx::Error> {
    let database_url = env::var("DATABASE_URL")
        .expect("❌ DATABASE_URL must be set in .env file");

    println!("🔌 Connecting to database...");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    println!("✅ Database connected successfully!");

    Ok(pool)
}