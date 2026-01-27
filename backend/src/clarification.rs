use crate::models::AssignmentWithCourse;
use crate::parser::ai_extractor::schedule_oracle::ScheduleOracle;
use crate::parser::ai_extractor::{
    GROQ_REASONING_MODELS, 
    GROQ_TEXT_MODELS, 
    GEMINI_MODELS,
    GeminiResponse,
    GroqResponse,
    extract_ai_text,
    extract_groq_text,
};
use crate::tui::JobLogger;
use uuid::Uuid;
use std::collections::HashMap;
use std::io::{Write, stdout};
use chrono::{Local, NaiveDateTime, NaiveDate, NaiveTime, Duration, Datelike, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::json;
use regex::Regex;
use once_cell::sync::Lazy;
use std::sync::Mutex;

static PRINT_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

const MAX_RETRIES: u32 = 4;

// ===== HELPER FUNCTIONS =====

pub fn identify_missing_fields(assignment: &AssignmentWithCourse) -> Vec<String> {
    let mut missing = Vec::new();
    
    if assignment.course_name.is_empty() || assignment.course_name == "Unknown Course" {
        missing.push("course_name".to_string());
    }
    
    let title_lower = assignment.title.to_lowercase();
    let is_generic_title = assignment.title.is_empty() || 
        title_lower.contains("tugas baru") ||
        title_lower == "assignment" ||
        title_lower == "tugas" ||
        title_lower == "pr" ||
        title_lower.len() < 3;
    
    if is_generic_title {
        missing.push("title".to_string());
    }
    
    if assignment.deadline_is_missing() {
        missing.push("deadline".to_string());
    }
    
    if assignment.parallel_codes.is_empty() {
        missing.push("parallel_codes".to_string());
    }
    
    if let Some(ref desc) = assignment.description {
        if desc.trim().is_empty() || desc.len() < 5 {
             missing.push("description".to_string());
        }
    } else {
        missing.push("description".to_string());
    }
    
    missing
}

pub fn generate_clarification_messages(
    assignment: &AssignmentWithCourse,
    missing_fields: &[String]
) -> (String, String) {
    let field_list = missing_fields.iter().map(|f| match f.as_str() {
        "course_name" => "📚 Nama Mata Kuliah",
        "title" => "📝 Judul Tugas",
        "deadline" => "⏰ Deadline",
        "parallel_codes" => "🧩 Kode Paralel",
        "description" => "📄 Deskripsi",
        _ => "❓ Unknown"
    }).collect::<Vec<_>>().join("\n");
    
    let desc_preview = assignment.description
        .as_ref()
        .map(|d| format!("📄 {}", d))
        .unwrap_or_else(|| "📄 (belum ada deskripsi)".to_string());
    
    let deadline_display = if let Some(d) = assignment.deadline {
        let wib = d + Duration::hours(7);
        wib.format("%Y-%m-%d %H:%M").to_string()
    } else {
        "N/A".to_string()
    };

    let parallel_display = if assignment.parallel_codes.is_empty() {
        "N/A".to_string()
    } else {
        assignment.format_parallel_display()
    };

    let sender_tag = assignment.sender_id.as_ref()
        .map(|id| {
            let num = id.split('@').next().unwrap_or(id);
            format!("@{}", num)
        })
        .unwrap_or_default();
    
    let info_message = format!(
        "*[PERLU KLARIFIKASI]* {}\n\
        `ID: {}`\n\
        \n\
        📌 *{}* - {}\n\
        {}\n\
        ⏰ Deadline: {}\n\
        🧩 Parallel: {}\n\
        \n\
        *[INFO KURANG]:*\n\
        {}",
        sender_tag, 
        assignment.id, 
        assignment.course_name,
        assignment.title,
        desc_preview,
        deadline_display,
        parallel_display,
        field_list,
    );
    
   let template_message = "\
        \n_(Reply pesan ini langsung dengan info tambahannya, misalnya: 'dikumpulin besok jam 8 pagi')_\n\
        \n\
        💡 _Jika AI salah paham, gunakan format manual:_\n\
        `Deadline: ...`\n\
        `Desc: ...`".to_string();

    (info_message, template_message)
}


pub fn extract_assignment_id_from_message(text: &str) -> Option<Uuid> {
    let cleaned_text = text.replace('`', "");
    for line in cleaned_text.lines() {
        if line.to_lowercase().contains("id:") {
            if let Some(id_part) = line.split(':').nth(1) {
                if let Ok(uuid) = Uuid::parse_str(id_part.trim()) {
                    return Some(uuid);
                }
            }
        }
    }
    None
}

pub fn generate_cancellation_message(assignment_id: Uuid) -> String {
    format!("❌ *KLARIFIKASI DIBATALKAN*\nTugas ID `{}` dibuang.", assignment_id)
}

pub fn generate_parse_failed_message() -> String {
    "⚠️ *MAAF, TIDAK PAHAM*\nAku bingung dengan format pesanmu. Coba gunakan bahasa yang lebih sederhana atau format `Key: Value`.".to_string()
}

pub fn generate_no_date_message() -> String {
    "⚠️ *TANGGAL TIDAK DITEMUKAN*\nKamu menyebutkan jam, tapi aku tidak tahu untuk tanggal berapa.".to_string()
}

// ===== Unified logging helpers =====

fn logger_log(logger: Option<&JobLogger>, msg: &str) {
    if let Some(l) = logger {
        l.log(msg);
    } else {
        println!("{}", msg);
        let _ = stdout().flush(); 
    }
}

fn logger_log_countdown(logger: Option<&JobLogger>, attempt: u32, remaining: u64) {
    if let Some(l) = logger {
        l.log_countdown(attempt, remaining);
    } else {
        let _guard = PRINT_LOCK.lock().unwrap();
        print!("\x1b[1A\x1b[2K│ \x1b[33m⏳ RETRY #{}\x1b[0m - Waiting \x1b[36m{}\x1b[0m seconds...\n", attempt, remaining);
        let _ = stdout().flush();
    }
}

async fn retry_with_countdown(attempt: u32, logger: Option<&JobLogger>) {
    let delay = 10 * attempt as u64;

    if logger.is_some() {
        logger_log(logger, "│");
        logger_log(logger, &format!("│ \x1b[33m⏳ RETRY #{}\x1b[0m - Waiting \x1b[36m{}\x1b[0m seconds...", attempt, delay));

        for remaining in (1..=delay).rev() {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            logger_log_countdown(logger, attempt, remaining);
        }

        logger_log(logger, "│");
    } else {
        println!("│");
        println!("│ \x1b[33m⏳ RETRY #{}\x1b[0m - Waiting \x1b[36m{}\x1b[0m seconds...", attempt, delay);
        let _ = stdout().flush();

        for remaining in (1..=delay).rev().skip(1) {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            {
                let _guard = PRINT_LOCK.lock().unwrap();
                print!("\x1b[1A\x1b[2K│ \x1b[33m⏳ RETRY #{}\x1b[0m - Waiting \x1b[36m{}\x1b[0m seconds...\n", 
                       attempt, remaining);
                let _ = stdout().flush();
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        {
            let _guard = PRINT_LOCK.lock().unwrap();
            print!("\x1b[1A\x1b[2K");
            let _ = stdout().flush();
        }

        println!("│");
        let _ = stdout().flush();
    }
}

fn log_trying_line_tui(model: &str, index: usize, total: usize, logger: &JobLogger) {
    let formatted = format!("│ \x1b[34m🔄 TRYING\x1b[0m : \x1b[35m{}\x1b[0m (\x1b[36m{}\x1b[0m / \x1b[36m{}\x1b[0m)", model, index, total);
    logger.log(&formatted);
}

fn print_trying_line_stdout(model: &str, index: usize, total: usize, last_trying: &mut bool) {
    let formatted = format!("\x1b[34m🔄 TRYING\x1b[0m : \x1b[35m{}\x1b[0m (\x1b[36m{}\x1b[0m / \x1b[36m{}\x1b[0m)", model, index, total);

    let _guard = PRINT_LOCK.lock().unwrap();

    if *last_trying {
        print!("\x1b[1A\x1b[2K│ {}\n", formatted);
    } else {
        println!("│ {}", formatted);
        *last_trying = true;
    }
    let _ = stdout().flush();
}

fn clear_previous_trying_stdout(last_trying: &mut bool) {
    if *last_trying {
        let _guard = PRINT_LOCK.lock().unwrap();
        print!("\x1b[1A\x1b[2K");
        let _ = stdout().flush();
        *last_trying = false;
    }
}

// ===== AI PARSING WITH RETRY LOGIC =====

#[derive(Debug, Serialize, Deserialize)]
pub struct AIClarificationResult {
    pub deadline: Option<String>,
    pub deadline_time: Option<String>,
    pub course_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub parallel_codes: Option<Vec<String>>,
    pub is_cancellation: bool,
}

pub async fn parse_clarification_response(
    text: &str, 
    assignment: &AssignmentWithCourse,
    missing_fields: &[String],
    logger: &JobLogger,
) -> Result<HashMap<String, String>, String> {
    parse_clarification_response_internal(text, assignment, missing_fields, Some(logger)).await
}

pub async fn parse_clarification_response_stdout(
    text: &str, 
    assignment: &AssignmentWithCourse,
    missing_fields: &[String]
) -> Result<HashMap<String, String>, String> {
    parse_clarification_response_internal(text, assignment, missing_fields, None).await
}

async fn parse_clarification_response_internal(
    text: &str, 
    assignment: &AssignmentWithCourse,
    missing_fields: &[String],
    logger: Option<&JobLogger>,
) -> Result<HashMap<String, String>, String> {
    let current_deadline = assignment.deadline.map(|d| d.naive_utc());
    let next_meeting_hint = resolve_next_meeting(assignment);

    logger_log(logger, "┌──[CLARIFICATION PARSING]──────────────────");
    
    let message_display = text
        .replace('\n', "\\n")
        .chars()
        .take(60)
        .collect::<String>();
    
    logger_log(logger, &format!("│ 📝 Message\t: \"{}...\"", message_display));
    logger_log(logger, &format!("│ 🔍 Missing\t: {:?}", missing_fields));
    
    if let Some(hint) = next_meeting_hint {
        logger_log(logger, &format!("│ 📅 Schedule\t: Next at {}", hint.format("%Y-%m-%d %H:%M")));
    }
    
    logger_log(logger, "│");

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            retry_with_countdown(attempt, logger).await;
        }

        let now = Local::now();
        let current_date = now.format("%Y-%m-%d").to_string();
        let current_day = now.format("%A").to_string();
        let current_year = now.year();

        let prompt = build_clarification_prompt(
            text,
            missing_fields,
            &current_date,
            &current_day,
            current_year,
            current_deadline,
            next_meeting_hint,
        );

        // TIER 1: Try Gemini
        match try_gemini_clarification(&prompt, current_deadline, logger).await {
            Ok(result) => {
                logger_log(logger, "└────────────────────────────────────────────");
                return Ok(result);
            }
            Err(e) if e == "rate limit" => {
                logger_log(logger, "│ 🔄 Fallback\t: Switching to Groq...");
                logger_log(logger, "│");
            }
            Err(_) => {}
        }

        // TIER 2: Try Groq reasoning
        match try_groq_reasoning_clarification(&prompt, current_deadline, logger).await {
            Ok(result) => {
                logger_log(logger, "└────────────────────────────────────────────");
                return Ok(result);
            }
            Err(_) => {}
        }

        // TIER 3: Try Groq standard
        match try_groq_standard_clarification(&prompt, current_deadline, logger).await {
            Ok(result) => {
                logger_log(logger, "└────────────────────────────────────────────");
                return Ok(result);
            }
            Err(_) => {
                if attempt < MAX_RETRIES - 1 {
                    logger_log(logger, &format!("│ ⚠️ Attempt {}/{}\t: All models failed, retrying...", 
                             attempt + 1, MAX_RETRIES - 1));
                }
            }
        }
    }

    // Fallback to regex after all retries
    logger_log(logger, &format!("│ ❌ CRITICAL\t: All AI models exhausted after {} retries", MAX_RETRIES));
    logger_log(logger, "│ 🔄 Fallback\t: Using regex parser...");
    logger_log(logger, "└────────────────────────────────────────────");
    
    parse_natural_language_fallback(text, current_deadline, next_meeting_hint)
}

fn resolve_next_meeting(assignment: &AssignmentWithCourse) -> Option<NaiveDateTime> {
    let oracle = ScheduleOracle::load_from_file("schedule.json").ok()?;
    let today = Local::now().naive_local().date();
    let mut earliest: Option<NaiveDateTime> = None;

    for p in &assignment.parallel_codes {
        if let Some((date, time_str)) = oracle.get_next_meeting_with_time(&assignment.course_name, p, today) {
             if let Ok(time) = NaiveTime::parse_from_str(&time_str, "%H:%M") {
                 let dt = date.and_time(time);
                 
                 if earliest.is_none() || dt < earliest.unwrap() {
                     earliest = Some(dt);
                 }
             }
        }
    }
    earliest
}

// ===== GEMINI CLARIFICATION =====

async fn try_gemini_clarification(
    prompt: &str,
    current_deadline: Option<NaiveDateTime>,
    logger: Option<&JobLogger>,
) -> Result<HashMap<String, String>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set".to_string())?;
    
    let mut last_trying = false;
    let mut all_rate_limited = true;
    
    for (idx, model) in GEMINI_MODELS.iter().enumerate() {
        let index = idx + 1;

        if let Some(l) = logger {
            log_trying_line_tui(model, index, GEMINI_MODELS.len(), l);
        } else {
            print_trying_line_stdout(model, index, GEMINI_MODELS.len(), &mut last_trying);
        }
        
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );
        
        let request_body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.1,
                "maxOutputTokens": 1024,
                "responseMimeType": "application/json"
            }
        });
        
        let client = reqwest::Client::new();
        let response = match client
            .post(&url)
            .header("X-Goog-Api-Key", &api_key)
            .json(&request_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                all_rate_limited = false;
                if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                logger_log(logger, &format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
            continue;
        }
        
        if status.is_success() {
         
            let gemini_response: GeminiResponse = match response.json().await {
                Ok(r) => r,
                Err(e) => {
                    all_rate_limited = false;
                    if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                    logger_log(logger, &format!("│ \x1b[31m❌ PARSE FAILED\x1b[0m\t: {} - {}", model, e));
                    continue;
                }
            };
            
            let ai_text = match extract_ai_text(&gemini_response) {
                Ok(t) => t,
                Err(e) => {
                    all_rate_limited = false;
                    if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                    logger_log(logger, &format!("│ \x1b[31m❌ EXTRACT FAILED\x1b[0m\t: {} - {}", model, e));
                    continue;
                }
            };

            let result = match parse_ai_response(&ai_text, current_deadline) {
                Ok(r) => r,
                Err(e) => {
                    all_rate_limited = false;
                    if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                    logger_log(logger, &format!("│ \x1b[31m❌ PARSE FAILED\x1b[0m\t: {} - {}", model, e));
                    continue;
                }
            };
            
      
            if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
            logger_log(logger, &format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Gemini {}/{})", model, index, GEMINI_MODELS.len()));
            
            return Ok(result);
        }
        
        all_rate_limited = false;
        if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
        logger_log(logger, &format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, status));
    }
    
    if all_rate_limited {
        return Err("rate limit".to_string());
    }
    
    Err("All Gemini models failed".to_string())
}


// ===== GROQ REASONING CLARIFICATION =====

async fn try_groq_reasoning_clarification(
    prompt: &str,
    current_deadline: Option<NaiveDateTime>,
    logger: Option<&JobLogger>,
) -> Result<HashMap<String, String>, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    let mut last_trying = false;
    
    for (idx, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let index = idx + 1;
        if let Some(l) = logger {
            log_trying_line_tui(model, index, GROQ_REASONING_MODELS.len(), l);
        } else {
            print_trying_line_stdout(model, index, GROQ_REASONING_MODELS.len(), &mut last_trying);
        }
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.1,
            "top_p": 0.95,
            "max_completion_tokens": 2048,
            "response_format": { "type": "json_object" }
        });
        
        let client = reqwest::Client::new();
        let response = match client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                logger_log(logger, &format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
            continue;  
        }
        
        if status.is_success() {
            let groq_response: GroqResponse = match response.json().await {
                Ok(r) => r,
                Err(e) => {
                    if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                    logger_log(logger, &format!("│ \x1b[31m❌ PARSE FAILED\x1b[0m\t: {} - {}", model, e));
                    continue;
                }
            };
            
            let ai_text = match extract_groq_text(&groq_response) {
                Ok(t) => t,
                Err(e) => {
                    if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                    logger_log(logger, &format!("│ \x1b[31m❌ EXTRACT FAILED\x1b[0m\t: {} - {}", model, e));
                    continue;
                }
            };

            let result = match parse_ai_response(&ai_text, current_deadline) {
                Ok(r) => r,
                Err(e) => {
                    if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                    logger_log(logger, &format!("│ \x1b[31m❌ PARSE FAILED\x1b[0m\t: {} - {}", model, e));
                    continue;
                }
            };

            if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
            logger_log(logger, &format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Groq Reasoning {}/{})", model, index, GROQ_REASONING_MODELS.len()));
            
            return Ok(result);
        }
        
        if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
        logger_log(logger, &format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, status));
    }
    
    Err("All Groq reasoning models failed".to_string())
}


// ===== GROQ STANDARD CLARIFICATION =====

async fn try_groq_standard_clarification(
    prompt: &str,
    current_deadline: Option<NaiveDateTime>,
    logger: Option<&JobLogger>,
) -> Result<HashMap<String, String>, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    let mut last_trying = false;
    
    for (idx, model) in GROQ_TEXT_MODELS.iter().enumerate() {
        let index = idx + 1;
        if let Some(l) = logger {
            log_trying_line_tui(model, index, GROQ_TEXT_MODELS.len(), l);
        } else {
            print_trying_line_stdout(model, index, GROQ_TEXT_MODELS.len(), &mut last_trying);
        }
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.1,
            "top_p": 0.95,
            "max_tokens": 2048,
            "response_format": { "type": "json_object" }
        });
        
        let client = reqwest::Client::new();
        let response = match client.post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
                logger_log(logger, &format!("│ \x1b[31m❌ FAILED\t: {} - Network error\x1b[0m", model));
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
            logger_log(logger, &format!("│ \x1b[33m⏳ RATE LIMIT\t: {} - Trying next model...\x1b[0m", model));
            continue;
        }
        
        if status.is_success() {
            if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
            logger_log(logger, &format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: \x1b[35m{}\x1b[0m (Groq Standard \x1b[36m{}\x1b[0m/\x1b[36m{}\x1b[0m)", model, index, GROQ_TEXT_MODELS.len()));
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            return parse_ai_response(&ai_text, current_deadline);
        }
        
        if logger.is_none() { clear_previous_trying_stdout(&mut last_trying); }
        logger_log(logger, &format!("│ \x1b[31m❌ FAILED\t: {} - HTTP {}\x1b[0m", model, status));
    }
    
    Err("All Groq standard models failed".to_string())
}

// ===== HELPER FUNCTIONS =====

fn is_cancellation(text: &str) -> bool {
    let cancel_keywords = [
        "cancel", "batal", "batalkan", "tidak", "no", "skip",
        "gajadi", "ga jadi", "gak jadi", "nggak jadi", "ndak jadi", "tidak jadi",
        "nope", "lupakan", "abaikan"
    ];
    cancel_keywords.iter().any(|&kw| text == kw || text.starts_with(&format!("{} ", kw)))
}

fn build_clarification_prompt(
    user_message: &str,
    missing_fields: &[String],
    current_date: &str,
    current_day: &str,
    current_year: i32,
    current_deadline: Option<NaiveDateTime>,
    schedule_hint: Option<NaiveDateTime>,
) -> String {
    let missing_fields_str = missing_fields.join(", ");
    
    let existing_deadline_info = if let Some(dl) = current_deadline {
        format!("Existing deadline: {} (only update if user provides new info)", dl.format("%Y-%m-%d %H:%M"))
    } else {
        "No existing deadline yet".to_string()
    };
    
    let schedule_info = if let Some(sched) = schedule_hint {
        format!("NEXT CLASS MEETING: {} (Use this if user says 'pertemuan berikutnya', 'sesuai jadwal', 'pas kelas', 'during class')", 
            sched.format("%Y-%m-%d %H:%M"))
    } else {
        "Schedule info: Not available".to_string()
    };

    format!(
        r#"You are a bilingual (Indonesian/English) assistant that parses NATURAL LANGUAGE clarification responses for academic assignments. 

CURRENT CONTEXT:
- Today: {current_date} ({current_day})
- Year: {current_year}
- {existing_deadline_info}
- {schedule_info}
- Fields needing clarification: [{missing_fields_str}]

USER MESSAGE: 
"{user_message}"

YOUR TASK:
Parse the user's FREE-FORM message. Users may write naturally WITHOUT labels/prefixes. 
Extract information based on the missing fields and context.

MAPPINGS:
- Dates: "besok"=+1d, "lusa"=+2d, "minggu depan"=+7d.
- Times: "pagi"=08:00, "siang"=12:00, "sore"=15:00, "malam"=20:00.
- Keywords: "pertemuan berikutnya", "sesuai jadwal" => Use NEXT CLASS MEETING date/time.
- Codes: "K1", "K2", "All".

RESPONSE FORMAT (JSON only):
{{
  "deadline": "YYYY-MM-DD" or null,
  "deadline_time": "HH:MM" or null,
  "course_name": "string" or null,
  "title": "string" or null,
  "description": "string" or null,
  "parallel_codes": ["K1", "K2"] or null,
  "is_cancellation": false
}}

Respond with ONLY the JSON."#,
        current_date = current_date,
        current_day = current_day,
        current_year = current_year,
        existing_deadline_info = existing_deadline_info,
        schedule_info = schedule_info,
        missing_fields_str = missing_fields_str,
        user_message = user_message,
    )
}

fn parse_ai_response(
    ai_response: &str,
    current_deadline: Option<NaiveDateTime>,
) -> Result<HashMap<String, String>, String> {
    let cleaned = ai_response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let parsed: AIClarificationResult = serde_json::from_str(cleaned)
        .map_err(|e| format!("Failed to parse AI response: {} - Raw: {}", e, cleaned))?;

    if parsed.is_cancellation {
        return Err("cancelled".to_string());
    }

    let mut updates = HashMap::new();

    if let Some(date_str) = &parsed.deadline {
        if !date_str.is_empty() {
            let time_str = parsed.deadline_time.as_deref().unwrap_or("23:59");
            let deadline_str = format!("{} {}", date_str, time_str);
            updates.insert("deadline".to_string(), deadline_str);
        }
    } else if let Some(time_str) = &parsed.deadline_time {
        if let Some(existing) = current_deadline {
            if let Ok(new_time) = NaiveTime::parse_from_str(time_str, "%H:%M") {
                let new_deadline = existing.date().and_time(new_time);
                updates.insert("deadline".to_string(), new_deadline.format("%Y-%m-%d %H:%M").to_string());
            }
        } else {
            return Err("no_date".to_string());
        }
    }

    if let Some(course) = &parsed.course_name {
        if !course.is_empty() { updates.insert("course_name".to_string(), course.clone()); }
    }
    if let Some(title) = &parsed.title {
        if !title.is_empty() { updates.insert("title".to_string(), title.clone()); }
    }
    if let Some(desc) = &parsed.description {
        if !desc.is_empty() { updates.insert("description".to_string(), desc.clone()); }
    }
    if let Some(codes) = &parsed.parallel_codes {
        if !codes.is_empty() {
            let normalized: Vec<String> = codes.iter().map(|c| c.to_uppercase()).collect();
            updates.insert("parallel_codes".to_string(), normalized.join(","));
        }
    }

    if updates.is_empty() {
        return Err("no_data".to_string());
    }

    Ok(updates)
}

// ===== FALLBACK REGEX PARSER =====

pub fn parse_natural_language_fallback(
    text: &str,
    current_deadline: Option<NaiveDateTime>,
    schedule_hint: Option<NaiveDateTime>,
) -> Result<HashMap<String, String>, String> {
    let text_lower = text.trim().to_lowercase();
    
    if is_cancellation(&text_lower) {
        return Err("cancelled".to_string());
    }
    
    let now = Local::now().naive_local();
    let today = now.date();
    
    let mut updates = HashMap::new();

    if let Some(sched) = schedule_hint {
        if check_schedule_keywords(&text_lower) {
            updates.insert("deadline".to_string(), sched.format("%Y-%m-%d %H:%M").to_string());
        }
    }

    if !updates.contains_key("deadline") {
        let parsed_date = parse_relative_date(&text_lower, today);
        let parsed_time = parse_natural_time(&text_lower);

        if let Some(date) = parsed_date {
            let time = parsed_time.unwrap_or_else(|| NaiveTime::from_hms_opt(23, 59, 0).unwrap());
            let deadline = date.and_time(time);
            updates.insert("deadline".to_string(), deadline.format("%Y-%m-%d %H:%M").to_string());
        } else if let Some(time) = parsed_time {
            if let Some(existing) = current_deadline {
                let new_deadline = existing.date().and_time(time);
                updates.insert("deadline".to_string(), new_deadline.format("%Y-%m-%d %H:%M").to_string());
            }
        }
    }

    if let Some(desc) = extract_description_part(&text_lower) {
        if !desc.is_empty() {
            updates.insert("description".to_string(), desc);
        }
    }

    let parallel_codes = detect_parallel_codes(&text_lower);
    if !parallel_codes.is_empty() {
        updates.insert("parallel_codes".to_string(), parallel_codes.join(","));
    }

    if updates.is_empty() {
        return Err("no_data".to_string());
    }

    Ok(updates)
}

fn check_schedule_keywords(text: &str) -> bool {
    let keywords = [
        "pertemuan berikutnya", "sesuai jadwal", "ikut jadwal",
        "saat kelas", "ketika kelas", "pas kelas",
        "next meeting", "during class", "in class"
    ];
    keywords.iter().any(|k| text.contains(k))
}

fn parse_relative_date(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    if text.contains("besok") || text.contains("tomorrow") {
        return Some(today + Duration::days(1));
    }
    if text.contains("lusa") || text.contains("day after tomorrow") {
        return Some(today + Duration::days(2));
    }
    if text.contains("minggu depan") || text.contains("next week") {
        return Some(today + Duration::days(7));
    }
    if let Some(days) = extract_days_from_text(text) {
        return Some(today + Duration::days(days));
    }
    if let Some(target_day) = parse_day_name(text) {
        let force_next = text.contains("depan") || text.contains("next");
        return Some(next_weekday(today, target_day, force_next));
    }
    None
}

fn extract_days_from_text(text: &str) -> Option<i64> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+)\s*hari\s*lagi").unwrap());
    if let Some(caps) = RE.captures(text) {
        if let Some(num) = caps.get(1) {
             return num.as_str().parse::<i64>().ok();
        }
    }
    None
}

fn parse_day_name(text: &str) -> Option<Weekday> {
    let day_mappings = [
        ("senin", Weekday::Mon), ("monday", Weekday::Mon),
        ("selasa", Weekday::Tue), ("tuesday", Weekday::Tue),
        ("rabu", Weekday::Wed), ("wednesday", Weekday::Wed),
        ("kamis", Weekday::Thu), ("thursday", Weekday::Thu),
        ("jumat", Weekday::Fri), ("jum'at", Weekday::Fri), ("friday", Weekday::Fri),
        ("sabtu", Weekday::Sat), ("saturday", Weekday::Sat),
        ("minggu", Weekday::Sun), ("sunday", Weekday::Sun),
    ];
    for (name, weekday) in day_mappings {
        if text.contains(name) { return Some(weekday); }
    }
    None
}

fn next_weekday(from: NaiveDate, target: Weekday, force_next_week: bool) -> NaiveDate {
    let current_num = from.weekday().num_days_from_monday();
    let target_num = target.num_days_from_monday();
    let mut days_ahead = if target_num > current_num { 
        target_num - current_num 
    } else { 
        7 - current_num + target_num 
    };
    
    if days_ahead == 0 { days_ahead = 7; }
    if days_ahead < 7 && force_next_week { days_ahead += 7; }

    from + Duration::days(days_ahead as i64)
}

fn parse_natural_time(text: &str) -> Option<NaiveTime> {
    let keywords = [
        ("tengah malam", 23, 59), ("pagi", 8, 0), ("siang", 12, 0), 
        ("sore", 15, 0), ("malam", 20, 0), ("subuh", 5, 0)
    ];
    for (k, h, m) in keywords {
        if text.contains(k) { return NaiveTime::from_hms_opt(h, m, 0); }
    }

    static RE_TIME: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:jam\s*)?(\d{1,2})[:.](\d{2})").unwrap());
    if let Some(caps) = RE_TIME.captures(text) {
        if let (Ok(h), Ok(m)) = (caps[1].parse::<u32>(), caps[2].parse::<u32>()) {
            return NaiveTime::from_hms_opt(h, m, 0);
        }
    }
    
    static RE_HOUR: Lazy<Regex> = Lazy::new(|| Regex::new(r"jam\s*(\d{1,2})").unwrap());
    if let Some(caps) = RE_HOUR.captures(text) {
        if let Ok(h) = caps[1].parse::<u32>() {
            return NaiveTime::from_hms_opt(h, 0, 0);
        }
    }
    
    None
}

fn extract_description_part(text: &str) -> Option<String> {
    let indicators = ["tugasnya", "tugas", "kerjakan", "submit", "soal", "halaman", "chapter"]; 
    if !indicators.iter().any(|&i| text.contains(i)) { return None; }
    
    if text.len() > 10 { return Some(text.to_string()); }
    None
}

fn detect_parallel_codes(text: &str) -> Vec<String> {
    let mut codes = Vec::new();
    if text.contains("semua") || text.contains("all") { return vec!["ALL".to_string()]; }
    
    static RE_CODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b([KPR][1-4])\b").unwrap());
    for caps in RE_CODE.captures_iter(text) {
        codes.push(caps[1].to_uppercase());
    }
    codes
}