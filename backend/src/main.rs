use axum::{
    extract::State,
    routing::{post, get},
    Json,
    Router,
};
use axum::http::StatusCode;
use axum::middleware;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;  
use std::io::Write;
use tokio::sync::{Mutex, mpsc};
use tokio::net::TcpListener;
use sqlx::PgPool;
use std::time::{Instant, Duration}; 
use std::collections::HashMap;
//use chrono::{Datelike};
use chrono::Duration as ChronoDuration;
use once_cell::sync::OnceCell;

pub mod models;
pub mod scheduler;
pub mod classifier;
pub mod parser;
pub mod whitelist;
pub mod database;
pub mod clarification;
pub mod tui;
pub mod dashboard;

use crate::database::crud;
use crate::parser::commands::CommandResponse;
use crate::tui::TuiState;

use models::{MessageType, AIClassification, WebhookPayload, SendTextRequest, NewAssignment, UnrecognizedCategory, Assignment};
use classifier::classify_message;
use parser::commands::handle_command;
use parser::ai_extractor::{extract_with_ai, check_duplicate_assignment}; 
use whitelist::Whitelist;

static TUI_STATE: OnceCell<Arc<tui::state::TuiState>> = OnceCell::new();

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

type MessageCache = Arc<Mutex<HashSet<String>>>;
type SpamTracker = Arc<Mutex<HashMap<String, (u32, Instant)>>>;

#[derive(Clone)]
pub struct AppState {
    pub cache: MessageCache,
    pub spam_tracker: SpamTracker, 
    pub whitelist: Arc<Whitelist>,
    pub pool: PgPool,
    pub log_tx: mpsc::UnboundedSender<tui::state::LogEntry>,
    pub tui_state: Arc<TuiState>,
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

    let waha_urls = vec![
        std::env::var("WAHA_URL").unwrap_or_else(|_| "http://marbot_waha:3001".to_string()),
        "http://localhost:3001".to_string(),
        "http://127.0.0.1:3001".to_string(),
    ];
    
    let api_key = std::env::var("WAHA_API_KEY")
        .unwrap_or_else(|_| "devkey123".to_string());
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();
    
  
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
               
                continue;
            }
        }
    }
    

    "\x1b[31m❌ DOCKER NOT RUNNING\x1b[0m".to_string()
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let mar_lines: Vec<&str> = BANNER_ART.lines().collect();
    let bot_lines: Vec<&str> = BANNER_ART_BOT.lines().collect();

    for i in 0..mar_lines.len().max(bot_lines.len()) {
        if i < mar_lines.len() {
            let line = mar_lines[i]
                .replace('█', "\x1b[38;2;255;255;255m█\x1b[38;2;198;97;63m")
                .replace('╗', "\x1b[38;2;198;97;63m╗")
                .replace('║', "\x1b[38;2;198;97;63m║")
                .replace('╔', "\x1b[38;2;198;97;63m╔")
                .replace('═', "\x1b[38;2;198;97;63m═")
                .replace('╝', "\x1b[38;2;198;97;63m╝")
                .replace('╚', "\x1b[38;2;198;97;63m╚");
            print!("{}", line);
        }
        if i < bot_lines.len() {
            let line = bot_lines[i]
                .replace('█', "\x1b[38;2;255;255;255m█\x1b[38;2;198;97;63m")
                .replace('╗', "\x1b[38;2;198;97;63m╗")
                .replace('║', "\x1b[38;2;198;97;63m║")
                .replace('╔', "\x1b[38;2;198;97;63m╔")
                .replace('═', "\x1b[38;2;198;97;63m═")
                .replace('╝', "\x1b[38;2;198;97;63m╝")
                .replace('╚', "\x1b[38;2;198;97;63m╚");
            print!("{}", line);
        }
        println!("\x1b[0m");
    }

    println!("\x1b[90m{}\x1b[0m", BANNER_SUBTITLE);
    println!("\x1b[1;30m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");

    // 2. Check Environment Variables
    let gemini_status = if std::env::var("GEMINI_API_KEY").is_ok() {
        "\x1b[32m✅ READY\x1b[0m"
    } else {
        "\x1b[31m❌ MISSING\x1b[0m"
    };

    println!(" 🔧 \x1b[1mSYSTEM CHECK\x1b[0m");
    println!("    ├─ 🧠 Gemini AI\t: {}", gemini_status);

    // WAHA Health Check
    print!("    ├─ 🔌 WAHA API\t: 🔌 Checking...");
    std::io::stdout().flush().unwrap();

    let waha_status = check_waha_health().await;
    print!("\r    ├─ 🔌 WAHA API\t: {}\x1b[K\n", waha_status);
    std::io::stdout().flush().unwrap();

    // 3. Database Connection
    print!("    ├─ 💾 Database\t: 🔌 Connecting...");
    std::io::stdout().flush().unwrap();

    let pool = match database::pool::create_pool().await {
        Ok(p) => {
            print!("\r    ├─ 💾 Database\t: \x1b[32m✅ CONNECTED\x1b[0m\x1b[K\n");
            std::io::stdout().flush().unwrap();
            p
        }
        Err(e) => {
            print!("\r    ├─ 💾 Database\t: \x1b[31m❌ FAILED\x1b[0m\x1b[K\n");
            std::io::stdout().flush().unwrap();
            eprintln!("       └─ Error: {}", e);
            return;
        }
    };

    let whitelist = Arc::new(Whitelist::new());
    let cache = Arc::new(Mutex::new(HashSet::new()));
    let spam_tracker = Arc::new(Mutex::new(HashMap::new())); 


    let spam_tracker_clone = spam_tracker.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            
            let mut tracker = spam_tracker_clone.lock().await;
            let now = Instant::now();
            
           
            tracker.retain(|_, (_, reset_time)| now < *reset_time);
            
            // Safety limit
            if tracker.len() > 1000 {
                tracker.clear();
            }
        }
    });

    // 4. Initialize TUI System
    let (tui_state, log_tx) = tui::init();
    
    
    TUI_STATE.set(tui_state.clone()).ok();
    
    tui::spawn_log_collector(tui_state.clone());

 
    tui_state.clone().start_periodic_cleanup();

    // 5. Run Scheduler
    let pool_for_scheduler = pool.clone();
    let log_tx_for_scheduler = log_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        if let Err(e) = scheduler::start_scheduler(pool_for_scheduler, log_tx_for_scheduler).await {
            eprintln!("\n\x1b[31m❌ Scheduler Error: {:?}\x1b[0m", e);
        }
    });
    println!("    └─ ⏰ Scheduler\t: \x1b[32m✅ RUNNING\x1b[0m");

    let state = AppState { 
        cache,
        spam_tracker, 
        whitelist, 
        pool,
        log_tx,
        tui_state: tui_state.clone(),  
    };
    
  
    let dashboard_routes = Router::new()
        .route("/tui", get(dashboard::serve_dashboard_page))
        .route("/tui/api/data", get(dashboard::get_dashboard_data))
        .route_layer(middleware::from_fn(dashboard::basic_auth_middleware));
    
   
    let app = Router::new()
        .route("/webhook", post(webhook))
        .route("/health", get(health_check))
        .merge(dashboard_routes)
        .with_state(state);

    let port = 3000;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("\x1b[1;30m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!(" 🚀 \x1b[1;32mMARBOT IS ONLINE!\x1b[0m");
    println!("    📡 Listening on\t: \x1b[36mhttp://0.0.0.0:{}\x1b[0m", port);
    println!("    📍 Webhook URL\t: \x1b[36mhttp://localhost:{}/webhook\x1b[0m", port);
    println!("    🎨 Dashboard\t: \x1b[36mhttp://43.133.129.209:{}/tui\x1b[0m", port);
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

    let request_start = Instant::now();

   
    if payload.event != "message.any" {
        return StatusCode::OK;
    }

    
    let dedup_key = format!(
        "{}:{}:{}",
        payload.payload.id,
        payload.payload.from,
        payload.payload.body.chars().take(50).collect::<String>()
    );

    {
        let mut cache = state.cache.lock().await;
        if !cache.insert(dedup_key.clone()) { 
            return StatusCode::OK;
        }

        if cache.len() > 100 {
            cache.clear();
        }
    }


    if payload.payload.from_me {
        return StatusCode::OK;
    }

    // Ignore messages from debug group
    let debug_group_id = std::env::var("DEBUG_GROUP_ID").ok();

    // EXTRACT SENDER AND CHAT IDs
    let chat_id = &payload.payload.from;  
    

    let sender_phone = if chat_id.ends_with("@g.us") {
        payload.payload.participant
            .as_ref()
            .unwrap_or(chat_id)
    } else {
        chat_id
    };
    
    // Extract WhatsApp display name 
    let sender_name = payload.payload.data
        .as_ref()
        .and_then(|data| {

            data.info.as_ref()
                .and_then(|info| info.push_name.as_ref())
                .map(|s| s.as_str())
                // Fallback to WEBJS/NOWEB
                .or_else(|| data.push_name.as_ref().map(|s| s.as_str()))
        })
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

        // Cek apakah waktu reset 
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


    let (quoted_message_text, quoted_message_id) = if let Some(quoted) = payload.payload.get_quoted_message() {
        (Some(quoted.text.clone()), Some(quoted.id.clone()))  
    } else {
        (None, None)
    };

    
    // TUI INTEGRATION: Create job logger
    let job_id = tui::generate_job_id();
    let logger = tui::JobLogger::new(job_id.clone(), state.log_tx.clone());
    let mut tags = Vec::new();

    match &message_type {
        MessageType::Command(cmd) => {
            use crate::models::BotCommand;
            let cmd_tag = match cmd {
                BotCommand::Ping => "ping",
                BotCommand::Tugas => "tugas",
                BotCommand::Todo => "todo",
                BotCommand::Today => "today",
                BotCommand::Week => "week",
                BotCommand::Help => "help",
                BotCommand::Undo => "undo",
                BotCommand::Done(_) => "done",
                BotCommand::Delete(_) => "delete",
                BotCommand::Expand(_) => "expand",
                BotCommand::SetKelas(_, _) => "setkelas",
                BotCommand::MyKelas => "mykelas",
                BotCommand::MissingArgument(_) => "error",
                BotCommand::UnknownCommand(_) => "unknown",
                BotCommand::Update(_, _) => "ai",
            };
            tags.push(format!("#{}", cmd_tag));
        }
        MessageType::NeedsAI(_) => {
            tags.push("#ai".to_string());
        }
    }


    tags.sort();
    tags.dedup();


    let message_body_for_search = Some(payload.payload.body.clone());
    let quoted_message_for_search = quoted_message_text.clone();

    if let Some(tui_state) = TUI_STATE.get() {
        tui_state.create_job(
            job_id.clone(),
            chat_id.to_string(),
            sender_name.to_string(),
            message_body_for_search,
            quoted_message_for_search,
            tags,
        ).await;
    }

    
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

    logger.log(&format!("\n| Message from: \x1b[32m{}\x1b[0m", chat_id));
    logger.log(&format!("| Sender      : \x1b[32m{}\x1b[0m (\x1b[32m{}\x1b[0m)", sender_name, sender_phone));
    logger.log(&format!("| Body        : \x1b[32m{}\x1b[0m", body_truncated));
    logger.log(&format!("| Type        : \x1b[32m{}\x1b[0m\n", type_display));

    if let Some(ref quoted) = quoted_message_text {
        logger.log(&format!("| Quoted: \"{}\"\n", 
            quoted.chars().take(80).collect::<String>()));
    }

    // ============= CLARIFICATION HANDLER =============
    if let Some(quoted) = payload.payload.get_quoted_message() {
        let is_clarification_reply = quoted.text.contains("*[PERLU KLARIFIKASI]*") 
            || (quoted.text.contains("ID:") && quoted.text.contains("```"));
        
        if is_clarification_reply {
            logger.log(&format!("📝 Clarification response detected from {}", sender_phone));
            

            if let Some(assignment_id) = clarification::extract_assignment_id_from_message(&quoted.text) {
                
                let current_assignment = crud::get_assignment_with_course_by_id(&state.pool, assignment_id)
                    .await
                    .ok()
                    .flatten();

                let missing_fields = if let Some(ref a) = current_assignment {
                    clarification::identify_missing_fields(a)
                } else {
                    Vec::new()
                };

             
                if let Some(ref assignment_obj) = current_assignment {
                    match clarification::parse_clarification_response(
                        &payload.payload.body, 
                        assignment_obj, 
                        &missing_fields,
                        &logger,
                    ).await {
                        Ok(updates) => {
                           
                            let new_deadline = updates.get("deadline")
                                .and_then(|d| crud::parse_deadline(d).ok());
                            let new_title = updates.get("title").cloned();
                            let new_description = updates.get("description").cloned();
                            
                           
                            let new_parallel_codes = updates.get("parallel_codes")
                                .map(|codes_str| {
                                    codes_str.split(',')
                                        .map(|s| s.trim().to_lowercase())
                                        .collect::<Vec<String>>()
                                });

                           
                            let course_id = if let Some(course_name) = updates.get("course_name") {
                                match crud::get_course_by_name(&state.pool, course_name).await {
                                    Ok(Some(course)) => Some(course.id),
                                    Ok(None) => {
                                        let error_msg = format!("❌ Mata kuliah '{}' tidak ditemukan.", course_name);
                                        let _ = send_reply(chat_id, &error_msg).await;
                                        logger.set_status(tui::state::JobStatus::Failed);
                                        return StatusCode::OK;
                                    }
                                    Err(e) => {
                                        logger.log(&format!("❌ Failed to lookup course: {}", e));
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
                                new_title.clone(),
                                new_description.clone(),
                                new_parallel_codes.clone(),
                                Some(payload.payload.id.clone()),
                                Some(payload.payload.body.clone()), 
                                Some(&logger),
                            ).await {
                                Ok(_) => {
                                 
                                    if let Some(cid) = course_id {
                                        let _ = sqlx::query("UPDATE assignments SET course_id = $1 WHERE id = $2")
                                            .bind(cid)
                                            .bind(assignment_id)
                                            .execute(&state.pool)
                                            .await;
                                    }
                                  
                                    if let Ok(Some(full_assignment)) = crud::get_assignment_with_course_by_id(&state.pool, assignment_id).await {
                                        send_academic_update_notification(
                                            chat_id,
                                            assignment_id,  
                                            &full_assignment.title,
                                            &full_assignment.course_name,
                                            &full_assignment.parallel_codes,
                                            new_deadline,
                                            new_parallel_codes.as_deref(),
                                            new_title.as_deref(),
                                            new_description.as_deref(),
                                            &state.pool,  
                                            &logger,
                                        ).await;
                                        
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
                                    logger.set_status(tui::state::JobStatus::Completed);
                                }
                                Err(e) => {
                                    let error_msg = format!("❌ Gagal menyimpan database: {}", e);
                                    let _ = send_reply(chat_id, &error_msg).await;
                                    logger.set_status(tui::state::JobStatus::Failed);
                                }
                            }
                        }
                        Err(err_type) => {
                    
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
                            logger.set_status(tui::state::JobStatus::Failed);
                        }
                    }
                } else {
                    let _ = send_reply(chat_id, "❌ Data tugas tidak ditemukan/sudah dihapus.").await;
                    logger.set_status(tui::state::JobStatus::Failed);
                }

                return StatusCode::OK;
            } else {
                logger.log("⚠️ Could not extract assignment ID from quoted message");
                logger.set_status(tui::state::JobStatus::Failed);
                return StatusCode::OK;
            }
        }
    }
    // ============= END CLARIFICATION =============
    // STEP 2: CHECK WHITELIST
    let (should_process, reason) =
        state.whitelist.should_process(chat_id, is_command);

    if !should_process {
        logger.log(&format!("🚫 Ignoring: {} (from: {})\n", reason, chat_id));
        logger.set_status(tui::state::JobStatus::Completed);
        return StatusCode::OK;
    }

    // STEP 3: HANDLE MESSAGE BASED ON TYPE
    match message_type {
        MessageType::Command(cmd) => {
            logger.log(&format!("⚙️ Processing command: {:?}", cmd));
            let response = handle_command(cmd, sender_phone, sender_name, chat_id, &state.pool, &logger).await;
            
            match response {
                CommandResponse::Text(text) => {
                    if let Err(e) = send_reply(chat_id, &text).await {
                        logger.log(&format!("❌ Failed to send reply: {}", e));
                        logger.set_status(tui::state::JobStatus::Failed);
                    } else {
                        logger.set_status(tui::state::JobStatus::Completed);
                    }
                }
                CommandResponse::ResendMessages { messages, summary } => {
                    
                    for (i, msg_content) in messages.iter().enumerate() {
                        let formatted_msg = format!("*↱* _Forwarded_ \n\n{}", msg_content);
                        
                        if let Err(e) = send_reply(chat_id, &formatted_msg).await {
                            logger.log(&format!("❌ Failed to send message {}: {}", i + 1, e));
                        }
                        
                       
                        if i < messages.len() - 1 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                        }
                    }

                
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                 
                    if let Err(e) = send_reply(chat_id, &summary).await {
                        logger.log(&format!("❌ Failed to send summary: {}", e));
                        logger.set_status(tui::state::JobStatus::Failed);
                    } else {
                        logger.set_status(tui::state::JobStatus::Completed);
                    }
                }
                
                CommandResponse::ProcessWithAI { message, force_mode: _, target_assignment } => {
                  
                    let courses_list = crud::get_all_courses_formatted(&state.pool)
                        .await
                        .unwrap_or_default();
                    
                    let assignments = crud::get_assignments_for_classification(&state.pool, Some(&logger))
                        .await
                        .unwrap_or_default();
                    
                    let course_map: HashMap<uuid::Uuid, String> = sqlx::query_as::<_, (uuid::Uuid, String)>(
                        "SELECT id, name FROM courses"
                    )
                    .fetch_all(&state.pool)
                    .await
                    .map(|rows| rows.into_iter().collect())
                    .unwrap_or_default();
                    
                  
                    let text: String = if let Some(ref target) = target_assignment {
                        let parallel_display = if target.parallel_codes.is_empty() {
                            String::from("N/A")
                        } else {
                            format!("[{}]", target.parallel_codes.join(", "))
                        };
                        
                        let deadline_display = target.deadline
                            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| String::from("N/A"));
                        
                        format!(
                            "UPDATE EXISTING ASSIGNMENT:\n\
                            Course: {}\n\
                            Title: {}\n\
                            Current Deadline: {}\n\
                            Parallels: {}\n\
                            \n\
                            CHANGES: {}",
                            target.course_name,
                            target.title,
                            deadline_display,
                            parallel_display,
                            message
                        )
                    } else {
                        message
                    };
                    
                  
                    match extract_with_ai(
                        text.as_str(),
                        &courses_list,
                        &assignments,
                        &course_map,
                        None,
                        sender_phone,
                        &state.pool,
                        quoted_message_text.as_deref(),
                        quoted_message_id.as_deref(),
                        &logger,
                    ).await {
                        Ok(classification) => {
                          
                            if !matches!(classification, AIClassification::AssignmentUpdate { .. }) {
                                logger.log(&format!("⚠️ AI returned unexpected type: {:?}", classification));
                                
                                let error_msg = "⚠️ *AI TIDAK MENGENALI SEBAGAI UPDATE*\n\n\
                                    Coba gunakan kata kunci seperti:\n\
                                    - 'deadline berubah menjadi...'\n\
                                    - 'diundur ke...'\n\
                                    - 'judul: ...'\n\
                                    - 'untuk parallel: ...'";
                                
                                let _ = send_reply(chat_id, error_msg).await;
                                logger.set_status(tui::state::JobStatus::Failed);
                                return StatusCode::OK;
                            }
                            
                            logger.log(&format!("✅ AI Classification: {:?}\n", classification));
                            
                           
                            if let (Some(target), AIClassification::AssignmentUpdate { 
                                new_deadline, 
                                new_title, 
                                new_description, 
                                parallel_codes, 
                                changes,
                                .. 
                            }) = (target_assignment, classification) {
                                let deadline_parsed = new_deadline.as_ref()
                                    .and_then(|d| crud::parse_deadline(d).ok());

                                logger.log(&format!("🔍 DEBUG: Updating with message_id={}, body={}", 
                                    payload.payload.id, 
                                    payload.payload.body
                                ));
                                
                                match crud::update_assignment_fields(
                                    &state.pool,
                                    target.id,
                                    deadline_parsed,
                                    new_title.clone(),
                                    new_description.clone(),
                                    if parallel_codes.is_empty() { None } else { Some(parallel_codes.clone()) },
                                    Some(payload.payload.id.clone()),
                                    Some(payload.payload.body.clone()),
                                    Some(&logger),
                                ).await {
                                    Ok(_) => {
                                    
                                        if deadline_parsed.is_some() || !parallel_codes.is_empty() || new_title.is_some() || new_description.is_some() {
                                            send_academic_update_notification(
                                                chat_id,
                                                target.id,  
                                                &target.title,
                                                &target.course_name,
                                                &target.parallel_codes,
                                                deadline_parsed,
                                                Some(&parallel_codes),
                                                new_title.as_deref(),
                                                new_description.as_deref(),
                                                &state.pool,  
                                                &logger,
                                            ).await;
                                        }
                                        
                                        let success_msg = format!(
                                            "✅ *UPDATE BERHASIL*\n\n\
                                            📝 *{}*\n\
                                            📚 {}\n\
                                            \n\
                                            🔄 Changes: {}",
                                            target.title,
                                            target.course_name,
                                            changes
                                        );
                                        
                                        let _ = send_reply(chat_id, &success_msg).await;
                                        logger.set_status(tui::state::JobStatus::Completed);
                                    }
                                    Err(e) => {
                                        logger.log(&format!("❌ Update failed: {}", e));
                                        let _ = send_reply(chat_id, &format!("❌ Gagal update: {}", e)).await;
                                        logger.set_status(tui::state::JobStatus::Failed);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            logger.log(&format!("❌ AI extraction failed: {}", e));
                            let _ = send_reply(chat_id, "❌ AI gagal memproses pesan. Coba lagi.").await;
                            logger.set_status(tui::state::JobStatus::Failed);
                        }
                    }
                }
            }
        }

        MessageType::NeedsAI(text) => {
            logger.log(">> Processing with AI...");
            
          
            let image_base64 = if payload.payload.has_media.unwrap_or(false) {
                if let Some(ref media) = payload.payload.media {
                    if media.mimetype.as_ref().map(|m| m.starts_with("image/")).unwrap_or(false) {
                      
                        if let Some(ref url) = media.url {
                            match download_media_from_url(url, &logger).await {
                                Ok(base64) => Some(base64),
                                Err(e) => {
                                    logger.log(&format!("❌ Failed to download image: {}", e));
                                    None
                                }
                            }
                        } else {
                            logger.log("⚠️  No media URL provided by WAHA");
                            None
                        }
                    } else { None }
                } else { None }
            } else { None };
            
          
            let courses_list = crud::get_all_courses_formatted(&state.pool).await.unwrap_or_default();
            
            let assignments = crud::get_assignments_for_classification(&state.pool, Some(&logger)).await.unwrap_or_default();
            
            let course_map = sqlx::query_as::<_, (uuid::Uuid, String)>("SELECT id, name FROM courses")
                .fetch_all(&state.pool).await.map(|rows| rows.into_iter().collect()).unwrap_or_default();
            
            let ai_start = Instant::now();
            
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
                &logger, 
            ).await {
                Ok(classification) => {
                  
                    let ai_duration = ai_start.elapsed();
                    logger.log(&format!("🧠 AI Latency: {:.2?}", ai_duration));

                    logger.log(&format!("✅ AI Classification: {:?}\n", classification));
                    
                    handle_ai_classification(
                        state.pool.clone(), 
                        classification, 
                        &payload.payload.id, 
                        sender_phone, 
                        &payload.payload.body,
                        chat_id,
                        debug_group_id,
                        logger.clone(),
                    ).await;
                }
                Err(e) => {
                    logger.log(&format!("❌ AI extraction failed: {}", e));
                    let _ = send_reply(chat_id, "❌ Failed to process message").await;
                    logger.set_status(tui::state::JobStatus::Failed);
                }
            }
        }
    }
    
   
    let total_duration = request_start.elapsed();
    logger.log(&format!("⏱️ Total Request Processed in: {:.2?}\n", total_duration));

    StatusCode::OK
}

#[allow(non_snake_case)]
async fn handle_ai_classification(
    pool: PgPool,
    classification: AIClassification, 
    message_id: &str,
    sender_id: &str,
    message_body: &str,
    source_chat_id: &str, 
    debug_group_id: Option<String>,
    logger: tui::JobLogger,
) {
    let message_id = message_id.to_string();
    let sender_id = sender_id.to_string();
    let message_body = message_body.to_string();
    let source_chat_id = source_chat_id.to_string(); 

    match classification {
     
        AIClassification::MultipleAssignments { assignments, .. } => {
         
            if let Some(tui_state) = TUI_STATE.get() {
                tui_state.add_job_tag(logger.job_id().to_string(), "#batch".to_string()).await;
            }
            
            let debug_group = debug_group_id.clone();
            
            if let Some(debug_id) = &debug_group {
                let _ = send_reply(debug_id, &format!("📦 Processing {} assignments...", assignments.len())).await;
            }
            
            let mut unique_assignments = Vec::new();
            let mut seen = std::collections::HashSet::new();
            
            for assignment in &assignments {
               
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
                    &message_body,
                    debug_group_id.clone(),
                    index + 1,
                    logger.clone(),
                ).await;
            }
            
            logger.set_status(tui::state::JobStatus::Completed);
        }
        
        // Single assignment - USE AI FOR DUPLICATE DETECTION
        AIClassification::AssignmentInfo { course_name, title, deadline, description, parallel_codes, .. } => {
           
            if let Some(tui_state) = TUI_STATE.get() {
                tui_state.add_job_tag(logger.job_id().to_string(), "#assignment".to_string()).await;
            }
            
            let debug_group = debug_group_id.clone();
            let msg_body = message_body.to_string();
            let logger_clone = logger.clone();
            
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
                    logger_clone.clone(),
                ).await;
                
                logger_clone.set_status(tui::state::JobStatus::Completed);
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
            
            if let Some(tui_state) = TUI_STATE.get() {
                tui_state.add_job_tag(logger.job_id().to_string(), "#update".to_string()).await;
            }
            
            let pool_clone = pool.clone();
            let msg_id = message_id.clone();
            let msg_body = message_body.clone();
            let debug_clone = debug_group_id.clone();
            let source_chat_clone = source_chat_id.clone();
            let logger_clone = logger.clone();

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
                
                let active_assignments = crud::get_recent_assignments_for_matching(&pool_clone, Some(&logger_clone))
                    .await
                    .unwrap_or_default();
                

                // LOGIC 1: RE-ANNOUNCEMENT CHECK
                if let Some(ref title) = new_title {
                    if let (Some(_course_id), Some(cname)) = (course_id, &course_name) {
                        let dup_check = check_duplicate_assignment(
                            title,
                            new_description.as_deref().unwrap_or(""),
                            cname,
                            &parallel_codes,
                            &active_assignments,
                            &course_map,
                            &logger_clone,
                        ).await;
                        
                        if let Ok(Some((id, reason))) = dup_check { 
                            logger_clone.log(&format!("🔄 \x1b[33mRe-announcement\x1b[0m: \x1b[1m{}\x1b[0m", title));
                            
                            let deadline_parsed = new_deadline.as_ref()
                                .and_then(|d| crud::parse_deadline(d).ok());
                            
                            match crud::update_assignment_fields(
                                &pool_clone,
                                id,
                                deadline_parsed,
                                None,
                                new_description.clone(),
                                if parallel_codes.is_empty() { None } else { Some(parallel_codes.clone()) },
                                Some(msg_id.clone()),
                                Some(msg_body.clone()),
                                Some(&logger_clone),
                            ).await {
                                Ok(_) => {
                                    if deadline_parsed.is_some() || !parallel_codes.is_empty() {
                                        send_academic_update_notification(
                                            &source_chat_clone,
                                            id,  
                                            title,
                                            cname,
                                            &parallel_codes,
                                            deadline_parsed,
                                            if parallel_codes.is_empty() { None } else { Some(&parallel_codes) },
                                            None,
                                            new_description.as_deref(),
                                            &pool_clone,  
                                            &logger,
                                        ).await;
                                    }

                                    if let Some(debug_id) = debug_clone {
                                        let _reason_display = if !reason.is_empty() {
                                            format!("\n_{}_", reason)
                                        } else {
                                            String::new()
                                        };
                                        
                                        let _ = send_reply(
                                            &debug_id,
                                            &format!("🔄 *UPDATED*: {}\n_{}_",  title, changes)
                                        ).await;
                                    }
                                }
                                Err(e) => {
                                    logger_clone.log(&format!("❌ Update failed: {}", e));
                                }
                            }
                            
                            logger_clone.set_status(tui::state::JobStatus::Completed);
                            return;
                        }
                    }
                }
                
                // LOGIC 2: REGULAR UPDATE MATCHING
                match parser::ai_extractor::match_update_to_assignment(
                    &changes,
                    &reference_keywords,
                    &active_assignments,
                    &course_map,
                    &parallel_codes,
                    &logger_clone,
                ).await {
                    Ok(Some((assignment_id, reason))) => {
                        let current_title = active_assignments.iter()
                            .find(|a| a.id == assignment_id)
                            .map(|a| a.title.clone())
                            .unwrap_or_else(|| "Unknown".to_string());
                        
                        logger_clone.log(&format!("🔄 \x1b[33mUpdating\x1b[0m: \x1b[1m{}\x1b[0m", current_title));
                        
                        let deadline_parsed = new_deadline.as_ref()
                            .and_then(|d| crud::parse_deadline(d).ok());
                        
                        if let Ok(updated_assign) = crud::update_assignment_fields(
                            &pool_clone,
                            assignment_id,
                            deadline_parsed,
                            new_title.clone(),
                            new_description.clone(),
                            if parallel_codes.is_empty() { None } else { Some(parallel_codes.clone()) },
                            Some(msg_id),
                            Some(msg_body),
                            Some(&logger_clone),
                        ).await {
                            if deadline_parsed.is_some() || !parallel_codes.is_empty() || new_title.is_some() || new_description.is_some() {
                                let course_name_for_alert = if let Some(cid) = updated_assign.course_id {
                                    course_map.get(&cid)
                                        .cloned()
                                        .unwrap_or_else(|| "General".to_string())
                                } else {
                                    course_name.unwrap_or_else(|| "General".to_string())
                                };
                                
                                send_academic_update_notification(
                                    &source_chat_clone,
                                    assignment_id,  
                                    &updated_assign.title,
                                    &course_name_for_alert,
                                    &updated_assign.parallel_codes,
                                    deadline_parsed,
                                    if parallel_codes.is_empty() { None } else { Some(&parallel_codes) },
                                    new_title.as_deref(),
                                    new_description.as_deref(),
                                    &pool_clone,  
                                    &logger,
                                ).await;
                            }
                            
                            if let Some(debug_id) = debug_clone {
                                let _reason_display = if !reason.is_empty() {
                                    format!("\n_{}_", reason)
                                } else {
                                    String::new()
                                };
                                
                                let _ = send_reply(
                                    &debug_id,
                                    &format!("🔄 *UPDATED*: {}\n_{}_",
                                        current_title, 
                                        changes,
                                    )
                                ).await;
                            }
                        }
                        
                        logger_clone.set_status(tui::state::JobStatus::Completed);
                    }
                    Ok(None) => {
                        logger_clone.log(&format!("⚠️  \x1b[33mNo match found\x1b[0m for update: {:?}", reference_keywords));
                        
                        if let Some(debug_id) = debug_clone {
                            let _ = send_reply(
                                &debug_id,
                                "⚠️ Could not find assignment to update"
                            ).await;
                        }
                        
                        logger_clone.set_status(tui::state::JobStatus::Failed);
                    }
                    Err(e) => {
                        logger_clone.log(&format!("❌ Update matching failed: {}", e));
                        logger_clone.set_status(tui::state::JobStatus::Failed);
                    }
                }
            });
        }
        
        AIClassification::Unrecognized { reason, category } => {
            if let Some(tui_state) = TUI_STATE.get() {
                let tag = match category {
                    UnrecognizedCategory::Informal => "#informal",
                    UnrecognizedCategory::AcademicRelated => "#academic-related",
                };
                tui_state.add_job_tag(logger.job_id().to_string(), tag.to_string()).await;
            }
            
            match category {
                UnrecognizedCategory::Informal => {
                    logger.log("💬 Informal chat detected - ignoring");
                }
                UnrecognizedCategory::AcademicRelated => {
                    if let Some(debug_id) = debug_group_id {
                        let message = reason
                            .as_ref()
                            .map(|r| format!("_{}_", r))
                            .unwrap_or_else(|| "_Academic-related but not an assignment_".to_string());
                        
                        let _ = send_reply(&debug_id, &message).await;
                    }
                }
            }
            
            logger.set_status(tui::state::JobStatus::Completed);
        }
    }
}

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
    logger: tui::JobLogger,
) {
    let title_clone = title.clone();
    let desc_clone = description.clone().unwrap_or("No description".to_string());
    let deadline_parsed = deadline.as_ref()
        .and_then(|d| crud::parse_deadline(d).ok());
    let parallel_code_parsed = extract_parallel_code(&title);


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
    
   
    if let Some(_cid) = course_id {
        if let Some(cname) = &course_name {
            let course_map: HashMap<uuid::Uuid, String> = sqlx::query_as::<_, (uuid::Uuid, String)>(
                "SELECT id, name FROM courses"
            )
            .fetch_all(&pool)
            .await
            .map(|r| r.into_iter().collect())
            .unwrap_or_default();
            
            let existing_assignments = crud::get_recent_assignments_for_duplicate_check(&pool, Some(&logger))
                .await
                .unwrap_or_default();
            
            if !existing_assignments.is_empty() {
        
                let total_count = existing_assignments.len();
                
        
                let mut strict_candidates: Vec<Assignment> = Vec::new();
                
                for assignment in existing_assignments {
                 
                    let same_course = assignment.course_id
                        .and_then(|id| course_map.get(&id))
                        .map(|name| name.eq_ignore_ascii_case(cname))
                        .unwrap_or(false);
                    
                    if !same_course {
                        continue;
                    }
                
                    if is_parallel_superset_or_equal(&final_parallel_codes[..], &assignment.parallel_codes[..]) {
                        strict_candidates.push(assignment);
                    }
                }
                
                if !strict_candidates.is_empty() {
                    logger.log(&format!(
                        "│ 🔍 Strict parallel filtering: {} → {} candidates",
                        total_count,
                        strict_candidates.len()
                    ));
                    
                    let match_result = check_duplicate_assignment(
                        &title_clone,
                        &desc_clone,
                        cname,
                        final_parallel_codes.as_slice(),
                        &strict_candidates,
                        &course_map,
                        &logger, 
                    ).await;
                    
                    match &match_result {
                        Ok(Some((id, reason))) => {
                            logger.log(&format!("🔄 \x1b[33mUpdating\x1b[0m: \x1b[1m{}\x1b[0m", title_clone));
                            
                            let update_result = crud::update_assignment_fields(
                                &pool, 
                                *id, 
                                deadline_parsed, 
                                None,
                                Some(desc_clone.clone()), 
                                if final_parallel_codes.is_empty() { None } else { Some(final_parallel_codes.clone()) },
                                Some(message_id.to_string()),
                                Some(message_body.to_string()),
                                Some(&logger),
                            ).await;
                            
                            if update_result.is_ok() {
                                if let Some(debug_id) = &debug_group_id {
                                    let prefix = if assignment_number > 0 {
                                        format!("{}. ", assignment_number)
                                    } else {
                                        String::new()
                                    };
                                    
                                    let reason_display = if !reason.is_empty() {
                                        format!("\n_{}_", reason)
                                    } else {
                                        String::new()
                                    };
                                    
                                    let _ = send_reply(
                                        debug_id, 
                                        &format!("{}🔄 *UPDATED*: {}{}", prefix, title_clone, reason_display)
                                    ).await;
                                }
                            }
                            return;
                        }
                        Ok(None) => {
                            logger.log("│ ✅ Not a duplicate (passed strict filtering)");
                        }
                        Err(_) => {
                            logger.log("│ ⚠️ Duplicate check failed, treating as new");
                        }
                    }
                } else {
                    logger.log(&format!(
                        "│ ⏭️  Skipped duplicate check: No candidates after strict parallel filtering (found {} total)",
                        total_count
                    ));
                }
            }
        }
    }
    


    let new_assignment = NewAssignment {
        course_id, 
        title: title_clone.clone(), 
        description: desc_clone.clone(),
        deadline: deadline_parsed, 
        parallel_codes: final_parallel_codes.clone(),
        sender_id: Some(sender_id.to_string()), 
        message_id: message_id.to_string(),
        relating_messages: vec![message_body.to_string()],
    };
    
    match crud::create_assignment(&pool, new_assignment, Some(&logger)).await {
        Ok(_) => {
            logger.log(&format!("✅ Assignment created: {}", title_clone));
            
            // ============ CLARIFICATION CHECK ============
            if let Some(cid) = course_id {
                if let Ok(Some(assignment)) = crud::get_assignment_by_title_and_course(&pool, &title_clone, cid).await {
                    if let Ok(Some(full_assign)) = crud::get_assignment_with_course_by_id(&pool, assignment.id).await {
                        
                    
                        logger.log("🔍 Starting clarification check...");
                        logger.log(&format!("   \x1b[36m📚 {}\x1b[0m", full_assign.course_name));
                        logger.log(&format!("   \x1b[1m📝 {}\x1b[0m", full_assign.title));
                        
                        if let Some(ref desc) = full_assign.description {
                            let desc_display = desc.chars().take(60).collect::<String>();
                            let desc_final = if desc.len() > 60 {
                                format!("{}...", desc_display)
                            } else {
                                desc_display
                            };
                            logger.log(&format!("   \x1b[90m📄 {}\x1b[0m", desc_final));
                        }
                        
                        if let Some(deadline) = full_assign.deadline {
                            let indonesia_time = deadline + ChronoDuration::hours(7);
                            logger.log(&format!("   \x1b[32m⏰ {}\x1b[0m", indonesia_time.format("%Y-%m-%d %H:%M WIB")));
                        } else {
                            logger.log("   \x1b[33m⏰ (no deadline)\x1b[0m");
                        }
                        
                        if !full_assign.parallel_codes.is_empty() {
                            logger.log(&format!("   \x1b[35m🧩 {}\x1b[0m", full_assign.parallel_codes.join(", ").to_uppercase()));
                        }
                        
                   
                        let missing = clarification::identify_missing_fields(&full_assign);
                        
                        if !missing.is_empty() {
                            logger.log(&format!("   \x1b[33m🔔 Missing: {}\x1b[0m", missing.join(", ")));
                            
                            if let Some(debug_id) = &debug_group_id {
                                let (info_msg, template_msg) = clarification::generate_clarification_messages(&full_assign, &missing);
                                let combined_msg = format!("{}\n{}", info_msg, template_msg);

                                match send_reply(debug_id, &combined_msg).await {
                                    Ok(_) => logger.log("   \x1b[32m✅ Clarification sent\x1b[0m\n"),
                                    Err(e) => logger.log(&format!("   \x1b[31m❌ Send failed: {}\x1b[0m\n", e)),
                                }
                            }
                            return; 
                        } else {
                            logger.log("   \x1b[32m✅ Complete (no clarification needed)\x1b[0m\n");
                        }
                    }
                }
            }
            // ============ END CLARIFICATION ============
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
                
                logger.log("📤 Sending success message...");
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
            logger.log(&format!("❌ Failed to save assignment: {}", e));
            
            if let Some(debug_id) = &debug_group_id {
                let _ = send_reply(
                    debug_id, 
                    &format!("⚠️ Failed to save assignment: {}", title_clone)
                ).await;
            }
        }
    }
}


// FITUR : FUNGSI HELPER DENGAN KIRIM REPLY DENGAN ID PESAN
async fn send_reply_with_id(chat_id: &str, text: &str, reply_to: Option<String>) -> Result<(), String> {
    let waha_url = format!("{}/api/sendText", std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()));
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = SendTextRequest { 
        chat_id: chat_id.to_string(), 
        text: text.to_string(), 
        session: "default".to_string(),
        reply_to: reply_to 
    };
    
    let client = reqwest::Client::new();
    let res = client.post(waha_url)
        .header("X-Api-Key", api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
        
    if res.status().is_success() { Ok(()) } else { Err(format!("API Error")) }
}

async fn send_reply(chat_id: &str, text: &str) -> Result<(), String> {
    send_reply_with_id(chat_id, text, None).await
}


#[allow(non_snake_case)]
async fn send_academic_update_notification(
    source_chat: &str,
    assignment_id: uuid::Uuid,
    title: &str,
    _course_name: &str,
    assignment_parallels: &[String],
    deadline: Option<chrono::DateTime<chrono::Utc>>,
    parallel_codes: Option<&[String]>,
    new_title: Option<&str>,
    new_description: Option<&str>,
    pool: &PgPool,
    logger: &tui::JobLogger,
) {
    let academic_env = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
    let channels: Vec<&str> = academic_env.split(',').map(|s| s.trim()).collect();
    
    let mut updated_fields = Vec::new();
    
    if let Some(d) = deadline {
        let wib = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
        let deadline_wib = d.with_timezone(&wib);
        updated_fields.push(format!("deadline diubah jadi {}", deadline_wib.format("%d %b %Y, %H:%M WIB")));
    }
    
    if let Some(codes) = parallel_codes {
        if !codes.is_empty() {
            let codes_display = codes.iter()
                .map(|c| c.to_uppercase())
                .collect::<Vec<_>>()
                .join(", ");
            updated_fields.push(format!("sekarang untuk [{}]", codes_display));
        }
    }
    
    if let Some(t) = new_title {
        updated_fields.push(format!("judul diubah jadi \"{}\"", t));
    }
    
    if let Some(d) = new_description {
        let desc_preview = if d.len() > 50 {
            format!("{}...", &d[..50])
        } else {
            d.to_string()
        };
        updated_fields.push(format!("deskripsi: {}", desc_preview));
    }
    
    if updated_fields.is_empty() {
        return;
    }
    
    let fields_text = if updated_fields.len() == 1 {
        updated_fields[0].clone()
    } else if updated_fields.len() == 2 {
        format!("{} dan {}", updated_fields[0], updated_fields[1])
    } else {
        let last = updated_fields.last().unwrap();
        let rest = &updated_fields[..updated_fields.len() - 1];
        format!("{}, dan {}", rest.join(", "), last)
    };
    
   
    let parallel_display = if assignment_parallels.is_empty() {
        String::new()
    } else {
        format!(" [{}]", assignment_parallels.iter()
            .map(|c| c.to_uppercase())
            .collect::<Vec<_>>()
            .join(", "))
    };
    
    
    let assignment_data = sqlx::query_as::<_, (Vec<String>, Vec<String>)>(
        "SELECT message_ids, relating_messages FROM assignments WHERE id = $1"
    )
    .bind(assignment_id)
    .fetch_optional(pool)
    .await;
    
    let (original_message_id, original_message_body) = match assignment_data {
        Ok(Some((message_ids, relating_messages))) => {
            logger.log(&format!("📤 Quote data: {} msg_ids, {} bodies", message_ids.len(), relating_messages.len()));
            (message_ids.first().cloned(), relating_messages.first().cloned())
        }
        Ok(None) => {
            logger.log(&format!("⚠️ Assignment {} not found", assignment_id));
            (None, None)
        }
        Err(e) => {
            logger.log(&format!("❌ Query failed: {}", e));
            (None, None)
        }
    };
    
    for channel_id in channels {
        if channel_id.is_empty() || channel_id == source_chat {
            continue;
        }
        
        let msg = format!(
            "*[Update] {}{}*\n\
            _{}_",
            title,
            parallel_display,
            fields_text
        );
        
        if let Some(ref msg_id) = original_message_id {
            if check_message_exists(channel_id, msg_id).await {
                if send_reply_with_id(channel_id, &msg, Some(msg_id.clone())).await.is_ok() {
                    logger.log("✅ Native reply sent");
                    continue;
                }
            }
            logger.log("⚠️ Original message not found, using manual quote");
        }
        
        let quoted_text = original_message_body
            .as_ref()
            .map(|body| format_quote_fallback(body))
            .unwrap_or_else(|| "> Unable to quote!".to_string());
        
        let fallback_msg = format!("{}\n\n{}", quoted_text, msg);
        let _ = send_reply(channel_id, &fallback_msg).await;
    }
}


fn format_quote_fallback(message: &str) -> String {

    let cleaned = message
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    
    // Truncate to 100 characters
    let truncated = if cleaned.len() > 100 {
        format!("{}...", &cleaned[..100])
    } else {
        cleaned
    };

    format!("> {}", truncated)
}

async fn check_message_exists(chat_id: &str, message_id: &str) -> bool {
    let waha_url = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string());
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
    let url = format!(
        "{}/api/default/chats/{}/messages/{}",
        waha_url,
        chat_id,
        message_id
    );
    
    let client = reqwest::Client::new();
    match client.get(&url)
        .header("X-Api-Key", api_key)
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(_) => false
    }
}

fn extract_parallel_code(title: &str) -> Option<String> {
    let u = title.to_uppercase();
    if u.contains("ALL") { return Some("all".into()); }
    ["K1", "K2", "K3", "P1", "P2", "P3"].iter().find(|&c| u.contains(c)).map(|c| c.to_lowercase())
}

fn is_parallel_superset_or_equal(new_codes: &[String], existing_codes: &[String]) -> bool {

    if new_codes.iter().any(|c| c.eq_ignore_ascii_case("all")) {
        return true;
    }
    if existing_codes.iter().any(|c| c.eq_ignore_ascii_case("all")) {
        return true;
    }
    

    if new_codes.is_empty() && existing_codes.is_empty() {
        return true;
    }
    

    if new_codes.is_empty() || existing_codes.is_empty() {
        return false;
    }
    
    existing_codes.iter().all(|existing| 
        new_codes.iter().any(|new| new.eq_ignore_ascii_case(existing))
    )
}



async fn download_media_from_url(media_url: &str, logger: &tui::JobLogger) -> Result<String, String> {
    logger.log(&format!("   📥 Original URL: {}", media_url));
    

    let fixed_url = if media_url.contains("localhost:3000") {
   
        let waha_hosts = vec![
            std::env::var("WAHA_URL").ok(),
            Some("http://waha:3000".to_string()),
            Some("http://marbot_waha:3000".to_string()),
        ];
                let base_url = waha_hosts.into_iter()
            .flatten()
            .next()
            .unwrap_or_else(|| "http://waha:3000".to_string());
        
        // Extract the path from original URL
        // From: http://localhost:3000/api/files/default/XXX.jpeg
        // To:   http://waha:3000/api/files/default/XXX.jpeg
        media_url.replace("http://localhost:3000", &base_url)
    } else {
        media_url.to_string()
    };
    
    logger.log(&format!("   📥 Downloading from: {}", fixed_url));
    

    let api_key = std::env::var("WAHA_API_KEY")
        .unwrap_or_else(|_| "devkey123".to_string());
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Client error: {}", e))?;
    
    let res = client
        .get(&fixed_url)  
        .header("X-Api-Key", api_key)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;
    
    if !res.status().is_success() {
        return Err(format!("HTTP {}: {}", res.status(), fixed_url));
    }
    
    logger.log("   ✅ Download successful");
    
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    logger.log(&format!("   📦 Size: {:.2} MB", bytes.len() as f64 / 1_000_000.0));
    
    use base64::{Engine as _, engine::general_purpose};
    use image::io::Reader as ImageReader;
    use std::io::Cursor;


    if (bytes.len() as f64 / 1_000_000.0) > 3.5 {
        logger.log("   🔄 Compressing image...");
        
        let img = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| format!("Format error: {}", e))?
            .decode()
            .map_err(|e| format!("Decode error: {}", e))?;
        
        let img = img.thumbnail(2048, 2048);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageOutputFormat::Jpeg(80))
            .map_err(|e| format!("Compress error: {}", e))?;
            
        logger.log(&format!("   ✅ Compressed: {:.2} MB", buf.len() as f64 / 1_000_000.0));
        return Ok(general_purpose::STANDARD.encode(&buf));
    } else {
        return Ok(general_purpose::STANDARD.encode(&bytes));
    }
}