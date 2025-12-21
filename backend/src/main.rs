use axum::{
    extract::State,
    routing::post,
    Json,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;


// Track recent message IDs to avoid duplicates
type MessageCache = Arc<Mutex<HashSet<String>>>;

#[derive(Debug, Deserialize)]
struct WebhookPayload {
    event: String,
    session: String,
    payload: MessagePayload,
}

#[derive(Debug, Deserialize)]
struct MessagePayload {
    #[serde(default)]
    id: String,
    #[serde(default)]
    body: String,
    from: String,
    #[serde(default)]
    #[serde(rename = "fromMe")]
    from_me: bool,
    #[serde(default)]
    #[serde(rename = "chatId")]
    #[serde(flatten)]
    extra: Value,
}

#[derive(Debug, Serialize)]
struct SendTextRequest {
    #[serde(rename = "chatId")]
    chat_id: String,
    text: String,
    session: String,
}


async fn webhook(
    State(cache): State<MessageCache>,
    Json(payload): Json<WebhookPayload>,
) {
    // Skip non-message.any events
    if payload.event != "message.any" {
        return;
    }

    // DEBUG: Print the full payload structure
    // println!("🔍 DEBUG payload.payload: {:?}", payload.payload);
    

    // kadang waha ngirim webhook 2x
    // gw juga bingung, fix later aja ini masih workaround
    // Create composite deduplication key: timestamp + from + body
    let dedup_key = format!(
        "{}:{}:{}",
        payload.payload.id,
        payload.payload.from,
        payload.payload.body.chars().take(50).collect::<String>() // First 50 chars
    );
    
    // Deduplicate
    {
        let mut cache = cache.lock().unwrap();
        if cache.contains(&dedup_key) {
            println!("⏭️  Skipping duplicate message");
            return;
        }
        cache.insert(dedup_key);
        
        if cache.len() > 100 {
            cache.clear();
        }
    }
    
    // Skip messages sent BY us (via WhatsApp Web or bot replies)
    // UNLESS it's a command (starts with #) for self-testing
    if payload.payload.from_me {
        // Allow self-testing with commands
        if !payload.payload.body.trim().starts_with('#') {
            println!("⏭️  Ignoring own message: {}", payload.payload.body);
            return;
        }
        println!("🔧 Processing own command for testing: {}", payload.payload.body);
    }
    
    println!("📨 Message from {}: {}", payload.payload.from, payload.payload.body);

    let response_text = match payload.payload.body.trim() {
        "#ping" => {
            println!("✅ PING command detected");
            Some("pong")
        }
        _ => {
            println!("ℹ️  Not a recognized command, ignoring");
            None
        }
    };

    if let Some(text) = response_text {
        match send_reply(&payload.payload.from, text).await {
            Ok(_) => println!("✅ Reply sent successfully"),
            Err(e) => eprintln!("❌ Failed to send reply: {}", e),
        }
    }
}

async fn send_reply(chat_id: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let waha_url = "http://localhost:3001/api/sendText";
    
    let payload = SendTextRequest {
        chat_id: chat_id.to_string(),
        text: text.to_string(),
        session: "default".to_string(),
    };

    println!("📤 Sending to WAHA: {} -> '{}'", chat_id, text);

    let client = reqwest::Client::new();
    let response = client
        .post(waha_url)
        .header("X-Api-Key", "devkey123")
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
    
    let cache: MessageCache = Arc::new(Mutex::new(HashSet::new()));
    
    let app = Router::new()
        .route("/webhook", post(webhook))
        .with_state(cache);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("👂 Listening on {}", addr);
    println!("📍 Webhook endpoint: http://localhost:3000/webhook");

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}