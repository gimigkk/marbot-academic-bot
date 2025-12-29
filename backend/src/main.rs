use axum::{
    extract::State,
    routing::post,
    Json,
    Router,
};
use axum::http::StatusCode;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;  
use std::io::Write;
use std::time::{Duration, Instant};
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
pub mod clarification;

use crate::database::crud;
use crate::parser::commands::CommandResponse;

use models::{MessageType, AIClassification, WebhookPayload, SendTextRequest, NewAssignment};
use classifier::classify_message;
use parser::commands::handle_command;
use parser::ai_extractor::{extract_with_ai}; 
use whitelist::Whitelist;

type MessageCache = Arc<Mutex<HashSet<String>>>;
type RateLimiter = Arc<Mutex<HashMap<String, VecDeque<Instant>>>>;


const BANNER: &str = r#"
\x1b[36m

███╗   ███╗ █████╗ ██████╗ ██████╗  ██████╗ ████████╗
████╗ ████║██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚══██╔══╝
██╔████╔██║███████║██████╔╝██████╔╝██║   ██║   ██║   
██║╚██╔╝██║██╔══██║██╔══██╗██╔══██╗██║   ██║   ██║   
██║ ╚═╝ ██║██║  ██║██║  ██║██████╔╝╚██████╔╝   ██║   
╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝    ╚═╝   
                                                                                                                                                                                                                              
         [WhatsApp Academic Assistant v1.0]           
              Created by Gilang & Arya     
\x1b[0m"#;

#[derive(Clone)]
struct AppState {
    cache: MessageCache,
    whitelist: Arc<Whitelist>,
    pool: PgPool,
    rate_limiter: RateLimiter,
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    // 1. Tampilan Awal (Clear Screen & Banner)
    print!("\x1b[2J\x1b[1;1H"); 
    println!("{}", BANNER);
    println!("\x1b[1;30m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");

    // 2. Cek Environment Variables
    let gemini_status = if std::env::var("GEMINI_API_KEY").is_ok() {
        "\x1b[32m✅ READY\x1b[0m"
    } else {
        "\x1b[31m❌ MISSING\x1b[0m"
    };

    let waha_status = if std::env::var("WAHA_API_KEY").is_ok() {
        "\x1b[32m✅ READY\x1b[0m"
    } else {
        "\x1b[33m⚠️  DEFAULT\x1b[0m"
    };

    println!(" 🔧 \x1b[1mSYSTEM CHECK\x1b[0m");
    println!("    ├─ 🧠 Gemini AI    : {}", gemini_status);
    println!("    ├─ 🔌 WAHA API     : {}", waha_status);

    // 3. Koneksi Database
    print!("    ├─ 🗄️  Database     : 🔌 Connecting...");
    std::io::stdout().flush().unwrap();

    let pool = match database::pool::create_pool().await {
        Ok(p) => {
            // Use \x1b[K to clear from cursor to end of line
            print!("\r    ├─ 🗄️  Database     : \x1b[32m✅ CONNECTED\x1b[0m\x1b[K\n");
            std::io::stdout().flush().unwrap();
            p
        }
        Err(e) => {
            print!("\r    ├─ 🗄️  Database     : \x1b[31m❌ FAILED\x1b[0m\x1b[K\n");
            std::io::stdout().flush().unwrap();
            eprintln!("       └─ Error: {}", e);
            return;
        }
    };

    let whitelist = Arc::new(Whitelist::new());
    let cache = Arc::new(Mutex::new(HashSet::new()));
    let rate_limiter = Arc::new(Mutex::new(HashMap::new()));

    // 4. Jalankan Scheduler
    let pool_for_scheduler = pool.clone();
    tokio::spawn(async move {
       
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        if let Err(e) = scheduler::start_scheduler(pool_for_scheduler).await {
            eprintln!("\n\x1b[31m❌ Scheduler Error: {:?}\x1b[0m", e);
        }
    });
    println!("    └─ ⏰ Scheduler    : \x1b[32m✅ RUNNING\x1b[0m");

    let state = AppState { 
        cache, 
        whitelist, 
        pool,
        rate_limiter,
    };
    
    let app = Router::new()
        .route("/webhook", post(webhook))
        .with_state(state);

    let port = 3000;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("\x1b[1;30m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!(" 🚀 \x1b[1;32mMARBOT IS ONLINE!\x1b[0m");
    println!("    📡 Listening on   : \x1b[36mhttp://0.0.0.0:{}\x1b[0m", port);
    println!("    📍 Webhook URL    : \x1b[36mhttp://localhost:{}/webhook\x1b[0m", port);
    println!("\x1b[1;30m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!("\nWaiting for incoming messages...\n");

    let listener = TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

#[allow(non_snake_case)]
async fn webhook(
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) -> StatusCode {

    // Check message payload
    //println!("\n🔍 DEBUG Raw payload: {:#?}\n", payload);

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
    // if let Some(debug_id) = &debug_group_id {
    //     if payload.payload.from == *debug_id {
    //         return StatusCode::OK;
    //     }
    // }

    // ✅ EXTRACT SENDER AND CHAT IDs CORRECTLY
    let chat_id = &payload.payload.from;  // Where to send reply (group or personal chat)
    
    // Extract sender's actual phone number
    // For group messages: 'participant' field contains the actual sender
    // For personal chats: 'from' field IS the sender
    let sender_phone = if chat_id.ends_with("@g.us") {
        // Group message - use participant field for sender ID
        payload.payload.participant
            .as_ref()
            .unwrap_or(chat_id)
    } else {
        // Personal chat - 'from' field is the sender
        chat_id
    };
    
    // ✅ Extract WhatsApp display name (pushName) from _data field
    let sender_name = payload.payload.data
        .as_ref()
        .and_then(|data| data.push_name.as_ref())
        .map(|name| name.as_str())
        .unwrap_or_else(|| {
            // Fallback: extract phone number from sender_phone (remove @c.us or @g.us)
            sender_phone.split('@').next().unwrap_or(sender_phone)
        });

    // Terminal logging for server monitoring
    println!("📨 Message from: {}", chat_id);
    println!("   Sender: {} ({})", sender_name, sender_phone);
    println!("   Body: {}", payload.payload.body);

    // Simple anti-spam guard
    if is_spam(&state.rate_limiter, sender_phone).await {
        println!("🚫 Spam detected from {}", sender_phone);
        let warn_msg = "🚫 Mohon hindari spam. Tunggu beberapa saat sebelum mengirim pesan lagi.";
        if let Err(e) = send_reply(chat_id, warn_msg).await {
            eprintln!("❌ Failed to send spam warning: {}", e);
        }
        return StatusCode::OK;
    }

    // println!("   🔍 Has quoted message: {}", payload.payload.quoted_msg.is_some());
    // if let Some(ref q) = payload.payload.quoted_msg {
    //     println!("   📎 Quoted text preview: {}", q.text.chars().take(50).collect::<String>());
    // }

    // ============= NEW: CLARIFICATION HANDLER =============
    // Check if this message is replying to a clarification request
    if let Some(quoted) = payload.payload.get_quoted_message() {
        if quoted.text.contains("⚠️ *PERLU KLARIFIKASI*") {
            println!("📝 Clarification response detected from {}", sender_phone);
            
            // Extract assignment ID from the quoted clarification message
            if let Some(assignment_id) = clarification::extract_assignment_id_from_message(&quoted.text) {
                println!("🔍 Updating assignment: {}", assignment_id);
                
                // Parse user's response (e.g., "Course: Pemrograman\nDeadline: 2025-12-30")
                let updates = clarification::parse_clarification_response(&payload.payload.body);

                if updates.is_empty() {
                    let error_msg = "❌ Format tidak valid. Gunakan format:\n\
                                    `Course: [nama]`\n\
                                    `Title: [judul]`\n\
                                    `Deadline: [YYYY-MM-DD]`\n\
                                    `Parallel: [K1/K2/K3]`\n\
                                    `Description: [keterangan]`\n\n\
                                    _Cukup isi field yang kurang saja!_";
                    
                    if let Err(e) = send_reply(chat_id, error_msg).await {
                        eprintln!("❌ Failed to send error: {}", e);
                    }
                    return StatusCode::OK;
                }

                // Parse deadline if provided
                let new_deadline = updates.get("deadline")
                    .and_then(|d| crud::parse_deadline(d).ok());

                let new_title = updates.get("title").cloned();
                let new_description = updates.get("description").cloned();
                let new_parallel = updates.get("parallel_code")
                    .map(|p| p.to_lowercase());

                // Special handling for course name update
                let course_id = if let Some(course_name) = updates.get("course_name") {
                    match crud::get_course_by_name(&state.pool, course_name).await {
                        Ok(Some(course)) => Some(course.id),
                        Ok(None) => {
                            let error_msg = format!("❌ Mata kuliah '{}' tidak ditemukan.\n\n_Cek ejaan atau tambahkan dulu ke database._", course_name);
                            if let Err(e) = send_reply(chat_id, &error_msg).await {
                                eprintln!("❌ Failed to send error: {}", e);
                            }
                            return StatusCode::OK;
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to lookup course: {}", e);
                            None
                        }
                    }
                } else {
                    None
                };

                // Update the assignment in database
                match crud::update_assignment_fields(
                    &state.pool,
                    assignment_id,
                    new_deadline,
                    new_title.clone(),
                    new_description.clone(),
                    new_parallel.clone(),
                    None, // Don't change message_id for clarifications
                ).await {
                    Ok(updated) => {
                        // If course_id was updated, do that separately
                        if let Some(cid) = course_id {
                            if let Err(e) = sqlx::query(
                                "UPDATE assignments SET course_id = $1 WHERE id = $2"
                            )
                            .bind(cid)
                            .bind(assignment_id)
                            .execute(&state.pool)
                            .await {
                                eprintln!("❌ Failed to update course_id: {}", e);
                            }
                        }
                        
                        // Get course name for display
                        let display_course_name = if let Some(course_name) = updates.get("course_name") {
                            course_name.to_string()
                        } else if let Some(cid) = updated.course_id {
                            sqlx::query_scalar::<_, String>(
                                "SELECT name FROM courses WHERE id = $1"
                            )
                            .bind(cid)
                            .fetch_one(&state.pool)
                            .await
                            .unwrap_or_else(|_| "Unknown Course".to_string())
                        } else {
                            "Unknown Course".to_string()
                        };
                        
                        let response = format!(
                            "✅ *KLARIFIKASI TERSIMPAN*\n\
                            ━━━━━━━━━━━━━━━━━━\n\
                            📝 *{}*\n\
                            📚 {}\n\
                            ⏰ Deadline: {}\n\
                            🧩 Paralel: {}\n\
                            📄 {}\n\n\
                            _Terima kasih atas klarifikasinya!_",
                            updated.title,
                            display_course_name,
                            updated.deadline
                                .map(|d| d.format("%Y-%m-%d").to_string())
                                .unwrap_or("Belum ada deadline".to_string()),
                            updated.parallel_code.as_deref().unwrap_or("N/A"),
                            updated.description
                        );
                        
                        if let Err(e) = send_reply(chat_id, &response).await {
                            eprintln!("❌ Failed to send confirmation: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to update assignment: {}", e);
                        
                        let error_msg = format!("❌ Gagal menyimpan klarifikasi: {}", e);
                        if let Err(e) = send_reply(chat_id, &error_msg).await {
                            eprintln!("❌ Failed to send error: {}", e);
                        }
                    }
                }

                return StatusCode::OK;
            } else {
                println!("⚠️  Could not extract assignment ID from quoted message");
            }
        }
    }
    // ============= END CLARIFICATION HANDLER =============

    

    // STEP 1: CLASSIFY MESSAGE
    let message_type = classify_message(&payload.payload.body);
    let is_command = matches!(message_type, MessageType::Command(_));

    // STEP 2: CHECK WHITELIST
    let (should_process, reason) =
        state.whitelist.should_process(chat_id, is_command);

    if !should_process {
        println!("🚫 Ignoring: {} (from: {})\n", reason, chat_id);
        return StatusCode::OK;
    }

    // STEP 3: HANDLE MESSAGE BASED ON TYPE
    match message_type {
        MessageType::Command(cmd) => {
            println!("⚙️  Processing command: {:?}", cmd);
            
            // ✅ FIXED: Pass sender_phone (not chat_id) as user identifier
            // ✅ Pass sender_name for display in responses
            let response = handle_command(cmd, sender_phone, sender_name, chat_id, &state.pool).await;
            
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
            
            // Check if message has media (image)
            let image_base64 = if payload.payload.has_media.unwrap_or(false) {
                if let Some(ref media) = payload.payload.media {
                    if let Some(ref media_url) = media.url {
                        // Check if it's an image
                        let is_image = media.mimetype
                            .as_ref()
                            .map(|m| m.starts_with("image/"))
                            .unwrap_or(false);
                        
                        if is_image {
                            let api_key = std::env::var("WAHA_API_KEY")
                                .unwrap_or_else(|_| "devkey123".to_string());
                            
                            match fetch_image_from_url(media_url, &api_key).await {
                                Ok(base64) => Some(base64),
                                Err(e) => {
                                    eprintln!("❌ Failed to download image: {}", e);
                                    None
                                }
                            }
                        } else {
                            println!("⚠️  Media is not an image: {:?}", media.mimetype);
                            None
                        }
                    } else {
                        eprintln!("⚠️  hasMedia=true but no URL (check WHATSAPP_DOWNLOAD_MEDIA config)");
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            
            // Fetch available courses (formatted for AI)
            let courses_result = crud::get_all_courses_formatted(&state.pool).await;
            
            match courses_result {
                Ok(courses_list) => {
                    // Fetch active assignments for context
                    let active_assignments_result = crud::get_active_assignments(&state.pool).await;
                    
                    match active_assignments_result {
                        Ok(active_assignments) => {
                            // Build course map (simple query: id -> name)
                            let course_map_result = sqlx::query_as::<_, (uuid::Uuid, String)>(
                                "SELECT id, name FROM courses"
                            )
                            .fetch_all(&state.pool)
                            .await;
                            
                            match course_map_result {
                                Ok(courses) => {
                                    let course_map: std::collections::HashMap<uuid::Uuid, String> = 
                                        courses.into_iter().collect();
                                    
                                    // Extract with AI (now with context)
                                    match extract_with_ai(
                                        &text, 
                                        &courses_list, 
                                        &active_assignments,
                                        &course_map,
                                        image_base64.as_deref()
                                    ).await {
                                        Ok(classification) => {
                                            println!("✅ AI Classification: {:?}\n", classification);
                                            
                                            // Handle classification and send to debug group
                                            handle_ai_classification(
                                                state.pool.clone(),
                                                classification,
                                                &payload.payload.id,
                                                sender_phone,  // ✅ FIXED: Use sender_phone instead of chat_id
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
                                    eprintln!("❌ Failed to fetch course map: {}", e);
                                    
                                    let error_msg = "❌ Failed to fetch course data".to_string();
                                    if let Err(e) = send_reply(chat_id, &error_msg).await {
                                        eprintln!("❌ Failed to send error reply: {}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to fetch active assignments: {}", e);
                            
                            let error_msg = "❌ Failed to fetch assignment context".to_string();
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

async fn is_spam(rate_limiter: &RateLimiter, sender_id: &str) -> bool {
    const WINDOW_SECS: u64 = 10;
    const MAX_MESSAGES: usize = 5;
    const CLEANUP_LIMIT_MULTIPLIER: usize = 3;
    const CLEANUP_TARGET_MULTIPLIER: usize = 2;

    let now = Instant::now();

    let is_spam = {
        let mut map = rate_limiter.lock().await;
        let entry = map.entry(sender_id.to_string()).or_insert_with(VecDeque::new);

        while let Some(&ts) = entry.front() {
            if now.duration_since(ts) > Duration::from_secs(WINDOW_SECS) {
                entry.pop_front();
            } else {
                break;
            }
        }

        let at_limit = entry.len() >= MAX_MESSAGES;
        entry.push_back(now);

        let cleanup_limit = MAX_MESSAGES * CLEANUP_LIMIT_MULTIPLIER;
        let cleanup_target = MAX_MESSAGES * CLEANUP_TARGET_MULTIPLIER;

        if entry.len() > cleanup_limit {
            while entry.len() > cleanup_target {
                entry.pop_front();
            }
        }

        at_limit
    };

    is_spam
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

#[allow(non_snake_case)]
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
            let debug_group_clone = debug_group_id.clone();
            
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
                                None,
                                Some(message_id.clone()),
                            ).await {
                                Ok(updated) => {
                                    let response = format!(
                                        "🔄 *INFO TUGAS DIPERBARUI*\n\
                                        ━━━━━━━━━━━━━━━━━━\n\
                                        📝 *{}*\n\
                                        ⚠️ _Terdeteksi duplikat, data diupdate_\n\
                                        📅 Due: {}\n\
                                        ━━━━━━━━━━━━━━━━━━",
                                        updated.title,
                                        deadline_for_response.unwrap_or("No due date".to_string())
                                    );
                                    
                                    // Send to debug group instead
                                    if let Some(debug_id) = &debug_group_clone {
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
                        
                        // NEW: Check if clarification is needed
                        if let Some(cid) = course_id {
                            // Get the assignment we just created
                            match crud::get_assignment_by_title_and_course(&pool_clone, &title_clone, cid).await {
                                Ok(Some(assignment)) => {
                                    // Fetch with course info for clarification check
                                    match crud::get_assignment_with_course_by_id(&pool_clone, assignment.id).await {
                                        Ok(Some(created_assignment)) => {
                                            let missing_fields = clarification::identify_missing_fields(&created_assignment);
                                            
                                            if !missing_fields.is_empty() {
                                                println!("⚠️  Missing fields detected: {:?}", missing_fields);
                                                
                                                // Send clarification request to debug group
                                                if let Some(debug_id) = &debug_group_clone {
                                                    let clarification_msg = clarification::generate_clarification_message(
                                                        &created_assignment,
                                                        &missing_fields
                                                    );
                                                    
                                                    if let Err(e) = send_reply(debug_id, &clarification_msg).await {
                                                        eprintln!("❌ Failed to send clarification: {}", e);
                                                    }
                                                }
                                                
                                                // Don't send success message - waiting for clarification
                                                return;
                                            }
                                        }
                                        Ok(None) => {
                                            eprintln!("⚠️  Assignment not found after creation");
                                        }
                                        Err(e) => {
                                            eprintln!("❌ Error fetching created assignment: {}", e);
                                        }
                                    }
                                }
                                Ok(None) => {
                                    eprintln!("⚠️  Could not fetch created assignment");
                                }
                                Err(e) => {
                                    eprintln!("❌ Error fetching assignment: {}", e);
                                }
                            }
                        }
                        
                        // Normal success message (only if no clarification needed)
                        let response = format!(
                            "✨ *TUGAS BARU TERSIMPAN* ✨\n\
                            ━━━━━━━━━━━━━━━━━━\n\
                            📚 *{}*\n\
                            📝 {}\n\
                            📅 Deadline: {}\n\
                            ━━━━━━━━━━━━━━━━━━\n\
                            📄 _{}_",
                            course_name_for_response.unwrap_or("Mata Kuliah Umum".to_string()),
                            title_clone,
                            deadline_for_response.unwrap_or("? (Cek lagi)".to_string()),
                            description_clone
                        );
                        
                        if let Some(debug_id) = &debug_group_clone {
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
        
        AIClassification::AssignmentUpdate { reference_keywords, changes, new_deadline, new_title, new_description, parallel_code, .. } => {
            println!("🔄 UPDATE DETECTED");
            
            let new_deadline_clone = new_deadline.clone();
            let new_title_clone = new_title.clone();
            let changes_clone = changes.clone();
            let reference_keywords_clone = reference_keywords.clone();
            let new_description_clone = new_description.clone();
            let pool_clone = pool.clone();
            let parallel_code_clone = parallel_code.clone();
            let update_msg_id = message_id.clone();

            // Fetch course_map BEFORE spawning
            let course_map_result = sqlx::query_as::<_, (uuid::Uuid, String)>(
                "SELECT id, name FROM courses"
            )
            .fetch_all(&pool_clone)
            .await;
            
            tokio::spawn(async move {
                // Build course_map from the fetched data
                let course_map: std::collections::HashMap<uuid::Uuid, String> = match course_map_result {
                    Ok(courses) => courses.into_iter().collect(),
                    Err(e) => {
                        eprintln!("❌ Failed to fetch course map: {}", e);
                        return;
                    }
                };
                
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
                            &assignments,
                            &course_map,
                            parallel_code_clone.as_deref(),
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
                                    parallel_code_clone,
                                    Some(update_msg_id),
                                ).await {
                                    Ok(updated) => {
                                        let response = format!(
                                            "🔄 *INFO TUGAS DIPERBARUI*\n\
                                            ━━━━━━━━━━━━━━━━━━\n\
                                            📝 *{}*\n\
                                            ⚠️ Perubahan: _{}_\n\
                                            📅 Deadline Baru: {}\n\
                                            ━━━━━━━━━━━━━━━━━━",
                                            updated.title,
                                            changes_clone,
                                            new_deadline_clone.unwrap_or("Tetap".to_string())
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
                                    message_id: update_msg_id,
                                };
                                
                                match crud::create_assignment(&pool_clone, new_assignment).await {
                                    Ok(_) => {
                                        let response = format!(
                                            "✨ *TUGAS BARU TERSIMPAN* ✨\n\
                                            ━━━━━━━━━━━━━━━━━━\n\
                                            📚 *{}*\n\
                                            📝 {}\n\
                                            📅 Deadline: {}\n\
                                            ━━━━━━━━━━━━━━━━━━\n\
                                            📄 _{}_",
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
        Err(format!("WAHA API error: {} - {}", status, body))
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
    
    // Check for "all" first (case-insensitive)
    if upper.contains("ALL") {
        return Some("all".to_string());
    }
    
    // Then check for specific codes
    for code in ["K1", "K2", "K3", "P1", "P2", "P3"] {
        if upper.contains(code) {
            return Some(code.to_lowercase());
        }
    }
    None
}

async fn fetch_image_from_url(url: &str, api_key: &str) -> Result<String, String> {
    // Fix URL if needed
    let corrected_url = url.replace("http://localhost:3000", "http://localhost:3001");
    
    //println!("   📡 Downloading: {}", corrected_url);
    
    let client = reqwest::Client::new();
    let response = client
        .get(&corrected_url)
        .header("X-Api-Key", api_key)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }
    
    let image_bytes = response.bytes().await
        .map_err(|e| format!("Failed to read bytes: {}", e))?;
    
    // Check size and compress if needed
    let base64_size_mb = (image_bytes.len() * 4 / 3) as f64 / 1_000_000.0;
    
    
    if base64_size_mb > 3.5 {
        println!("   🔄 Compressing image (too large for Groq)...");
        
        // Use the older, more compatible image loading API
        use image::io::Reader as ImageReader;
        use std::io::Cursor;
        
        let img = ImageReader::new(Cursor::new(&image_bytes))
            .with_guessed_format()
            .map_err(|e| format!("Failed to guess format: {}", e))?
            .decode()
            .map_err(|e| format!("Failed to decode image: {}", e))?;
        
        // Resize to max 2048px on longest side
        let img = img.thumbnail(2048, 2048);
        
        // Re-encode as JPEG with compression
        let mut compressed_bytes = Vec::new();
        let mut cursor = Cursor::new(&mut compressed_bytes);
        img.write_to(&mut cursor, image::ImageOutputFormat::Jpeg(80))
            .map_err(|e| format!("Failed to compress: {}", e))?;
        
        let compressed_size_mb = (compressed_bytes.len() * 4 / 3) as f64 / 1_000_000.0;
        println!("   ✅ Compressed to: {:.2} MB", compressed_size_mb);
        
        use base64::{Engine as _, engine::general_purpose};
        Ok(general_purpose::STANDARD.encode(&compressed_bytes))
    } else {
        // Image is already small enough
        use base64::{Engine as _, engine::general_purpose};
        Ok(general_purpose::STANDARD.encode(&image_bytes))
    }
}
