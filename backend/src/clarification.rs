use crate::models::AssignmentWithCourse;
use crate::parser::ai_extractor::schedule_oracle::ScheduleOracle;
// Import the model constants
use crate::parser::ai_extractor::{GROQ_REASONING_MODELS, GROQ_TEXT_MODELS, GEMINI_MODELS};
use uuid::Uuid;
use std::collections::HashMap;
use chrono::{Local, NaiveDateTime, NaiveDate, NaiveTime, Duration, Datelike, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::json;
use regex::Regex;
use once_cell::sync::Lazy;

// ===== YOUR EXISTING HELPER FUNCTIONS (unchanged) =====

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
    
    let info_message = format!(
        "*[PERLU KLARIFIKASI]*\n\
        `ID: {}`\n\
        \n\
        📌 *{}* - {}\n\
        {}\n\
        ⏰ Deadline: {}\n\
        🧩 Parallel: {}\n\
        \n\
        *[INFO KURANG]:*\n\
        {}",
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

// ===== AI & NATURAL LANGUAGE PARSING =====

pub async fn parse_clarification_response(
    text: &str, 
    assignment: &AssignmentWithCourse,
    missing_fields: &[String]
) -> Result<HashMap<String, String>, String> {
    
    let current_deadline = assignment.deadline.map(|d| d.naive_utc());
    let next_meeting_hint = resolve_next_meeting(assignment);

    println!("🤖 Attempting AI Clarification parsing...");
    match parse_clarification_with_ai_fallback(text, missing_fields, current_deadline, next_meeting_hint).await {
        Ok(result) => {
             println!("✅ AI Parsing Success");
             Ok(result)
        },
        Err(e) => {
            eprintln!("⚠️ All AI models failed: {}. Falling back to Regex.", e);
            parse_natural_language_fallback(text, current_deadline, next_meeting_hint)
        }
    }
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

// ===== AI FALLBACK SYSTEM (using imported constants) =====

async fn parse_clarification_with_ai_fallback(
    user_message: &str,
    missing_fields: &[String],
    current_deadline: Option<NaiveDateTime>,
    schedule_hint: Option<NaiveDateTime>,
) -> Result<HashMap<String, String>, String> {
    
    let text_lower = user_message.trim().to_lowercase();
    if is_cancellation(&text_lower) {
        return Err("cancelled".to_string());
    }

    let now = Local::now();
    let current_date = now.format("%Y-%m-%d").to_string();
    let current_day = now.format("%A").to_string();
    let current_year = now.year();

    let prompt = build_clarification_prompt(
        user_message,
        missing_fields,
        &current_date,
        &current_day,
        current_year,
        current_deadline,
        schedule_hint,
    );

    println!("\x1b[1;30m┌── 🤖 CLARIFICATION PARSING ──────────────────\x1b[0m");
    
    // TIER 1: Gemini
    match try_gemini_clarification(&prompt, current_deadline).await {
        Ok(result) => {
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            return Ok(result);
        }
        Err(e) => {
            eprintln!("│ ⚠️  Gemini failed: {}", e);
            eprintln!("│\n│ 🔄 Falling back to Groq...");
        }
    }
    
    // TIER 2: Groq reasoning
    match try_groq_reasoning_clarification(&prompt, current_deadline).await {
        Ok(result) => {
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            return Ok(result);
        }
        Err(e) => {
            eprintln!("│ ⚠️  Groq Reasoning failed: {}", e);
        }
    }
    
    // TIER 3: Groq standard
    match try_groq_standard_clarification(&prompt, current_deadline).await {
        Ok(result) => {
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            return Ok(result);
        }
        Err(e) => {
            eprintln!("│ ⚠️  Groq Standard failed: {}", e);
        }
    }
    
    println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
    Err("All AI models failed".to_string())
}

// ===== GEMINI CLARIFICATION =====

async fn try_gemini_clarification(
    prompt: &str,
    current_deadline: Option<NaiveDateTime>,
) -> Result<HashMap<String, String>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set".to_string())?;
    
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
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
            Err(e) => {
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {} (Gemini {}/{})", 
                    model, index + 1, GEMINI_MODELS.len());
                eprintln!("│    Error: {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  R-LIMIT  : {} (Gemini {}/{})", 
                model, index + 1, GEMINI_MODELS.len());
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Gemini models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : {} (Gemini {}/{})", 
                model, index + 1, GEMINI_MODELS.len());
            
            let json: serde_json::Value = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = json["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .ok_or("Failed to extract text from Gemini response")?;
            
            return parse_ai_response(ai_text, current_deadline);
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} (Gemini {}/{}) - {}", 
            model, index + 1, GEMINI_MODELS.len(), status);
        eprintln!("│    {}", truncate_for_log(&error_text));
        
        if index < GEMINI_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Gemini models failed".to_string())
}

// ===== GROQ REASONING CLARIFICATION =====

async fn try_groq_reasoning_clarification(
    prompt: &str,
    current_deadline: Option<NaiveDateTime>,
) -> Result<HashMap<String, String>, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    for (index, model) in GROQ_REASONING_MODELS.iter().enumerate() {
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
            Err(e) => {
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {} (Groq Reasoning {}/{})", 
                    model, index + 1, GROQ_REASONING_MODELS.len());
                eprintln!("│    Error: {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  R-LIMIT  : {} (Groq Reasoning {}/{})", 
                model, index + 1, GROQ_REASONING_MODELS.len());
            if index < GROQ_REASONING_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Groq reasoning models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : {} (Groq Reasoning {}/{})", 
                model, index + 1, GROQ_REASONING_MODELS.len());
            
            let json: serde_json::Value = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or("Failed to extract text from Groq response")?;
            
            return parse_ai_response(ai_text, current_deadline);
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} (Groq Reasoning {}/{}) - {}", 
            model, index + 1, GROQ_REASONING_MODELS.len(), status);
        eprintln!("│    {}", truncate_for_log(&error_text));
        
        if index < GROQ_REASONING_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Groq reasoning models failed".to_string())
}

// ===== GROQ STANDARD CLARIFICATION =====

async fn try_groq_standard_clarification(
    prompt: &str,
    current_deadline: Option<NaiveDateTime>,
) -> Result<HashMap<String, String>, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    for (index, model) in GROQ_TEXT_MODELS.iter().enumerate() {
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
            Err(e) => {
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {} (Groq Standard {}/{})", 
                    model, index + 1, GROQ_TEXT_MODELS.len());
                eprintln!("│    Error: {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  R-LIMIT  : {} (Groq Standard {}/{})", 
                model, index + 1, GROQ_TEXT_MODELS.len());
            if index < GROQ_TEXT_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Groq standard models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            println!("│ \x1b[33m⚠️  STANDARD\x1b[0m : {} (Groq Standard {}/{})", 
                model, index + 1, GROQ_TEXT_MODELS.len());
            
            let json: serde_json::Value = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or("Failed to extract text from Groq response")?;
            
            return parse_ai_response(ai_text, current_deadline);
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} (Groq Standard {}/{}) - {}", 
            model, index + 1, GROQ_TEXT_MODELS.len(), status);
        eprintln!("│    {}", truncate_for_log(&error_text));
        
        if index < GROQ_TEXT_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Groq standard models failed".to_string())
}

// ===== HELPER FUNCTIONS =====

fn truncate_for_log(text: &str) -> String {
    if text.len() > 60 {
        format!("{}...", &text[..60])
    } else {
        text.to_string()
    }
}

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

// ===== FALLBACK REGEX PARSER (keep your existing implementation) =====

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