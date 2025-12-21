use axum::{
    extract::State,
    routing::post,
    Json,
    Router,
};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

mod models;
mod classifier;
mod parser;
mod whitelist;

use models::{MessageType, AIClassification, WebhookPayload, SendTextRequest, ForwardMessageRequest};
use classifier::classify_message;
use parser::commands::handle_command;
use parser::ai_extractor::extract_with_ai;
use whitelist::Whitelist;

// Track recent message IDs to avoid duplicates
type MessageCache = Arc<Mutex<HashSet<String>>>;

// Shared state
#[derive(Clone)]
struct AppState {
    cache: MessageCache,
    whitelist: Arc<Whitelist>,
}

async fn webhook(
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) {
    // Skip non-message.any events
    if payload.event != "message.any" {
        return;
    }

    // Deduplication key
    let dedup_key = format!(
        "{}:{}:{}",
        payload.payload.id,
        payload.payload.from,
        payload.payload.body.chars().take(50).collect::<String>()
    );
    
    // Deduplicate
    {
        let mut cache = state.cache.lock().unwrap();
        if cache.contains(&dedup_key) {
            println!("⏭️  Skipping duplicate message");
            return;
        }
        cache.insert(dedup_key);
        
        if cache.len() > 100 {
            cache.clear();
        }
    }
    
    // Skip messages sent BY us in DMs (but allow in channels for testing)
    // Channels show "fromMe: true" when YOU post in YOUR channel
    let is_channel = payload.payload.from.ends_with("@newsletter");
    let is_dm = payload.payload.from.ends_with("@c.us");
    
    if payload.payload.from_me && is_dm {
        // In DMs, only process commands when sent by us
        if !payload.payload.body.trim().starts_with('#') {
            println!("⏭️  Ignoring own DM message: {}", payload.payload.body);
            return;
        }
        println!("🔧 Processing own command for testing: {}", payload.payload.body);
    } else if payload.payload.from_me && is_channel {
        // In channels, process everything (even non-commands) for testing
        println!("📢 Processing message from your own channel: {}", payload.payload.body);
    }
    
    let separator = "=".repeat(60);
    println!("\n{}", separator);
    println!("📨 NEW MESSAGE");
    println!("{}", separator);
    println!("From: {}", payload.payload.from);
    
    // Help identify channel types
    if payload.payload.from.ends_with("@newsletter") {
        println!("   📢 Type: WhatsApp Channel/Newsletter");
    } else if payload.payload.from.ends_with("@g.us") {
        println!("   👥 Type: WhatsApp Group");
    } else if payload.payload.from.ends_with("@c.us") {
        println!("   💬 Type: Direct Message");
    }
    
    println!("Message ID: {}", payload.payload.id);
    println!("Body: {}", payload.payload.body);
    println!("From Me: {}", payload.payload.from_me);
    println!("{}\n", separator);

    // Step 1: Classify message (command or needs AI?)
    let message_type = classify_message(&payload.payload.body);
    let is_command = matches!(message_type, MessageType::Command(_));
    
    println!("🔍 Classification: {:?}", message_type);

    // Step 2: Check whitelist
    let (should_process, reason) = state.whitelist.should_process(&payload.payload.from, is_command);
    
    if !should_process {
        println!("🚫 Ignoring message: {} (from: {})", reason, payload.payload.from);
        println!("   💡 This chat is not whitelisted for academic info");
        println!("   💡 Add to ACADEMIC_CHANNELS in .env to enable\n");
        return;
    }
    
    println!("✅ Processing allowed: {}", reason);

    let response_text = match message_type {
        MessageType::Command(cmd) => {
            // Handle bot command immediately
            Some(handle_command(cmd, &payload.payload.from))
        }
        
        MessageType::NeedsAI(text) => {
            // Send to AI for classification
            match extract_with_ai(&text).await {
                Ok(classification) => handle_ai_classification(
                    classification,
                    &payload.payload.from,
                    &payload.payload.id,
                    &payload.payload.from,
                ),
                Err(e) => {
                    let error_msg = e.to_string();
                    eprintln!("❌ AI extraction failed: {}", error_msg);
                    
                    // Check if it's a rate limit error
                    if error_msg.contains("429") || error_msg.contains("Too Many Requests") {
                        Some("⏳ AI rate limit reached. Please try again in a minute.".to_string())
                    } else {
                        Some("Sorry, I couldn't process that message. Please try again.".to_string())
                    }
                }
            }
        }
    };

    // Step 3: Send reply if we have one
    if let Some(text) = response_text {
        match send_reply(&payload.payload.from, &text).await {
            Ok(_) => println!("✅ Reply sent successfully\n"),
            Err(e) => eprintln!("❌ Failed to send reply: {}\n", e),
        }
    } else {
        println!("ℹ️  No reply needed\n");
    }
}

/// Handle AI classification result
fn handle_ai_classification(
    classification: AIClassification, 
    _user_phone: &str,
    message_id: &str,
    chat_id: &str,
) -> Option<String> {
    println!("\n🤖 AI Classification Result: {:?}", classification);
    
    match classification {
        AIClassification::AssignmentInfo { title, due_date, description, .. } => {
            println!("📚 ASSIGNMENT DETECTED");
            println!("   Title: {}", title);
            println!("   Due: {:?}", due_date);
            println!("   Description: {}", description);
            println!("   Message ID: {}", message_id);
            println!("   Chat ID: {}", chat_id);
            println!("   ⚠️  Would save to database here!");
            
            let due_text = due_date.unwrap_or_else(|| "No due date".to_string());
            Some(format!(
                "✅ *Assignment Saved!*\n\n\
                📝 {}\n\
                📅 Due: {}\n\
                📄 {}\n\n\
                Use #expand <id> to see the original message.",
                title, due_text, description
            ))
        }
        
        AIClassification::CourseInfo { content, .. } => {
            println!("ℹ️  COURSE INFO DETECTED");
            println!("   Content: {}", content);
            println!("   Message ID: {}", message_id);
            println!("   Chat ID: {}", chat_id);
            println!("   ⚠️  Would save to database here!");
            
            Some(format!(
                "📚 *Course Info Noted*\n\n\
                {}\n\n\
                (This will be organized once database is connected)",
                content
            ))
        }
        
        AIClassification::AssignmentReminder { assignment_reference } => {
            println!("⏰ REMINDER DETECTED");
            println!("   Reference: {}", assignment_reference);
            
            Some(format!(
                "⏰ Got it!\n\n\
                I'll remind you about: {}\n\
                (Reminders will work once database is connected)",
                assignment_reference
            ))
        }
        
        AIClassification::FailedCommand { attempted_command, suggestion } => {
            println!("❓ FAILED COMMAND");
            println!("   Attempted: {}", attempted_command);
            println!("   Suggestion: {}", suggestion);
            
            Some(format!(
                "❓ I didn't recognize that command.\n\n\
                You tried: {}\n\
                💡 Suggestion: {}\n\n\
                Type #help to see all commands.",
                attempted_command, suggestion
            ))
        }
        
        AIClassification::Unrecognized => {
            println!("🤷 UNRECOGNIZED MESSAGE - No reply");
            None
        }
    }
}

async fn send_reply(chat_id: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let waha_url = "http://localhost:3001/api/sendText";
    let api_key = std::env::var("WAHA_API_KEY")
        .unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = SendTextRequest {
        chat_id: chat_id.to_string(),
        text: text.to_string(),
        session: "default".to_string(),
    };

    println!("📤 Sending to WAHA: {} -> '{}'", chat_id, text);

    let client = reqwest::Client::new();
    let response = client
        .post(waha_url)
        .header("X-Api-Key", api_key)
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

#[allow(dead_code)]
async fn forward_message(
    to_chat_id: &str,
    message_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let waha_url = "http://localhost:3001/api/sendMessage";
    let api_key = std::env::var("WAHA_API_KEY")
        .unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = ForwardMessageRequest {
        chat_id: to_chat_id.to_string(),
        message_id: message_id.to_string(),
        session: "default".to_string(),
    };

    println!("📨 Forwarding message {} to {}", message_id, to_chat_id);

    let client = reqwest::Client::new();
    let response = client
        .post(waha_url)
        .header("X-Api-Key", api_key)
        .json(&payload)
        .send()
        .await?;

    if response.status().is_success() {
        println!("✅ Message forwarded successfully");
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await?;
        eprintln!("❌ WAHA forward error: {} - {}", status, body);
        Err(format!("WAHA forward error: {}", status).into())
    }
}

#[tokio::main]
async fn main() {
    // Load .env file
    dotenv::dotenv().ok();
    
    println!("🚀 Starting WhatsApp Academic Bot (with AI Classification)");
    
    // Check environment variables
    if std::env::var("GEMINI_API_KEY").is_err() {
        eprintln!("⚠️  WARNING: GEMINI_API_KEY not set! AI features will not work.");
    }
    
    // Initialize whitelist
    let whitelist = Arc::new(Whitelist::new());
    
    let cache: MessageCache = Arc::new(Mutex::new(HashSet::new()));
    let state = AppState { cache, whitelist };
    
    let app = Router::new()
        .route("/webhook", post(webhook))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let separator = "=".repeat(60);
    println!("👂 Listening on {}", addr);
    println!("📍 Webhook endpoint: http://localhost:3000/webhook");
    println!("\n{}\n", separator);

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}