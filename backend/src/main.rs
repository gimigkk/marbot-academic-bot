// src/main.rs

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
use sqlx::PgPool;
use chrono::{DateTime, Utc, NaiveDate};

mod models;
mod classifier;
mod parser;
mod whitelist;
mod database;

use crate::database::crud;

use models::{MessageType, AIClassification, WebhookPayload, SendTextRequest, NewAssignment};
use classifier::classify_message;
use parser::commands::handle_command;
use parser::ai_extractor::{extract_with_ai, match_update_to_assignment}; 
use whitelist::Whitelist;

type MessageCache = Arc<Mutex<HashSet<String>>>;

#[derive(Clone)]
struct AppState {
    cache: MessageCache,
    whitelist: Arc<Whitelist>,
    pool: PgPool,
}

async fn webhook(
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) {
    // For now we only get from "message.any"
    if payload.event != "message.any" {
        return;
    }

    // Deduplication Logic
    let dedup_key = format!(
        "{}:{}:{}",
        payload.payload.id,
        payload.payload.from,
        payload.payload.body.chars().take(50).collect::<String>()
    );

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

    // Ignore bot's own messages
    if payload.payload.from_me {
        println!("⏭️  Ignoring bot's own message");
        return;
    }

    // Debug Prints
    let separator = "=".repeat(60);
    println!("\n{}", separator);
    println!("📨 NEW MESSAGE");
    println!("{}", separator);
    println!("From: {}", payload.payload.from);
    println!("Body: {}", payload.payload.body);
    println!("{}\n", separator);

    // STEP 1: CLASSIFY MESSAGE
    let message_type = classify_message(&payload.payload.body);
    let is_command = matches!(message_type, MessageType::Command(_));

    println!("🔍 Classification: {:?}", message_type);

    // STEP 2: CHECK WHITELIST
    let (should_process, reason) =
        state.whitelist.should_process(&payload.payload.from, is_command);

    if !should_process {
        println!("🚫 Ignoring message: {} (from: {})", reason, payload.payload.from);
        return;
    }

    // STEP 3: EXECUTE LOGIC (Command or AI)
    let response_text = match message_type {
    
        MessageType::Command(cmd) => {
            // .await
            Some(handle_command(cmd, &payload.payload.from, &state.pool).await)
        }

        // STEP 4: DATA EXTRACT WITH AI
        MessageType::NeedsAI(text) => {
            // Fetch available courses from database
            let courses_result = crud::get_all_courses_formatted(&state.pool).await;
            
            match courses_result {
                Ok(courses_list) => {
                    println!("📚 Available courses: {}", courses_list);
                    
                    // Pass courses to AI extraction
                    match extract_with_ai(&text, &courses_list).await {
                        Ok(classification) => {
                            handle_ai_classification(
                                state.pool.clone(),
                                classification,
                                &payload.payload.id,
                                &payload.payload.from,
                            )
                        }
                        Err(e) => {
                            eprintln!("❌ AI extraction failed: {}", e);
                            Some("❌ Failed to process message".to_string())
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to fetch courses: {}", e);
                    Some("❌ Failed to fetch course list".to_string())
                }
            }
        }
    };

    // STEP 5: SEND REPLY
    if let Some(text) = response_text {
        if let Err(e) = send_reply(&payload.payload.from, &text).await {
            eprintln!("❌ Failed to send reply: {}", e);
        }
    }
}

/// Handle business logic after AI classifies the message
fn handle_ai_classification(
    pool: PgPool,
    classification: AIClassification, 
    message_id: &str,
    sender_id: &str,
) -> Option<String> {
    println!("\n🤖 AI Classification: {:?}", classification);
    
    let message_id = message_id.to_string();
    let sender_id = sender_id.to_string();
    
    match classification {
        AIClassification::AssignmentInfo { course_name, title, deadline, description, .. } => {
            println!("📚 NEW ASSIGNMENT DETECTED");
            
            let pool_clone = pool.clone();
            let course_name_for_lookup = course_name.clone();
            let title_clone = title.clone();
            let description_clone = description.clone();
            let deadline_parsed = parse_deadline(&deadline);
            let parallel_code = extract_parallel_code(&title);
            
            // Spawn async database work in background
            tokio::spawn(async move {
                // Look up course_id by name
                let course_id = if let Some(name) = &course_name_for_lookup {
                    match crud::create_assignment(&pool_clone, 
                        
                        name, // course name query
                        parallel_code.as_deref(),
                        &title_clone,
                        &description_clone,
                        Some(&sender_id),
                        &message_id,
                        deadline_parsed
                    ).await {
                        Ok(msg) => println!("✅ DB: {}", msg),
                        Err(e) => eprintln!("❌ DB Error: {}", e),
                    }
                } else {
                    println!("⚠️ No course name detected");
                };
            });
            
            Some(format!(
                "✅ *Assignment Detected*\n\n📚 Course: {}\n📝 {}\n📅 Due: {}\n\n_Sedang disimpan ke database..._",
                course_name.unwrap_or("Unknown".to_string()),
                title,
                deadline.unwrap_or("No due date".to_string()),
            ))
        }
        
        AIClassification::AssignmentUpdate { reference_keywords, changes, new_deadline, new_description, .. } => {
            println!("🔄 UPDATE DETECTED");
            
            let new_deadline_clone = new_deadline.clone();
            let changes_clone = changes.clone();
            let reference_keywords_clone = reference_keywords.clone();
            let new_description_clone = new_description.clone();
            let pool_clone = pool.clone();
            
            tokio::spawn(async move {
                // Get active assignments inside spawn
                match crud::get_assignments(&pool_clone).await { // get_assignments (active/all)
                    Ok(active_assignments) if !active_assignments.is_empty() => {
                        println!("📋 Found {} active assignments for matching", active_assignments.len());
                        
                        // Step 2: Ask AI to match
                        match match_update_to_assignment(
                            &changes_clone,
                            &reference_keywords_clone,
                            &active_assignments
                        ).await {
                            Ok(Some(assignment_id)) => {
                                println!("✅ AI matched to assignment ID: {}", assignment_id);
                                
                                // Step 3: Update logic
                                let deadline_parsed = parse_deadline(&new_deadline_clone);
                                
                                // Update deskripsi/judul
                                if let Err(e) = crud::update_assignment_details(
                                    &pool_clone, 
                                    assignment_id, 
                                    "Updated Title",
                                    &new_description_clone.unwrap_or("Updated description".to_string())
                                ).await {
                                     eprintln!("❌ Failed update details: {}", e);
                                }
                                println!("✅ Assignment updated via AI Match");
                            }
                            Ok(None) => println!("⚠️ AI couldn't match assignment"),
                            Err(e) => eprintln!("❌ AI matching error: {}", e),
                        }
                    }
                    Ok(_) => println!("⚠️ No assignments found to update"),
                    Err(e) => eprintln!("❌ DB Error: {}", e),
                }
            });
            
            Some(format!(
                "🔄 *Update Detected*\n\n✏️ {}\n📅 {}",
                changes,
                new_deadline_clone.unwrap_or("Unchanged".to_string())
            ))
        }
        
        AIClassification::Unrecognized => None,
    }
}

async fn send_reply(chat_id: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let waha_url = "http://localhost:3001/api/sendText";
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
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
        Err(format!("WAHA API error: {}", response.status()).into())
    }
}

fn parse_deadline(deadline_str: &Option<String>) -> Option<DateTime<Utc>> {
    deadline_str.as_ref().and_then(|s| {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .ok()
            .and_then(|date| date.and_hms_opt(23, 59, 59))
            .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
    })
}

fn extract_parallel_code(title: &str) -> Option<String> {
    let upper = title.to_uppercase();
    for code in ["K1", "K2", "K3", "P1", "P2", "P3"] {
        if upper.contains(code) {
            return Some(code.to_lowercase());
        }
    }
    None
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    
    println!("🚀 Starting WhatsApp Academic Bot");
    
    if std::env::var("GEMINI_API_KEY").is_err() {
        eprintln!("⚠️  WARNING: GEMINI_API_KEY not set!");
    }
    
    let pool = database::pool::create_pool().await
        .expect("❌ Failed to connect to database");
    
    let whitelist = Arc::new(Whitelist::new());
    let cache = Arc::new(Mutex::new(HashSet::new()));
    
    let state = AppState { 
        cache, 
        whitelist, 
        pool
    };
    
    let app = Router::new()
        .route("/webhook", post(webhook))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("👂 Listening on {}", addr);
    
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}