use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::TcpListener;

// Incoming webhook payload from WAHA
#[derive(Debug, Deserialize)]
struct WebhookPayload {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    body: String,
    from: String,
}

// Outgoing message payload to WAHA
#[derive(Debug, Serialize)]
struct SendTextRequest {
    #[serde(rename = "chatId")]
    chat_id: String,
    text: String,
}

// Main webhook handler
// Semua message bakal masuk ke sini dulu
async fn webhook(Json(payload): Json<WebhookPayload>) {
    println!("📨 Received message from {}: {}", payload.message.from, payload.message.body);

    // Check if message is a command
    let response_text = match payload.message.body.trim() {
        "#ping" => {
            println!("✅ PING command detected");
            Some("pong")
        }
        _ => {
            println!("ℹ️  Not a recognized command, ignoring");
            None
        }
    };

    // Send reply if we have a response
    if let Some(text) = response_text {
        match send_reply(&payload.message.from, text).await {
            Ok(_) => println!("✅ Reply sent successfully"),
            Err(e) => eprintln!("❌ Failed to send reply: {}", e),
        }
    }
}

// Function to send message via WAHA API
async fn send_reply(chat_id: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let waha_url = "http://localhost:3001/api/sendText";
    
    let payload = SendTextRequest {
        chat_id: chat_id.to_string(),
        text: text.to_string(),
    };

    println!("📤 Sending to WAHA: {} -> '{}'", chat_id, text);

    let client = reqwest::Client::new();
    let response = client
        .post(waha_url)
        .json(&payload)
        .send()
        .await?;

    if response.status().is_success() {
        println!("✅ WAHA API responded successfully");
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await?;
        eprintln!("❌ WAHA API error: {} - {}", status, body);
        Err(format!("WAHA API error: {}", status).into())
    }
}

#[tokio::main]
async fn main() {
    println!("🚀 Starting WhatsApp Academic Bot");
    
    let app = Router::new()
        .route("/webhook", post(webhook));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("👂 Listening on {}", addr);
    println!("📍 Webhook endpoint: http://localhost:3000/webhook");

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}