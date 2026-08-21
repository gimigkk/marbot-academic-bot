// backend/src/waha.rs - WAHA client helper with typing presence and artificial delay

use rand::Rng;
use std::time::Duration;
use crate::models::SendTextRequest;

#[derive(Debug, serde::Serialize)]
pub struct TypingRequest {
    pub session: String,
    #[serde(rename = "chatId")]
    pub chat_id: String,
}

/// Send POST /api/startTyping to WAHA (best-effort, non-blocking on failure)
pub async fn start_typing(chat_id: &str) {
    let waha_url = format!("{}/api/startTyping", std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()));
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = TypingRequest {
        session: "default".to_string(),
        chat_id: chat_id.to_string(),
    };
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();
        
    let _ = client.post(waha_url)
        .header("X-Api-Key", api_key)
        .json(&payload)
        .send()
        .await;
}

/// Send POST /api/stopTyping to WAHA (best-effort)
pub async fn stop_typing(chat_id: &str) {
    let waha_url = format!("{}/api/stopTyping", std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()));
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = TypingRequest {
        session: "default".to_string(),
        chat_id: chat_id.to_string(),
    };
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();
        
    let _ = client.post(waha_url)
        .header("X-Api-Key", api_key)
        .json(&payload)
        .send()
        .await;
}

/// Sleeps for a random artificial delay between 1500ms and 3000ms (1.5 - 3.0s)
pub async fn apply_typing_delay() {
    let delay_ms = rand::thread_rng().gen_range(1500..=3000);
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
}

/// Sleeps for a custom random artificial delay between min_ms and max_ms
pub async fn apply_custom_delay(min_ms: u64, max_ms: u64) {
    let delay_ms = rand::thread_rng().gen_range(min_ms..=max_ms);
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
}

/// Send raw text to WAHA directly (no extra typing/delay)
pub async fn send_raw_text(
    chat_id: &str, 
    text: &str, 
    reply_to: Option<String>, 
    mentions: Option<Vec<String>>
) -> Result<(), String> {
    let waha_url = format!("{}/api/sendText", std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()));
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = SendTextRequest { 
        chat_id: chat_id.to_string(), 
        text: text.to_string(), 
        session: "default".to_string(),
        reply_to,
        mentions,
    };
    
    let client = reqwest::Client::new();
    let res = client.post(waha_url)
        .header("X-Api-Key", api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
        
    if res.status().is_success() {
        Ok(())
    } else {
        Err("API Error".to_string())
    }
}

/// Send reply with typing indicator and randomized artificial delay (1.5 - 3.0s)
pub async fn send_reply(chat_id: &str, text: &str) -> Result<(), String> {
    send_reply_with_id(chat_id, text, None).await
}

/// Send reply with message ID quote, typing indicator, and randomized delay
pub async fn send_reply_with_id(chat_id: &str, text: &str, reply_to: Option<String>) -> Result<(), String> {
    // 1. Trigger Typing State in WAHA
    start_typing(chat_id).await;
    
    // 2. Artificial Human-like Delay (1.5 - 3.0s)
    apply_typing_delay().await;
    
    // 3. Send message text
    send_raw_text(chat_id, text, reply_to, None).await
}

/// Send reply with mentions, typing indicator, and randomized delay
pub async fn send_reply_with_mentions(chat_id: &str, text: &str, mentions: Vec<String>) -> Result<(), String> {
    start_typing(chat_id).await;
    apply_typing_delay().await;
    send_raw_text(chat_id, text, None, Some(mentions)).await
}
