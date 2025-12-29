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
use std::io::Write;
use tokio::sync::Mutex;  
use tokio::net::TcpListener;
use sqlx::PgPool;
use chrono::{DateTime, Utc, NaiveDate};
use std::time::{Instant, Duration}; 
use std::collections::HashMap;

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
type SpamTracker = Arc<Mutex<HashMap<String, (u32, Instant)>>>;


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
    spam_tracker: SpamTracker, 
    pool: PgPool,
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
    
    
    let spam_tracker = Arc::new(Mutex::new(HashMap::new())); 

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
        spam_tracker, // ✅ BARU
        whitelist, 
        pool
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

    // Ignore messages from debug group
    let debug_group_id = std::env::var("DEBUG_GROUP_ID").ok();

    // ✅ EXTRACT SENDER AND CHAT IDs
    let chat_id = &payload.payload.from;  
    
    // Extract sender's actual phone number
    let sender_phone = if chat_id.ends_with("@g.us") {
        payload.payload.participant
            .as_ref()
            .unwrap_or(chat_id)
    } else {
        chat_id
    };
    
    // ✅ Extract WhatsApp display name
    let sender_name = payload.payload.data
        .as_ref()
        .and_then(|data| data.push_name.as_ref())
        .map(|name| name.as_str())
        .unwrap_or_else(|| {
            sender_phone.split('@').next().unwrap_or(sender_phone)
        });

    
    // STEP 1: CLASSIFY MESSAGE DULUAN (Supaya bisa cek is_command)
    let message_type = classify_message(&payload.payload.body);
    let is_command = matches!(message_type, MessageType::Command(_));


    // ANTI-SPAM (HANYA UNTUK COMMAND)
    if is_command {
        const MAX_MESSAGES: u32 = 5;      // Batas 5 command
        const WINDOW_SECONDS: u64 = 30;   // Dalam 30 detik

        let mut tracker = state.spam_tracker.lock().await;
        
        let (count, reset_time) = tracker
            .entry(sender_phone.to_string())
            .or_insert((0, Instant::now() + Duration::from_secs(WINDOW_SECONDS)));

        // Cek apakah waktu reset sudah lewat?
        if Instant::now() > *reset_time {
            *count = 1;
            *reset_time = Instant::now() + Duration::from_secs(WINDOW_SECONDS);
        } else {
            *count += 1;
        }

        // Cek BATAS
        if *count > MAX_MESSAGES {
            println!("🚫 SPAM COMMAND BLOCKED: {} sent > {} cmds/{}s", sender_phone, MAX_MESSAGES, WINDOW_SECONDS);
            
            if *count == MAX_MESSAGES + 1 {
                let warning_msg = "⚠️ *RATE LIMIT REACHED*\nAnda mengirim command terlalu cepat. Harap tunggu sebentar.";
                let _ = send_reply(chat_id, warning_msg).await;
            }

            return StatusCode::OK;
        }
    }


    // Terminal logging
    println!("📨 Message from: {}", chat_id);
    println!("   Sender: {} ({})", sender_name, sender_phone);
    println!("   Body: {}", payload.payload.body);
    println!("   Type: {:?}", message_type);

    // ============= CLARIFICATION HANDLER =============
    if let Some(quoted) = payload.payload.get_quoted_message() {
        if quoted.text.contains("⚠️ *PERLU KLARIFIKASI*") {
            println!("📝 Clarification response detected from {}", sender_phone);
            
            if let Some(assignment_id) = clarification::extract_assignment_id_from_message(&quoted.text) {
                println!("🔍 Updating assignment: {}", assignment_id);
                
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

                let new_deadline = updates.get("deadline").and_then(|d| crud::parse_deadline(d).ok());
                let new_title = updates.get("title").cloned();
                let new_description = updates.get("description").cloned();
                let new_parallel = updates.get("parallel_code").map(|p| p.to_lowercase());

                let course_id = if let Some(course_name) = updates.get("course_name") {
                    match crud::get_course_by_name(&state.pool, course_name).await {
                        Ok(Some(course)) => Some(course.id),
                        Ok(None) => {
                            let error_msg = format!("❌ Mata kuliah '{}' tidak ditemukan.", course_name);
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

                match crud::update_assignment_fields(
                    &state.pool,
                    assignment_id,
                    new_deadline,
                    new_title.clone(),
                    new_description.clone(),
                    new_parallel.clone(),
                    None,
                ).await {
                    Ok(updated) => {
                        if let Some(cid) = course_id {
                             if let Err(e) = sqlx::query("UPDATE assignments SET course_id = $1 WHERE id = $2")
                                .bind(cid).bind(assignment_id).execute(&state.pool).await {
                                    eprintln!("❌ Failed to update course_id: {}", e);
                                }
                        }
                        
                        let display_course = if let Some(cn) = updates.get("course_name") { cn.to_string() } else { "Unknown".to_string() };
                        
                        let response = format!(
                            "✅ *KLARIFIKASI TERSIMPAN*\n
                            ━━━━━━━━━━━━━━━━━━\n
                            📝 *{}*\n
                            📚 {}\n
                            ⏰ Deadline: {}\n_
                            Terima kasih atas klarifikasinya!_",
                            updated.title,
                            display_course,
                            updated.deadline.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or("-".to_string())
                        );
                        
                        if let Err(e) = send_reply(chat_id, &response).await {
                            eprintln!("❌ Failed to send confirmation: {}", e);
                        }
                    }
                    Err(e) => {
                        let error_msg = format!("❌ Gagal menyimpan: {}", e);
                         if let Err(e) = send_reply(chat_id, &error_msg).await {
                            eprintln!("❌ Failed to send error: {}", e);
                        }
                    }
                }
                return StatusCode::OK;
            }
        }
    }
    // ============= END CLARIFICATION =============

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
                        if let Err(e) = send_reply(chat_id, &warning).await {
                            eprintln!("❌ Failed to send warning: {}", e);
                        }
                    }
                }
            }
        }

        MessageType::NeedsAI(text) => {
            println!("🤖 Processing with AI...");
            
            // Image handling (GUNAKAN VERSI AMAN DARI KODE ORIGINAL ANDA)
            let image_base64 = if payload.payload.has_media.unwrap_or(false) {
                if let Some(ref media) = payload.payload.media {
                    if let Some(ref media_url) = media.url {
                         if media.mimetype.as_ref().map(|m| m.starts_with("image/")).unwrap_or(false) {
                            let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
                            // Pakai fetch_image_from_url yang AMAN
                            match fetch_image_from_url(media_url, &api_key).await {
                                Ok(base64) => Some(base64),
                                Err(e) => {
                                    eprintln!("❌ Failed to download image: {}", e);
                                    None
                                }
                            }
                         } else { None }
                    } else { None }
                } else { None }
            } else { None };
            
            // Context fetching
            let courses_list = crud::get_all_courses_formatted(&state.pool).await.unwrap_or_default();
            let active_assignments = crud::get_active_assignments(&state.pool).await.unwrap_or_default();
            
            let course_map = sqlx::query_as::<_, (uuid::Uuid, String)>("SELECT id, name FROM courses")
                .fetch_all(&state.pool).await.map(|rows| rows.into_iter().collect()).unwrap_or_default();
            
            // Extract AI
            match extract_with_ai(&text, &courses_list, &active_assignments, &course_map, image_base64.as_deref()).await {
                Ok(classification) => {
                    println!("✅ AI Classification: {:?}\n", classification);
                    handle_ai_classification(state.pool.clone(), classification, &payload.payload.id, sender_phone, debug_group_id).await;
                }
                Err(e) => {
                    eprintln!("❌ AI extraction failed: {}", e);
                    let _ = send_reply(chat_id, "❌ Failed to process message").await;
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
        return Err(format!("Failed to forward message"));
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
            let pool_clone = pool.clone();
            let course_name_lookup = course_name.clone();
            let title_clone = title.clone();
            let desc_clone = description.clone().unwrap_or("No description".to_string());
            let deadline_parsed = parse_deadline(&deadline);
            let parallel_code = extract_parallel_code(&title);
            let debug_group = debug_group_id.clone();
            
            tokio::spawn(async move {
                let course_id = if let Some(name) = &course_name_lookup {
                    crud::get_course_by_name(&pool_clone, name).await.ok().flatten().map(|c| c.id)
                } else { None };
                
                // Duplicate check
                if let Some(cid) = course_id {
                    if let Ok(Some(existing)) = crud::get_assignment_by_title_and_course(&pool_clone, &title_clone, cid).await {
                         let _ = crud::update_assignment_fields(
                            &pool_clone, existing.id, deadline_parsed, None, Some(desc_clone), None, Some(message_id)
                        ).await;
                        
                        if let Some(debug_id) = &debug_group {
                            let _ = send_reply(debug_id, &format!("🔄 *DUPLICATE UPDATED*: {}", title_clone)).await;
                        }
                        return;
                    }
                }
                
                // Create
                let new_assignment = NewAssignment {
                    course_id, title: title_clone.clone(), description: desc_clone.clone(),
                    deadline: deadline_parsed, parallel_code, sender_id: Some(sender_id), message_id
                };
                
                match crud::create_assignment(&pool_clone, new_assignment).await {
                    Ok(_) => {
                        // Clarification check
                        if let Some(cid) = course_id {
                             if let Ok(Some(assignment)) = crud::get_assignment_by_title_and_course(&pool_clone, &title_clone, cid).await {
                                 if let Ok(Some(full_assign)) = crud::get_assignment_with_course_by_id(&pool_clone, assignment.id).await {
                                     let missing = clarification::identify_missing_fields(&full_assign);
                                     if !missing.is_empty() {
                                         if let Some(debug_id) = &debug_group {
                                             let msg = clarification::generate_clarification_message(&full_assign, &missing);
                                             let _ = send_reply(debug_id, &msg).await;
                                         }
                                         return;
                                     }
                                 }
                             }
                        }

                        // Success
                        if let Some(debug_id) = &debug_group {
                            let _ = send_reply(debug_id, &format!("✨ *NEW TASK*: {}\n📚 {}", title_clone, course_name_lookup.unwrap_or_default())).await;
                        }
                    }
                    Err(e) => eprintln!("Failed to save: {}", e),
                }
            });
        }
        
        AIClassification::AssignmentUpdate { reference_keywords, changes, new_deadline, new_title, new_description, parallel_code, .. } => {
            let pool_clone = pool.clone();
            let updates = (new_deadline, new_title, new_description, parallel_code);
            let msg_id = message_id.clone();

            tokio::spawn(async move {
                let course_map = sqlx::query_as::<_, (uuid::Uuid, String)>("SELECT id, name FROM courses")
                    .fetch_all(&pool_clone).await.map(|r| r.into_iter().collect()).unwrap_or_default();
                
                // Try find course
                let mut course_id = None;
                for kw in &reference_keywords {
                     if let Ok(Some(c)) = crud::get_course_by_name_or_alias(&pool_clone, kw).await {
                         course_id = Some(c.id); break;
                     }
                }
                
                if let Ok(assignments) = crud::get_recent_assignments_for_update(&pool_clone, course_id).await {
                     if let Ok(Some(assign_id)) = parser::ai_extractor::match_update_to_assignment(
                         &changes, &reference_keywords, &assignments, &course_map, updates.3.as_deref()
                     ).await {
                         let d = if let Some(s) = &updates.0 { crud::parse_deadline(s).ok() } else { None };
                         let _ = crud::update_assignment_fields(&pool_clone, assign_id, d, updates.1, updates.2, updates.3, Some(msg_id)).await;
                         
                         if let Some(debug_id) = &debug_group_id {
                             let _ = send_reply(debug_id, &format!("🔄 *UPDATED*: {}", changes)).await;
                         }
                         return;
                     }
                }
                
                // Fallback Create
                if let (Some(cid), Some(d_str)) = (course_id, updates.0) {
                     if let Ok(d) = crud::parse_deadline(&d_str) {
                         let t = reference_keywords.first().cloned().unwrap_or("Task".into());
                         let new_assign = NewAssignment {
                             course_id: Some(cid), title: t.clone(), description: changes.clone(),
                             deadline: Some(d), parallel_code: updates.3, sender_id: None, message_id: msg_id
                         };
                         let _ = crud::create_assignment(&pool_clone, new_assign).await;
                         if let Some(debug_id) = &debug_group_id {
                             let _ = send_reply(debug_id, &format!("✨ *FALLBACK TASK*: {}", t)).await;
                         }
                     }
                }
            });
        }
        AIClassification::Unrecognized => {}
    }
}

async fn send_reply(chat_id: &str, text: &str) -> Result<(), String> {
    let waha_url = "http://localhost:3001/api/sendText";
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    let payload = SendTextRequest { chat_id: chat_id.to_string(), text: text.to_string(), session: "default".to_string() };
    let client = reqwest::Client::new();
    let res = client.post(waha_url).header("X-Api-Key", api_key).json(&payload).send().await.map_err(|e| e.to_string())?;
    if res.status().is_success() { Ok(()) } else { Err(format!("API Error")) }
}

fn parse_deadline(s: &Option<String>) -> Option<DateTime<Utc>> {
    s.as_ref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
     .and_then(|d| d.and_hms_opt(23, 59, 59)).map(|n| DateTime::from_naive_utc_and_offset(n, Utc))
}

fn extract_parallel_code(title: &str) -> Option<String> {
    let u = title.to_uppercase();
    if u.contains("ALL") { return Some("all".into()); }
    ["K1", "K2", "K3", "P1", "P2", "P3"].iter().find(|&c| u.contains(c)).map(|c| c.to_lowercase())
}

async fn fetch_image_from_url(url: &str, api_key: &str) -> Result<String, String> {
    let url = url.replace("http://localhost:3000", "http://localhost:3001");
    let client = reqwest::Client::new();
    let res = client.get(&url).header("X-Api-Key", api_key).send().await.map_err(|e| e.to_string())?;
    
    if !res.status().is_success() { 
        return Err(format!("HTTP Error: {}", res.status())); 
    }
    
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    
    use base64::{Engine as _, engine::general_purpose};
    use image::io::Reader as ImageReader;
    use std::io::Cursor;

    if (bytes.len() as f64 / 1_000_000.0) > 3.5 {
         println!("   🔄 Compressing image...");
         
         let img = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| format!("Format error: {}", e))?
            .decode()
            .map_err(|e| format!("Decode error: {}", e))?;
         
         let img = img.thumbnail(2048, 2048);
         let mut buf = Vec::new();
         img.write_to(&mut Cursor::new(&mut buf), image::ImageOutputFormat::Jpeg(80))
            .map_err(|e| format!("Compress error: {}", e))?;
            
         Ok(general_purpose::STANDARD.encode(&buf))
    } else {
         Ok(general_purpose::STANDARD.encode(&bytes))
    }
}