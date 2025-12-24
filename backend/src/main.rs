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

pub mod models;
pub mod classifier;
pub mod parser;
pub mod whitelist;
pub mod database;

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

    // this is for handling when WAHA sends duplicate messages
    // don't know why it does that
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

    // for some reason messages that's sent by the bot is being read also.
    if payload.payload.from_me {
        println!("⏭️  Ignoring bot's own message");
        return;
    }

    // for debuggin in the terminal
    let separator = "=".repeat(60);
    println!("\n{}", separator);
    println!("📨 NEW MESSAGE");
    println!("{}", separator);
    println!("From: {}", payload.payload.from);

    if payload.payload.from.ends_with("@newsletter") {
        println!("   📢 Type: WhatsApp Channel/Newsletter");
    } else if payload.payload.from.ends_with("@g.us") {
        println!("   👥 Type: WhatsApp Group");
    } else {
        println!("   💬 Type: Direct Message");
    }

    println!("Message ID: {}", payload.payload.id);
    println!("Body: {}", payload.payload.body);
    println!("From Me: {}", payload.payload.from_me);
    println!("{}\n", separator);

    // STEP 1: CLASSIFY MESSAGE
    let message_type = classify_message(&payload.payload.body);
    let is_command = matches!(message_type, MessageType::Command(_));

    println!("🔍 Classification: {:?}", message_type);

    // STEP 2: CHECK WHITELIST (for academic info)
    let (should_process, reason) =
        state.whitelist.should_process(&payload.payload.from, is_command);

    if !should_process {
        println!("🚫 Ignoring message: {} (from: {})", reason, payload.payload.from);
        return;
    }

    // STEP 3: RUN COMMAND (if it's a bot command)
    let response_text = match message_type {
    MessageType::Command(cmd) => {
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

/// After classifying the message by AI,
/// business logic.
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
            println!("   Course: {}", course_name.as_ref().unwrap_or(&"Unknown".to_string()));
            
            let pool_clone = pool.clone();
            let course_name_for_lookup = course_name.clone();
            let title_clone = title.clone();
            let description_clone = description.clone().unwrap_or_else(|| "No description".to_string());
            let deadline_parsed = parse_deadline(&deadline);
            let parallel_code = extract_parallel_code(&title);
            let deadline_for_response = deadline.clone();
            let course_name_for_response = course_name.clone();
            
            // Spawn async database work in background
            tokio::spawn(async move {
                // Look up course_id by name
                let course_id = if let Some(name) = &course_name_for_lookup {
                    match crud::get_course_by_name(&pool_clone, name).await {
                        Ok(Some(course)) => {
                            println!("✅ Found course: {} (ID: {})", course.name, course.id);
                            Some(course.id)
                        }
                        Ok(None) => {
                            println!("⚠️ Course '{}' not found in database", name);
                            None
                        }
                        Err(e) => {
                            eprintln!("❌ Error looking up course: {}", e);
                            None
                        }
                    }
                } else {
                    None
                };
                
                // **NEW: Check for existing assignment with same title**
                if let Some(cid) = course_id {
                    match crud::get_assignment_by_title_and_course(&pool_clone, &title_clone, cid).await {
                        Ok(Some(existing)) => {
                            println!("⚠️ Assignment '{}' already exists (ID: {})", title_clone, existing.id);
                            println!("🔄 Updating existing assignment instead of creating duplicate...");
                            
                            // Update the existing assignment instead of creating duplicate
                            match crud::update_assignment_fields(
                                &pool_clone,
                                existing.id,
                                deadline_parsed,
                                Some(description_clone.clone()),
                            ).await {
                                Ok(updated) => {
                                    println!("✅ Successfully updated assignment: {}", updated.title);
                                    
                                    let response = format!(
                                        "🔄 *Assignment Updated!*\n\n\
                                        📝 {}\n\
                                        📅 Due: {}\n\
                                        📄 {}",
                                        updated.title,
                                        deadline_for_response.unwrap_or("No due date".to_string()),
                                        description_clone
                                    );
                                    
                                    if let Err(e) = send_reply(&sender_id, &response).await {
                                        eprintln!("❌ Failed to send update confirmation: {}", e);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("❌ Database update failed: {}", e);
                                }
                            }
                            return; // Exit early, don't create new
                        }
                        Ok(None) => {
                            println!("✅ No duplicate found, proceeding with creation");
                        }
                        Err(e) => {
                            eprintln!("❌ Error checking for duplicates: {}", e);
                        }
                    }
                }
                
                // Original creation code continues...
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
                        
                        // Send creation confirmation
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
                        
                        if let Err(e) = send_reply(&sender_id, &response).await {
                            eprintln!("❌ Failed to send creation confirmation: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to save to database: {}", e);
                    }
                }
            });
            
            // Return None since we're sending replies inside the tokio::spawn
            None
        }
        
        AIClassification::AssignmentUpdate { reference_keywords, changes, new_deadline, new_description, .. } => {
            println!("🔄 UPDATE DETECTED");
            println!("   Keywords: {:?}", reference_keywords);
            
            let new_deadline_clone = new_deadline.clone();
            let changes_clone = changes.clone();
            let reference_keywords_clone = reference_keywords.clone();
            let new_description_clone = new_description.clone();
            let pool_clone = pool.clone();
            
            // Clone for response - we'll determine the response type inside the spawn
            let sender_id_for_response = sender_id.to_string();
            
            tokio::spawn(async move {
                // Try to identify course from keywords
                let mut course_id: Option<uuid::Uuid> = None;
                let mut course_name: Option<String> = None;
                
                for keyword in &reference_keywords_clone {
                    println!("🔍 Checking if '{}' is a course name/alias...", keyword);
                    match crud::get_course_by_name_or_alias(&pool_clone, keyword).await {
                        Ok(Some(course)) => {
                            println!("✅ Found course: {} (ID: {})", course.name, course.id);
                            course_id = Some(course.id);
                            course_name = Some(course.name.clone());
                            break;
                        }
                        Ok(None) => {
                            println!("   '{}' is not a course", keyword);
                        }
                        Err(e) => {
                            eprintln!("❌ Error looking up course: {}", e);
                        }
                    }
                }
                
                // Get recent assignments
                match crud::get_recent_assignments_for_update(&pool_clone, course_id).await {
                    Ok(assignments) if !assignments.is_empty() => {
                        println!("📋 Found {} assignments to check for matching", assignments.len());
                        println!("📋 Assignments being sent to AI:");
                        for (i, a) in assignments.iter().enumerate() {
                            println!("   {}. Title: '{}', Description: '{}'", i + 1, a.title, a.description);
                        }
                        
                        // Ask AI to match
                        match parser::ai_extractor::match_update_to_assignment(
                            &changes_clone,
                            &reference_keywords_clone,
                            &assignments
                        ).await {
                            Ok(Some(assignment_id)) => {
                                println!("✅ AI matched to assignment ID: {}", assignment_id);
                                
                                // Parse the new deadline if provided
                                let parsed_deadline = if let Some(ref deadline_str) = new_deadline_clone {
                                    match crud::parse_deadline(deadline_str) {
                                        Ok(dt) => {
                                            println!("✅ Parsed deadline: {}", dt);
                                            Some(dt)
                                        }
                                        Err(e) => {
                                            eprintln!("❌ Failed to parse deadline '{}': {}", deadline_str, e);
                                            None
                                        }
                                    }
                                } else {
                                    None
                                };
                                
                                // Update the matched assignment
                                match crud::update_assignment_fields(
                                    &pool_clone,
                                    assignment_id,
                                    parsed_deadline,
                                    new_description_clone.clone(),
                                ).await {
                                    Ok(updated) => {
                                        println!("✅ Successfully updated assignment: {}", updated.title);
                                        println!("   New deadline: {:?}", updated.deadline);
                                        println!("   Description: {}", updated.description);
                                        
                                        // Send update confirmation
                                        let response = format!(
                                            "🔄 *Assignment Updated!*\n\n\
                                            📝 {}\n\
                                            ✏️ {}\n\
                                            📅 {}",
                                            updated.title,
                                            changes_clone,
                                            new_deadline_clone.unwrap_or("Unchanged".to_string())
                                        );
                                        
                                        if let Err(e) = send_reply(&sender_id_for_response, &response).await {
                                            eprintln!("❌ Failed to send update confirmation: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("❌ Database update failed: {}", e);
                                    }
                                }
                            }
                            Ok(None) => {
                                println!("⚠️ AI couldn't confidently match to any assignment");
                            }
                            Err(e) => {
                                eprintln!("❌ AI matching failed: {}", e);
                            }
                        }
                    }
                    Ok(_) => {
                        println!("⚠️ No assignments found in database");
                        if let Some(cid) = course_id {
                            println!("   (searched in course ID: {})", cid);
                        } else {
                            println!("   (searched across all courses)");
                        }
                        
                        // FALLBACK: Create new assignment
                        if let Some(cid) = course_id {
                            if let Some(ref deadline_str) = new_deadline_clone {
                                println!("💡 Converting update to new assignment creation...");
                                
                                // Extract title
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
                                
                                println!("   Title: {}", title);
                                println!("   Deadline: {}", deadline_str);
                                
                                // Extract parallel code
                                let parallel_code = reference_keywords_clone
                                    .iter()
                                    .find(|k| k.to_uppercase().starts_with('K') && k.len() == 2)
                                    .map(|k| k.to_lowercase());
                                
                                if let Some(ref pc) = parallel_code {
                                    println!("   Parallel: {}", pc);
                                }
                                
                                // Parse deadline
                                match crud::parse_deadline(deadline_str) {
                                    Ok(parsed_deadline) => {
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
                                            Ok(success_msg) => {
                                                println!("✅ {}", success_msg);
                                                
                                                // Send creation confirmation
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
                                                
                                                if let Err(e) = send_reply(&sender_id_for_response, &response).await {
                                                    eprintln!("❌ Failed to send creation confirmation: {}", e);
                                                }
                                            }
                                            Err(e) => {
                                                eprintln!("❌ Failed to create assignment: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("❌ Failed to parse deadline '{}': {}", deadline_str, e);
                                    }
                                }
                            } else {
                                println!("   ⚠️ Cannot create assignment - no deadline provided");
                            }
                        } else {
                            println!("   ⚠️ Cannot create assignment - course not identified");
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to fetch assignments from database: {}", e);
                    }
                }
            });
            
            // Return None since we're sending replies inside the tokio::spawn
            None
        }
        
        AIClassification::Unrecognized => None,
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

    let status = response.status();
    
    if status.is_success() {
        println!("✅ WAHA API responded successfully");
        Ok(())
    } else {
        let body = response.text().await?;
        eprintln!("❌ WAHA API error: {} - {}", status, body);
        Err(format!("WAHA API error: {}", status).into())
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
    let separator = "=".repeat(60);
    println!("👂 Listening on {}", addr);
    println!("📍 Webhook endpoint: http://localhost:3000/webhook");
    println!("\n{}\n", separator);

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}