use axum::{
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use std::net::SocketAddr;
use tokio::net::TcpListener;



#[derive(Debug, Deserialize)]
struct WebhookPayload {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    body: String,
    from: String,
}

async fn webhook(Json(payload): Json<WebhookPayload>) {
    println!("Received message: {:?}", payload.message);

    if payload.message.body.trim() == "#ping" {
        println!("PING received from {}", payload.message.from);
    }
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/webhook", post(webhook));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on {}", addr);

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
