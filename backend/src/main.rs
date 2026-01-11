use axum::{
    extract::State,
    routing::{post, get},
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
use std::time::{Instant, Duration}; 
use std::collections::HashMap;
//use chrono::{Datelike};
use chrono::Duration as ChronoDuration;

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
use parser::ai_extractor::{extract_with_ai, check_duplicate_assignment}; 
use whitelist::Whitelist;

type MessageCache = Arc<Mutex<HashSet<String>>>;
type SpamTracker = Arc<Mutex<HashMap<String, (u32, Instant)>>>;


const BANNER_ART: &str = r#"
███╗   ███╗ █████╗ ██████╗ 
████╗ ████║██╔══██╗██╔══██╗
██╔████╔██║███████║██████╔╝
██║╚██╔╝██║██╔══██║██╔══██╗
██║ ╚═╝ ██║██║  ██║██║  ██║
╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝"#;

const BANNER_ART_BOT: &str = r#"
██████╗  ██████╗ ████████╗
██╔══██╗██╔═══██╗╚══██╔══╝
██████╔╝██║   ██║   ██║   
██╔══██╗██║   ██║   ██║   
██████╔╝╚██████╔╝   ██║   
╚═════╝  ╚═════╝    ╚═╝"#;

const BANNER_SUBTITLE: &str = r#"
         [WhatsApp Academic Assistant v1.0]          
              Created by Gilang & Arya"#;

#[derive(Clone)]
struct AppState {
    cache: MessageCache,
    spam_tracker: SpamTracker, 
    whitelist: Arc<Whitelist>,
    pool: PgPool,
}

/// Health check endpoint for Docker
async fn health_check() -> Json<serde_json::Value> {
    use serde_json::json;
    Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Check WAHA service health and authentication status
async fn check_waha_health() -> String {
    // Try multiple URLs in order of preference
    let waha_urls = vec![
        std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()),
        "http://localhost:3001".to_string(),
        "[http://127.0.0.1:3001](http://127.0.0.1:3001)".to_string(),
    ];
    
    let api_key = std::env::var("WAHA_API_KEY")
        .unwrap_or_else(|_| "devkey123".to_string());
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();
    
    // Try each URL until one works
    for waha_url in &waha_urls {
        let session_url = format!("{}/api/sessions/default", waha_url);
        
        match client
            .get(&session_url)
            .header("X-Api-Key", &api_key)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    if let Ok(session_data) = response.json::<serde_json::Value>().await {
                        let status = session_data["status"].as_str().unwrap_or("UNKNOWN");
                        
                        return match status {
                            "WORKING" => "\x1b[32m✅ AUTHENTICATED\x1b[0m".to_string(),
                            "SCAN_QR_CODE" => "\x1b[33m⚠️  NEEDS QR SCAN\x1b[0m".to_string(),
                            "STARTING" => "\x1b[36m🔄 STARTING...\x1b[0m".to_string(),
                            "FAILED" => "\x1b[31m❌ FAILED\x1b[0m".to_string(),
                            _ => format!("\x1b[33m⚠️  STATUS: {}\x1b[0m", status),
                        };
                    } else {
                        return "\x1b[32m✅ CONNECTED\x1b[0m".to_string();
                    }
                } else if response.status().as_u16() == 401 {
                    return "\x1b[31m❌ INVALID API KEY\x1b[0m".to_string();
                }
            }
            Err(_) => {
                // Try next URL
                continue;
            }
        }
    }
    
    // All URLs failed
    "\x1b[31m❌ DOCKER NOT RUNNING\x1b[0m".to_string()
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    // 1. Tampilan Awal (Clear Screen & Banner)
    print!("\x1b[2J\x1b[1;1H");

    // Print MAR in white and BOT in Claude's orange side by side
    let mar_lines: Vec<&str> = BANNER_ART.lines().collect();
    let bot_lines: Vec<&str> = BANNER_ART_BOT.lines().collect();

    for i in 0..mar_lines.len().max(bot_lines.len()) {
        if i < mar_lines.len() {
            print!("\x1b[97m{}", mar_lines[i]);  // Bright white for MAR
        }
        if i < bot_lines.len() {
            print!("\x1b[38;2;224;128;79m{}", bot_lines[i]);  // Claude's orange #E0804F
        }
        println!("\x1b[0m");
    }

    println!("\x1b[90m{}\x1b[0m", BANNER_SUBTITLE);
    println!("\x1b[1;30m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");

    // 2. Cek Environment Variables
    let gemini_status = if std::env::var("GEMINI_API_KEY").is_ok() {
        "\x1b[32m✅ READY\x1b[0m"
    } else {
        "\x1b[31m❌ MISSING\x1b[0m"
    };

    println!(" 🔧 \x1b[1mSYSTEM CHECK\x1b[0m");
    println!("    ├─ 🧠 Gemini AI    : {}", gemini_status);

    // WAHA Health Check
    print!("    ├─ 🔌 WAHA API     : 🔌 Checking...");
    std::io::stdout().flush().unwrap();

    let waha_status = check_waha_health().await;
    print!("\r    ├─ 🔌 WAHA API     : {}\x1b[K\n", waha_status);
    std::io::stdout().flush().unwrap();

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
        spam_tracker, 
        whitelist, 
        pool
    };
    
    let app = Router::new()
        .route("/webhook", post(webhook))
        .route("/health", get(health_check))
        .with_state(state);

    let port = 3000;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("\x1b[1;30m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!(" 🚀 \x1b[1;32mMARBOT IS ONLINE!\x1b[0m");
    println!("    📡 Listening on    : \x1b[36m[http://0.0.0.0](http://0.0.0.0):{}\x1b[0m", port);
    println!("    📍 Webhook URL     : \x1b[36mhttp://localhost:{}/webhook\x1b[0m", port);
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

    //MONITORING GUIS
    let request_start = Instant::now();

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


    // Terminal logging with compact formatting
    let body_display = payload.payload.body
        .replace('\n', "\\n")
        .chars()
        .take(80)
        .collect::<String>();

    let body_truncated = if payload.payload.body.len() > 80 {
        format!("\"{}...\"", body_display)
    } else {
        format!("\"{}\"", body_display)
    };

    let type_display = match &message_type {
        MessageType::Command(cmd) => format!("Command({:?})", cmd),
        MessageType::NeedsAI(_) => "NeedsAI".to_string(),
    };

    println!("\n| Message from: \x1b[32m{}\x1b[0m", chat_id);
    println!("| Sender      : \x1b[32m{}\x1b[0m (\x1b[32m{}\x1b[0m)", sender_name, sender_phone);
    println!("| Body        : \x1b[32m{}\x1b[0m", body_truncated);
    println!("| Type        : \x1b[32m{}\x1b[0m\n", type_display);
    
    // Extract quoted message text and id
    let (quoted_message_text, quoted_message_id) = if let Some(quoted) = payload.payload.get_quoted_message() {
        (Some(quoted.text.clone()), Some(quoted.id.clone()))  // Clone both to own them
    } else {
        (None, None)
    };

    if let Some(ref quoted) = quoted_message_text {
        println!("| Quoted: \"{}\"\n", 
            quoted.chars().take(80).collect::<String>());
    }

    // ============= CLARIFICATION HANDLER =============
    if let Some(quoted) = payload.payload.get_quoted_message() {
        let is_clarification_reply = quoted.text.contains("*[PERLU KLARIFIKASI]*") 
            || (quoted.text.contains("ID:") && quoted.text.contains("```"));
        
        if is_clarification_reply {
            println!("📝 Clarification response detected from {}", sender_phone);
            
            // 1. Extract ID Assignment dari pesan yang di-reply
            if let Some(assignment_id) = clarification::extract_assignment_id_from_message(&quoted.text) {
                
                // 2. Ambil data assignment saat ini dari database
                let current_assignment = crud::get_assignment_with_course_by_id(&state.pool, assignment_id)
                    .await
                    .ok()
                    .flatten();

                // 3. Identifikasi field apa yang hilang (PENTING untuk konteks AI)
                let missing_fields = if let Some(ref a) = current_assignment {
                    clarification::identify_missing_fields(a)
                } else {
                    Vec::new()
                };

                // 4. Parse Jawaban User menggunakan AI (Async)
                if let Some(ref assignment_obj) = current_assignment {
                    match clarification::parse_clarification_response(
                        &payload.payload.body, 
                        assignment_obj, 
                        &missing_fields
                    ).await {
                        Ok(updates) => {
                            // Extract fields from updates HashMap
                            let new_deadline = updates.get("deadline")
                                .and_then(|d| crud::parse_deadline(d).ok());
                            let new_title = updates.get("title").cloned();
                            let new_description = updates.get("description").cloned();
                            
                            // Parse parallel_codes
                            let new_parallel_codes = updates.get("parallel_codes")
                                .map(|codes_str| {
                                    codes_str.split(',')
                                        .map(|s| s.trim().to_lowercase())
                                        .collect::<Vec<String>>()
                                });

                            // Handle course_id lookup if course_name is provided
                            let course_id = if let Some(course_name) = updates.get("course_name") {
                                match crud::get_course_by_name(&state.pool, course_name).await {
                                    Ok(Some(course)) => Some(course.id),
                                    Ok(None) => {
                                        let error_msg = format!("❌ Mata kuliah '{}' tidak ditemukan.", course_name);
                                        let _ = send_reply(chat_id, &error_msg).await;
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

                            // Update Database
                            match crud::update_assignment_fields(
                                &state.pool,
                                assignment_id,
                                new_deadline,
                                new_title,
                                new_description,
                                new_parallel_codes,
                                Some(payload.payload.id.clone()),
                                Some(payload.payload.body.clone()), 
                            ).await {
                                Ok(_) => {
                                    // Update course_id jika ada perubahan
                                    if let Some(cid) = course_id {
                                        let _ = sqlx::query("UPDATE assignments SET course_id = $1 WHERE id = $2")
                                            .bind(cid)
                                            .bind(assignment_id)
                                            .execute(&state.pool)
                                            .await;
                                    }
                                    
                                    // Konfirmasi Berhasil
                                    if let Ok(Some(full_assignment)) = crud::get_assignment_with_course_by_id(&state.pool, assignment_id).await {
                                        let deadline_display = full_assignment.deadline
                                            .map(|d| {
                                                let indonesia_time = d + ChronoDuration::hours(7);
                                                indonesia_time.format("%Y-%m-%d %H:%M WIB").to_string()
                                            })
                                            .unwrap_or("(belum ditentukan)".to_string());

                                        let response = format!(
                                            "*[KLARIF TERSIMPAN]*\n\
                                            _Terima kasih! Data berhasil diperbarui._\n\
                                            \n\
                                            📝 *{}*\n\
                                            📚 {}\n\
                                            📄 {}\n\
                                            ⏰ {}\n\
                                            🧩 Parallel: {}",
                                            full_assignment.title,
                                            full_assignment.course_name,
                                            full_assignment.description.as_deref().unwrap_or("-"),
                                            deadline_display,
                                            full_assignment.format_parallel_display()
                                        );
                                        
                                        let _ = send_reply(chat_id, &response).await;
                                    } else {
                                        let _ = send_reply(chat_id, "✅ *KLARIFIKASI TERSIMPAN*").await;
                                    }
                                }
                                Err(e) => {
                                    let error_msg = format!("❌ Gagal menyimpan database: {}", e);
                                    let _ = send_reply(chat_id, &error_msg).await;
                                }
                            }
                        }
                        Err(err_type) => {
                            // Handle Error dari AI Parser
                            match err_type.as_str() {
                                "cancelled" => {
                                    let cancel_msg = clarification::generate_cancellation_message(assignment_id);
                                    let _ = send_reply(chat_id, &cancel_msg).await;
                                }
                                "no_data" => {
                                    let parse_fail_msg = clarification::generate_parse_failed_message();
                                    let _ = send_reply(chat_id, &parse_fail_msg).await;
                                }
                                "no_date" => {
                                    let no_date_msg = clarification::generate_no_date_message();
                                    let _ = send_reply(chat_id, &no_date_msg).await;
                                }
                                _ => {
                                    let _ = send_reply(chat_id, "❌ Maaf, aku tidak mengerti format pesanmu.").await;
                                }
                            }
                        }
                    }
                } else {
                    let _ = send_reply(chat_id, "❌ Data tugas tidak ditemukan/sudah dihapus.").await;
                }

                return StatusCode::OK;
            } else {
                println!("⚠️ Could not extract assignment ID from quoted message");
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
                CommandResponse::ResendMessages { messages, summary } => {
                    // send each stored message
                    for (i, msg_content) in messages.iter().enumerate() {
                        let formatted_msg = format!("*⮣* _Forwarded_ \n\n{}", msg_content);
                        
                        if let Err(e) = send_reply(chat_id, &formatted_msg).await {
                            eprintln!("❌ Failed to send message {}: {}", i + 1, e);
                        }
                        
                        // Delay between messages
                        if i < messages.len() - 1 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                        }
                    }

                    // Small delay before sending messages
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                    // First send summary
                    if let Err(e) = send_reply(chat_id, &summary).await {
                        eprintln!("❌ Failed to send summary: {}", e);
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
            let assignments = crud::get_assignments(&state.pool).await.unwrap_or_default();
            
            let course_map = sqlx::query_as::<_, (uuid::Uuid, String)>("SELECT id, name FROM courses")
                .fetch_all(&state.pool).await.map(|rows| rows.into_iter().collect()).unwrap_or_default();
            
            // START MONITORING: AI Latency Timer
            let ai_start = Instant::now();
            
            // Pass quoted message to AI
            match extract_with_ai(
                &text, 
                &courses_list, 
                &assignments, 
                &course_map, 
                image_base64.as_deref(),
                sender_phone,   
                &state.pool,
                quoted_message_text.as_deref(),
                quoted_message_id.as_deref(),
            ).await {
                Ok(classification) => {
                    //  STOP MONITORING: Log AI Duration
                    let ai_duration = ai_start.elapsed();
                    println!("🧠 AI Latency: {:.2?}", ai_duration);

                    println!("✅ AI Classification: {:?}\n", classification);
                    handle_ai_classification(
                        state.pool.clone(), 
                        classification, 
                        &payload.payload.id, 
                        sender_phone, 
                        &payload.payload.body,
                        debug_group_id
                    ).await;
                }
                Err(e) => {
                    eprintln!("❌ AI extraction failed: {}", e);
                    let _ = send_reply(chat_id, "❌ Failed to process message").await;
                }
            }
        }
    }
    
    // STOP MONITORING: Global Request Timer
    let total_duration = request_start.elapsed();
    println!("⏱️  Total Request Processed in: {:.2?}\n", total_duration);

    StatusCode::OK
}

#[allow(non_snake_case)]
async fn handle_ai_classification(
    pool: PgPool,
    classification: AIClassification, 
    message_id: &str,
    sender_id: &str,
    message_body: &str,
    debug_group_id: Option<String>,
) {
    let message_id = message_id.to_string();
    let sender_id = sender_id.to_string();
    let message_body = message_body.to_string();
    
    match classification {
        // NEW: Handle multiple assignments
        AIClassification::MultipleAssignments { assignments, .. } => {
            let debug_group = debug_group_id.clone();
            
            if let Some(debug_id) = &debug_group {
                let _ = send_reply(debug_id, &format!("📦 Processing {} assignments...", assignments.len())).await;
            }
            
            // CRITICAL: Deduplicate within the batch BEFORE processing
            let mut unique_assignments = Vec::new();
            let mut seen = std::collections::HashSet::new();
            
            for assignment in &assignments {
                // Create a unique key: course + title + parallel
                let parallel_key = if assignment.parallel_codes.is_empty() {
                    "none".to_string()
                } else {
                    assignment.parallel_codes.join(",")
                };
                
                let key = format!(
                    "{}::{}::{}",
                    assignment.course_name.to_lowercase(),
                    assignment.title.to_lowercase(),
                    parallel_key
                );
                
                if seen.insert(key) {
                    unique_assignments.push(assignment.clone());
                } else {
                    // Duplicate detected within batch
                    if let Some(debug_id) = &debug_group {
                        let _ = send_reply(
                            debug_id, 
                            &format!("⚠️ Skipped duplicate in message: {} - {}", 
                                assignment.course_name, 
                                assignment.title
                            )
                        ).await;
                    }
                }
            }
            
            if let Some(debug_id) = &debug_group {
                if unique_assignments.len() < assignments.len() {
                    let _ = send_reply(
                        debug_id, 
                        &format!("✅ Processing {} unique assignments (filtered {} duplicates)", 
                            unique_assignments.len(),
                            assignments.len() - unique_assignments.len()
                        )
                    ).await;
                }
            }
            
            // For MultipleAssignments:
            for (index, assignment) in unique_assignments.into_iter().enumerate() {
                handle_single_assignment(
                    pool.clone(),
                    Some(assignment.course_name),
                    assignment.title,
                    assignment.deadline,
                    assignment.description,
                    assignment.parallel_codes,
                    &message_id,
                    &sender_id,
                    &message_body,  // FIX: Pass message_body here
                    debug_group_id.clone(),
                    index + 1,
                ).await;
            }
        }
        
        // Single assignment - USE AI FOR DUPLICATE DETECTION
        AIClassification::AssignmentInfo { course_name, title, deadline, description, parallel_codes, .. } => {
            let debug_group = debug_group_id.clone();
            let msg_body = message_body.to_string();
            
            tokio::spawn(async move {
                handle_single_assignment(
                    pool,
                    course_name,
                    title,
                    deadline,
                    description,
                    parallel_codes,
                    &message_id,
                    &sender_id,
                    &msg_body,
                    debug_group,
                    0,
                ).await
            });
        }
        
        AIClassification::AssignmentUpdate { 
            reference_keywords, 
            changes, 
            new_deadline, 
            new_title, 
            new_description, 
            parallel_codes, 
            .. 
        } => {
            let pool_clone = pool.clone();
            let msg_id = message_id.clone();
            let debug_clone = debug_group_id.clone();

            tokio::spawn(async move {
                let course_map: HashMap<uuid::Uuid, String> = sqlx::query_as::<_, (uuid::Uuid, String)>(
                    "SELECT id, name FROM courses"
                )
                .fetch_all(&pool_clone)
                .await
                .map(|r| r.into_iter().collect())
                .unwrap_or_default();
                
                let course_name = reference_keywords.first().cloned();
                let course_id = if let Some(name) = &course_name {
                    crud::get_course_by_name(&pool_clone, name).await.ok().flatten().map(|c| c.id)
                } else {
                    None
                };
                
                let active_assignments = crud::get_recent_assignments_for_update(&pool_clone)
                    .await
                    .unwrap_or_default();
                
                // SMART UPDATE: Check for re-announcement
                if let Some(ref title) = new_title {
                    if let (Some(_course_id), Some(cname)) = (course_id, &course_name) {
                        let dup_check = check_duplicate_assignment(
                            title,
                            new_description.as_deref().unwrap_or(""),
                            cname,
                            &parallel_codes,
                            &active_assignments,
                            &course_map,
                        ).await;
                        
                        if let Ok(Some(id)) = dup_check {
                            println!("🔄 \x1b[33mRe-announcement\x1b[0m: \x1b[1m{}\x1b[0m", title);
                            
                            let deadline_parsed = new_deadline.as_ref()
                                .and_then(|d| crud::parse_deadline(d).ok());
                            
                            let _ = crud::update_assignment_fields(
                                &pool_clone,
                                id,
                                deadline_parsed,
                                None,
                                new_description.clone(),
                                if parallel_codes.is_empty() { None } else { Some(parallel_codes.clone()) },
                                Some(msg_id.clone()),
                                None,
                            ).await;
                            
                            if let Some(debug_id) = debug_clone {
                                let _ = send_reply(
                                    &debug_id,
                                    &format!("🔄 *UPDATED*: {}\n_{}_", title, changes)
                                ).await;
                            }
                            return;
                        }
                    }
                }
                
                // REGULAR UPDATE MATCHING
                match parser::ai_extractor::match_update_to_assignment(
                    &changes,
                    &reference_keywords,
                    &active_assignments,
                    &course_map,
                    &parallel_codes,
                ).await {
                    Ok(Some(assignment_id)) => {
                        let current_title = active_assignments.iter()
                            .find(|a| a.id == assignment_id)
                            .map(|a| a.title.clone())
                            .unwrap_or_else(|| "Unknown".to_string());
                        
                        println!("🔄 \x1b[33mUpdating\x1b[0m: \x1b[1m{}\x1b[0m", current_title);
                        
                        let deadline_parsed = new_deadline.as_ref()
                            .and_then(|d| crud::parse_deadline(d).ok());
                        
                        if let Ok(_) = crud::update_assignment_fields(
                            &pool_clone,
                            assignment_id,
                            deadline_parsed,
                            new_title.clone(),
                            new_description.clone(),
                            if parallel_codes.is_empty() { None } else { Some(parallel_codes.clone()) },
                            Some(msg_id),
                            None,
                        ).await {
                            if let Some(debug_id) = debug_clone {
                                let _ = send_reply(
                                    &debug_id,
                                    &format!("🔄 *UPDATED*: {}\n_{}_", current_title, changes)
                                ).await;
                            }
                        }
                    }
                    Ok(None) => {
                        println!("⚠️  \x1b[33mNo match found\x1b[0m for update: {:?}", reference_keywords);
                        
                        if let Some(debug_id) = debug_clone {
                            let _ = send_reply(
                                &debug_id,
                                "⚠️ Could not find assignment to update"
                            ).await;
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Update matching failed: {}", e);
                    }
                }
            });
        }
        
        AIClassification::Unrecognized => {}
    }
}

/// Handle a single assignment with improved AI-powered duplicate detection
#[allow(non_snake_case)]
async fn handle_single_assignment(
    pool: PgPool,
    course_name: Option<String>,
    title: String,
    deadline: Option<String>,
    description: Option<String>,
    parallel_codes: Vec<String>,
    message_id: &str,
    sender_id: &str,
    message_body: &str,
    debug_group_id: Option<String>,
    assignment_number: usize,
) {
    let title_clone = title.clone();
    let desc_clone = description.clone().unwrap_or("No description".to_string());
    let deadline_parsed = deadline.as_ref()
        .and_then(|d| crud::parse_deadline(d).ok());
    let parallel_code_parsed = extract_parallel_code(&title);

    // ✅ Build final parallel codes Vec - MORE EXPLICIT
    let final_parallel_codes: Vec<String> = {
        if !parallel_codes.is_empty() {
            parallel_codes.clone()
        } else if let Some(code) = parallel_code_parsed {
            vec![code]
        } else {
            Vec::<String>::new()
        }
    };
    
    let course_id = if let Some(name) = &course_name {
        crud::get_course_by_name(&pool, name).await.ok().flatten().map(|c| c.id)
    } else { None };
    
    // DUPLICATE DETECTION
    if let Some(_cid) = course_id {
        if let Some(cname) = &course_name {
            let course_map: HashMap<uuid::Uuid, String> = sqlx::query_as::<_, (uuid::Uuid, String)>(
                "SELECT id, name FROM courses"
            )
            .fetch_all(&pool)
            .await
            .map(|r| r.into_iter().collect())
            .unwrap_or_default();
            
            let existing_assignments = crud::get_recent_assignments_for_update(&pool)
                .await
                .unwrap_or_default();
            
            if !existing_assignments.is_empty() {
                let match_result = check_duplicate_assignment(
                    &title_clone,
                    &desc_clone,
                    cname,
                    final_parallel_codes.as_slice(),
                    &existing_assignments,
                    &course_map,
                ).await;
                
                match &match_result {
                    Ok(Some(id)) => {
                        // CLEAN LOG: Just show what's being updated
                        println!("🔄 \x1b[33mUpdating\x1b[0m: \x1b[1m{}\x1b[0m", title_clone);
                        
                        let update_result = crud::update_assignment_fields(
                            &pool, 
                            *id, 
                            deadline_parsed, 
                            None,
                            Some(desc_clone.clone()), 
                            if final_parallel_codes.is_empty() { None } else { Some(final_parallel_codes.clone()) },
                            Some(message_id.to_string()),
                            Some(message_body.to_string()),
                        ).await;
                        
                        if update_result.is_ok() {
                            if let Some(debug_id) = &debug_group_id {
                                let prefix = if assignment_number > 0 {
                                    format!("{}. ", assignment_number)
                                } else {
                                    String::new()
                                };
                                let _ = send_reply(
                                    debug_id, 
                                    &format!("{}🔄 *UPDATED*: {}", prefix, title_clone)
                                ).await;
                            }
                        }
                        return;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        // Silent fallback - no need to spam logs
                    }
                }
            }
        }
    }
    

    // CREATE NEW ASSIGNMENT (no duplicate found)
    let new_assignment = NewAssignment {
        course_id, 
        title: title_clone.clone(), 
        description: desc_clone.clone(),
        deadline: deadline_parsed, 
        parallel_codes: final_parallel_codes.clone(),
        sender_id: Some(sender_id.to_string()), 
        message_id: message_id.to_string(),
        relating_messages: vec![message_body.to_string()],  // still error
    };
    
    match crud::create_assignment(&pool, new_assignment).await {
        Ok(_) => {
            println!("✅ Assignment created: {}", title_clone);
            
            // ============ CLARIFICATION CHECK ============
            if let Some(cid) = course_id {
                if let Ok(Some(assignment)) = crud::get_assignment_by_title_and_course(&pool, &title_clone, cid).await {
                    if let Ok(Some(full_assign)) = crud::get_assignment_with_course_by_id(&pool, assignment.id).await {
                        
                        // Compact assignment info display
                        println!("🔍 Starting clarification check...");
                        println!("   \x1b[36m📚 {}\x1b[0m", full_assign.course_name);
                        println!("   \x1b[1m📝 {}\x1b[0m", full_assign.title);
                        
                        if let Some(ref desc) = full_assign.description {
                            let desc_display = desc.chars().take(60).collect::<String>();
                            let desc_final = if desc.len() > 60 {
                                format!("{}...", desc_display)
                            } else {
                                desc_display
                            };
                            println!("   \x1b[90m📄 {}\x1b[0m", desc_final);
                        }
                        
                        if let Some(deadline) = full_assign.deadline {
                            let indonesia_time = deadline + ChronoDuration::hours(7);
                            println!("   \x1b[32m⏰ {}\x1b[0m", indonesia_time.format("%Y-%m-%d %H:%M WIB"));
                        } else {
                            println!("   \x1b[33m⏰ (no deadline)\x1b[0m");
                        }
                        
                        if !full_assign.parallel_codes.is_empty() {
                            println!("   \x1b[35m🧩 {}\x1b[0m", full_assign.parallel_codes.join(", ").to_uppercase());
                        }
                        
                        // Check for missing fields
                        let missing = clarification::identify_missing_fields(&full_assign);
                        
                        if !missing.is_empty() {
                            println!("   \x1b[33m🔔 Missing: {}\x1b[0m", missing.join(", "));
                            
                            if let Some(debug_id) = &debug_group_id {
                                let (info_msg, template_msg) = clarification::generate_clarification_messages(&full_assign, &missing);
                                let combined_msg = format!("{}\n{}", info_msg, template_msg);

                                match send_reply(debug_id, &combined_msg).await {
                                    Ok(_) => println!("   \x1b[32m✅ Clarification sent\x1b[0m\n"),
                                    Err(e) => eprintln!("   \x1b[31m❌ Send failed: {}\x1b[0m\n", e),
                                }
                            }
                            return; // Don't send success message
                        } else {
                            println!("   \x1b[32m✅ Complete (no clarification needed)\x1b[0m\n");
                        }
                    }
                }
            }
            // ============ END CLARIFICATION ============

            // Success message (only if NO clarification needed)
            if let Some(debug_id) = &debug_group_id {
                let prefix = if assignment_number > 0 {
                    format!("{}. ", assignment_number)
                } else {
                    String::new()
                };
                
                let deadline_str = deadline_parsed
                    .map(|d| {
                        let indonesia_time = d + ChronoDuration::hours(7);
                        format!("\n⏰ {}", indonesia_time.format("%Y-%m-%d %H:%M WIB"))
                    })
                    .unwrap_or_default();
                
                let parallel_str = if !final_parallel_codes.is_empty() {
                    format!("\n🧩 Parallel: {}", final_parallel_codes.join(", ").to_uppercase())
                } else {
                    String::new()
                };
                
                println!("📤 Sending success message...");
                let _ = send_reply(
                    debug_id, 
                    &format!("{}✨ *NEW TASK*: {}\n📚 {}{}{}", 
                        prefix, 
                        title_clone, 
                        course_name.unwrap_or_default(),
                        deadline_str,
                        parallel_str
                    )
                ).await;
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to save assignment: {}", e);
            
            if let Some(debug_id) = &debug_group_id {
                let _ = send_reply(
                    debug_id, 
                    &format!("⚠️ Failed to save assignment: {}", title_clone)
                ).await;
            }
        }
    }
}


async fn send_reply(chat_id: &str, text: &str) -> Result<(), String> {
    let waha_url = format!("{}/api/sendText", std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()));
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    let payload = SendTextRequest { chat_id: chat_id.to_string(), text: text.to_string(), session: "default".to_string() };
    let client = reqwest::Client::new();
    let res = client.post(waha_url).header("X-Api-Key", api_key).json(&payload).send().await.map_err(|e| e.to_string())?;
    if res.status().is_success() { Ok(()) } else { Err(format!("API Error")) }
}

fn extract_parallel_code(title: &str) -> Option<String> {
    let u = title.to_uppercase();
    if u.contains("ALL") { return Some("all".into()); }
    ["K1", "K2", "K3", "P1", "P2", "P3"].iter().find(|&c| u.contains(c)).map(|c| c.to_lowercase())
}

async fn fetch_image_from_url(url: &str, api_key: &str) -> Result<String, String> {
    let waha_base = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string());
    let url = url.replace("http://localhost:3000", &waha_base);
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