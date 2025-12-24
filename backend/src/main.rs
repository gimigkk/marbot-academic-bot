use axum::{
    extract::State,
    routing::post,
    Json,
    Router,
};
use axum::http::StatusCode;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;  
use tokio::sync::Mutex;  
use tokio::net::TcpListener;
use sqlx::PgPool;
use chrono::{DateTime, Utc, NaiveDate};

pub mod models;
pub mod scheduler;
pub mod classifier;
pub mod parser;
pub mod whitelist;
pub mod database;

use crate::database::crud;
use crate::parser::commands::CommandResponse;

use models::{MessageType, AIClassification, WebhookPayload, SendTextRequest, NewAssignment};
use classifier::classify_message;
use parser::commands::handle_command;
use parser::ai_extractor::{extract_with_ai}; 
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
) -> StatusCode {
    // Only process "message.any" events
    if payload.event != "message.any" {
        return StatusCode::OK;
    }

    // Deduplication
    let dedup_key = format!(
        "{}:{}:{}",
        payload.payload.id,
        payload.payload.from,
        payload.payload.body.chars().take(50).collect::<String>()
    );

    {
        let mut cache = state.cache.lock().await;
        if cache.contains(&dedup_key) {
            return StatusCode::OK;
        }

        cache.insert(dedup_key);

        if cache.len() > 100 {
            cache.clear();
        }
    }

    // Ignore messages from the bot itself
    if payload.payload.from_me {
        return StatusCode::OK;
    }

    // Ignore messages from debug group to prevent infinite loop
    let debug_group_id = std::env::var("DEBUG_GROUP_ID").ok();
    if let Some(debug_id) = &debug_group_id {
        if payload.payload.from == *debug_id {
            return StatusCode::OK;
        }
    }

    // Terminal logging for server monitoring
    println!("📨 Message from: {}", payload.payload.from);
    println!("   Body: {}", payload.payload.body);

    // STEP 1: CLASSIFY MESSAGE
    let message_type = classify_message(&payload.payload.body);
    let is_command = matches!(message_type, MessageType::Command(_));

    // STEP 2: CHECK WHITELIST
    let (should_process, reason) =
        state.whitelist.should_process(&payload.payload.from, is_command);

    if !should_process {
        println!("🚫 Ignoring: {} (from: {})", reason, payload.payload.from);
        return StatusCode::OK;
    }

    let chat_id = &payload.payload.from;

    // STEP 3: HANDLE MESSAGE BASED ON TYPE
    match message_type {
        MessageType::Command(cmd) => {
            println!("⚙️  Processing command: {:?}", cmd);
            
            let response = handle_command(cmd, chat_id, chat_id, &state.pool).await;
            
            match response {
                CommandResponse::Text(text) => {
                    if let Err(e) = send_reply(chat_id, &text).await {
                        eprintln!("❌ Failed to send reply: {}", e);
                    }
                }
                CommandResponse::ForwardMessage { message_id, warning } => {
                    if let Err(e) = forward_message(chat_id, &message_id).await {
                        eprintln!("❌ Failed to forward message: {}", e);
                    } else {
                        // Send warning after forwarding
                        if let Err(e) = send_reply(chat_id, &warning).await {
                            eprintln!("❌ Failed to send warning: {}", e);
                        }
                    }
                }
            }
        }

        // STEP 4: AI EXTRACTION
        MessageType::NeedsAI(text) => {
            println!("🤖 Processing with AI...");
            
            // Fetch available courses
            let courses_result = crud::get_all_courses_formatted(&state.pool).await;
            
            match courses_result {
                Ok(courses_list) => {
                    // Extract with AI
                    match extract_with_ai(&text, &courses_list).await {
                        Ok(classification) => {
                            println!("✅ AI Classification: {:?}", classification);
                            
                            // Handle classification and send to debug group
                            handle_ai_classification(
                                state.pool.clone(),
                                classification,
                                &payload.payload.id,
                                &payload.payload.from,
                                debug_group_id.clone(),
                            ).await;
                        }
                        Err(e) => {
                            eprintln!("❌ AI extraction failed: {}", e);
                            
                            let error_msg = "❌ Failed to process message".to_string();
                            if let Err(e) = send_reply(chat_id, &error_msg).await {
                                eprintln!("❌ Failed to send error reply: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to fetch courses: {}", e);
                    
                    let error_msg = "❌ Failed to fetch course list".to_string();
                    if let Err(e) = send_reply(chat_id, &error_msg).await {
                        eprintln!("❌ Failed to send error reply: {}", e);
                    }
                }
            }
        }
    }
    
    StatusCode::OK
}

async fn forward_message(chat_id: &str, message_id: &str) -> Result<(), String> {
    let waha_url = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://localhost:3001".to_string());
    let api_key = std::env::var("WAHA_API_KEY").map_err(|e| e.to_string())?;
    
    let forward_payload = serde_json::json!({
        "session": "default",
        "chatId": chat_id,
        "messageId": message_id
    });
    
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/forwardMessage", waha_url))
        .header("X-Api-Key", api_key)
        .header("Content-Type", "application/json")
        .json(&forward_payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to forward message: {}", error_text));
    }
    
    Ok(())
}

async fn handle_ai_classification(
    pool: PgPool,
    classification: AIClassification, 
    message_id: &str,
    sender_id: &str,
    debug_group_id: Option<String>,
) {
    let message_id = message_id.to_string();
    let sender_id = sender_id.to_string();
    
    match classification {
        AIClassification::AssignmentInfo { course_name, title, deadline, description, .. } => {
            println!("📚 NEW ASSIGNMENT DETECTED");
            
            let pool_clone = pool.clone();
            let course_name_for_lookup = course_name.clone();
            let title_clone = title.clone();
            let description_clone = description.clone().unwrap_or_else(|| "No description".to_string());
            let deadline_parsed = parse_deadline(&deadline);
            let parallel_code = extract_parallel_code(&title);
            let deadline_for_response = deadline.clone();
            let course_name_for_response = course_name.clone();
            
            tokio::spawn(async move {
                // Look up course_id by name
                let course_id = if let Some(name) = &course_name_for_lookup {
                    match crud::get_course_by_name(&pool_clone, name).await {
                        Ok(Some(course)) => Some(course.id),
                        Ok(None) => None,
                        Err(e) => {
                            eprintln!("❌ Error looking up course: {}", e);
                            None
                        }
                    }
                } else {
                    None
                };
                
                // Check for duplicates
                if let Some(cid) = course_id {
                    match crud::get_assignment_by_title_and_course(&pool_clone, &title_clone, cid).await {
                        Ok(Some(existing)) => {
                            println!("⚠️ Duplicate found, updating...");
                            
                            match crud::update_assignment_fields(
                                &pool_clone,
                                existing.id,
                                deadline_parsed,
                                None,
                                Some(description_clone.clone()),
                            ).await {
                                Ok(updated) => {
                                    let response = format!(
                                        "🔄 *Assignment Updated!*\n\n\
                                        📝 {}\n\
                                        📅 Due: {}\n\
                                        📄 {}",
                                        updated.title,
                                        deadline_for_response.unwrap_or("No due date".to_string()),
                                        description_clone
                                    );
                                    
                                    // Send to debug group instead
                                    if let Some(debug_id) = &debug_group_id {
                                        if let Err(e) = send_reply(debug_id, &response).await {
                                            eprintln!("❌ Failed to send to debug group: {}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("❌ Database update failed: {}", e);
                                }
                            }
                            return;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("❌ Error checking for duplicates: {}", e);
                        }
                    }
                }
                
                // Create new assignment
                let new_assignment = NewAssignment {
                    course_id,
                    title: title_clone.clone(),
                    description: description_clone.clone(),
                    deadline: deadline_parsed,
                    parallel_code,
                    sender_id: Some(sender_id.clone()),
                    message_id: message_id.clone(),
                };
                
                match crud::create_assignment(&pool_clone, new_assignment).await {
                    Ok(message) => {
                        println!("✅ {}", message);
                        
                        let response = format!(
                            "✅ *Assignment Saved!*\n\n\
                            📚 Course: {}\n\
                            📝 {}\n\
                            📅 Due: {}\n\
                            📄 {}",
                            course_name_for_response.unwrap_or("Unknown".to_string()),
                            title_clone,
                            deadline_for_response.unwrap_or("No due date".to_string()),
                            description_clone
                        );
                        
                        // Send to debug group instead
                        if let Some(debug_id) = &debug_group_id {
                            if let Err(e) = send_reply(debug_id, &response).await {
                                eprintln!("❌ Failed to send to debug group: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to save to database: {}", e);
                    }
                }
            });
        }
        
        AIClassification::AssignmentUpdate { reference_keywords, changes, new_deadline, new_title, new_description, .. } => {
            println!("🔄 UPDATE DETECTED");
            
            let new_deadline_clone = new_deadline.clone();
            let new_title_clone = new_title.clone();
            let changes_clone = changes.clone();
            let reference_keywords_clone = reference_keywords.clone();
            let new_description_clone = new_description.clone();
            let pool_clone = pool.clone();
            
            tokio::spawn(async move {
                // Try to identify course from keywords
                let mut course_id: Option<uuid::Uuid> = None;
                let mut course_name: Option<String> = None;
                
                for keyword in &reference_keywords_clone {
                    match crud::get_course_by_name_or_alias(&pool_clone, keyword).await {
                        Ok(Some(course)) => {
                            course_id = Some(course.id);
                            course_name = Some(course.name.clone());
                            break;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("❌ Error looking up course: {}", e);
                        }
                    }
                }
                
                // Get recent assignments and try to match
                match crud::get_recent_assignments_for_update(&pool_clone, course_id).await {
                    Ok(assignments) if !assignments.is_empty() => {
                        match parser::ai_extractor::match_update_to_assignment(
                            &changes_clone,
                            &reference_keywords_clone,
                            &assignments
                        ).await {
                            Ok(Some(assignment_id)) => {
                                let parsed_deadline = if let Some(ref deadline_str) = new_deadline_clone {
                                    crud::parse_deadline(deadline_str).ok()
                                } else {
                                    None
                                };
                                
                                match crud::update_assignment_fields(
                                    &pool_clone,
                                    assignment_id,
                                    parsed_deadline,
                                    new_title_clone.clone(),
                                    new_description_clone.clone(),
                                ).await {
                                    Ok(updated) => {
                                        let response = format!(
                                            "🔄 *Assignment Updated!*\n\n\
                                            📝 {}\n\
                                            ✏️ {}\n\
                                            📅 {}",
                                            updated.title,
                                            changes_clone,
                                            new_deadline_clone.unwrap_or("Unchanged".to_string())
                                        );
                                        
                                        // Send to debug group
                                        if let Some(debug_id) = &debug_group_id {
                                            if let Err(e) = send_reply(debug_id, &response).await {
                                                eprintln!("❌ Failed to send to debug group: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("❌ Database update failed: {}", e);
                                    }
                                }
                            }
                            Ok(None) => {
                                println!("⚠️ AI couldn't match to any assignment");
                            }
                            Err(e) => {
                                eprintln!("❌ AI matching failed: {}", e);
                            }
                        }
                    }
                    Ok(_) => {
                        println!("⚠️ No assignments found - trying fallback creation");
                        
                        // FALLBACK: Create new assignment
                        if let (Some(cid), Some(ref deadline_str)) = (course_id, &new_deadline_clone) {
                            let title = reference_keywords_clone
                                .iter()
                                .find(|k| {
                                    let lower = k.to_lowercase();
                                    lower.contains("lkp") || 
                                    lower.contains("tugas") || 
                                    lower.contains("quiz") ||
                                    lower.contains("uts") ||
                                    lower.contains("uas") ||
                                    lower.starts_with("bab ")
                                })
                                .cloned()
                                .unwrap_or_else(|| "Assignment".to_string());
                            
                            let parallel_code = reference_keywords_clone
                                .iter()
                                .find(|k| k.to_uppercase().starts_with('K') && k.len() == 2)
                                .map(|k| k.to_lowercase());
                            
                            if let Ok(parsed_deadline) = crud::parse_deadline(deadline_str) {
                                let description = new_description_clone
                                    .unwrap_or_else(|| changes_clone.clone());
                                
                                let new_assignment = NewAssignment {
                                    course_id: Some(cid),
                                    title: title.clone(),
                                    description: description.clone(),
                                    deadline: Some(parsed_deadline),
                                    parallel_code: parallel_code.clone(),
                                    sender_id: None,
                                    message_id: String::new(),
                                };
                                
                                match crud::create_assignment(&pool_clone, new_assignment).await {
                                    Ok(_) => {
                                        let response = format!(
                                            "✅ *New Assignment Saved!*\n\n\
                                            📚 {}\n\
                                            📝 {}\n\
                                            📅 {}\n\
                                            📄 {}",
                                            course_name.unwrap_or("Unknown".to_string()),
                                            title,
                                            deadline_str,
                                            description
                                        );
                                        
                                        // Send to debug group
                                        if let Some(debug_id) = &debug_group_id {
                                            if let Err(e) = send_reply(debug_id, &response).await {
                                                eprintln!("❌ Failed to send to debug group: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("❌ Failed to create assignment: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to fetch assignments: {}", e);
                    }
                }
            });
        }
        
        AIClassification::Unrecognized => {}
    }
}

async fn send_reply(chat_id: &str, text: &str) -> Result<(), String> {
    let waha_url = "http://localhost:3001/api/sendText";
    let api_key = std::env::var("WAHA_API_KEY")
        .unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = SendTextRequest {
        chat_id: chat_id.to_string(),
        text: text.to_string(),
        session: "default".to_string(),
    };

    let client = reqwest::Client::new();
    let response = client
        .post(waha_url)
        .header("X-Api-Key", api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    
    if status.is_success() {
        Ok(())
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(format!("WAHA API error: {}", status))
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

    
    let pool_for_scheduler = pool.clone(); 
    tokio::spawn(async move {
    println!("⏳ Menjalankan Scheduler (Background Job)...");
    if let Err(e) = scheduler::start_scheduler(pool_for_scheduler).await {
        eprintln!("❌ Gagal menjalankan scheduler: {:?}", e);
    }
    });
    
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
    println!("📍 Webhook endpoint: http://localhost:3000/webhook");
    println!("\n{}\n", "=".repeat(60));

    let listener = TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}