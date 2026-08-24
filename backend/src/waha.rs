// backend/src/waha.rs - WAHA client helper with sendSeen, typing presence, dynamic delay, and stopTyping

use rand::Rng;
use std::time::Duration;
use crate::models::SendTextRequest;

#[derive(Debug, serde::Serialize)]
pub struct ChatActionRequest {
    pub session: String,
    #[serde(rename = "chatId")]
    pub chat_id: String,
    #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Send POST /api/sendSeen to WAHA to mark incoming message/chat as read
pub async fn send_seen(chat_id: &str, message_id: Option<&str>) {
    let waha_url = format!("{}/api/sendSeen", std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()));
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = ChatActionRequest {
        session: "default".to_string(),
        chat_id: chat_id.to_string(),
        message_id: message_id.map(|s| s.to_string()),
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

/// Send POST /api/startTyping to WAHA (best-effort, non-blocking on failure)
pub async fn start_typing(chat_id: &str) {
    let waha_url = format!("{}/api/startTyping", std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()));
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = ChatActionRequest {
        session: "default".to_string(),
        chat_id: chat_id.to_string(),
        message_id: None,
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
    
    let payload = ChatActionRequest {
        session: "default".to_string(),
        chat_id: chat_id.to_string(),
        message_id: None,
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

/// Dynamic typing delay: scales with message character length with random jitter (1.5 - 4.5s)
pub async fn apply_typing_delay_for_text(text: &str) {
    let base_ms = rand::thread_rng().gen_range(1200..=1800);
    let char_ms = (text.chars().count() as u64) * 8;
    let total_ms = (base_ms + char_ms).clamp(1500, 4500);
    tokio::time::sleep(Duration::from_millis(total_ms)).await;
}

/// Default random artificial delay between 1500ms and 3000ms
pub async fn apply_typing_delay() {
    let delay_ms = rand::thread_rng().gen_range(1500..=3000);
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
}

/// Custom random artificial delay between min_ms and max_ms
pub async fn apply_custom_delay(min_ms: u64, max_ms: u64) {
    let delay_ms = rand::thread_rng().gen_range(min_ms..=max_ms);
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
}

/// Send raw text to WAHA directly
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

/// Send reply with full 4-step sequence: sendSeen -> startTyping -> dynamic delay -> stopTyping -> sendText
pub async fn send_reply(chat_id: &str, text: &str) -> Result<(), String> {
    send_reply_with_id(chat_id, text, None).await
}

/// Send reply with message ID quote, sendSeen, startTyping, dynamic delay, stopTyping, and sendText
pub async fn send_reply_with_id(chat_id: &str, text: &str, reply_to: Option<String>) -> Result<(), String> {
    // 1. Send seen before processing / sending reply
    send_seen(chat_id, reply_to.as_deref()).await;
    
    // 2. Start typing in WAHA
    start_typing(chat_id).await;
    
    // 3. Dynamic human-like typing delay based on message size
    apply_typing_delay_for_text(text).await;
    
    // 4. Stop typing before sending message
    stop_typing(chat_id).await;
    
    // 5. Send message text
    send_raw_text(chat_id, text, reply_to, None).await
}

/// Send reply with mentions and full typing sequence
pub async fn send_reply_with_mentions(chat_id: &str, text: &str, mentions: Vec<String>) -> Result<(), String> {
    send_seen(chat_id, None).await;
    start_typing(chat_id).await;
    apply_typing_delay_for_text(text).await;
    stop_typing(chat_id).await;
    send_raw_text(chat_id, text, None, Some(mentions)).await
}
