use crate::models::{AIClassification, Assignment};
use uuid::Uuid;
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use sqlx::PgPool;

use super::schedule_oracle::ScheduleOracle;
use once_cell::sync::Lazy;

use super::prompts::*;
use super::parsing::*;
use super::{GROQ_REASONING_MODELS, GROQ_VISION_MODELS, GROQ_TEXT_MODELS, GEMINI_MODELS};
use super::context_builder::build_context;

pub static SCHEDULE_ORACLE: Lazy<ScheduleOracle> = Lazy::new(|| {
    ScheduleOracle::load_from_file("schedule.json")
        .expect("Failed to load schedule.json")
});

// ===== RETRY CONFIGURATION =====
const MAX_RETRIES: u32 = 4;
const RETRY_DELAY_SECS: u64 = 20;

/// Generic retry wrapper for API calls with exponential backoff
async fn retry_with_backoff<F, Fut, T>(
    operation: F,
    operation_name: &str,
) -> Result<T, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let mut attempt = 1;
    
    loop {
        match operation().await {
            Ok(result) => {
                // Clear retry line if we had retries
                if attempt > 1 {
                    print!("\r\x1b[K");
                    std::io::stdout().flush().ok();
                }
                return Ok(result);
            }
            Err(e) if e.contains("rate limit") && attempt < MAX_RETRIES => {
                let delay = RETRY_DELAY_SECS * attempt as u64;
                print!("\r│ ⏳ RETRY     : {} (attempt {}/{}) - waiting {}s...\x1b[K", 
                    operation_name, attempt + 1, MAX_RETRIES, delay);
                std::io::stdout().flush().ok();
                tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                attempt += 1;
            }
            Err(e) => {
                // Clear retry line on final failure
                if attempt > 1 {
                    print!("\r\x1b[K");
                    std::io::stdout().flush().ok();
                }
                return Err(e);
            }
        }
    }
}

// ===== MAIN AI EXTRACTION FUNCTION =====

pub async fn extract_with_ai(
    text: &str,
    available_courses: &str,
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    image_base64: Option<&str>,
    sender_id: &str,
    pool: &PgPool,
    quoted_message: Option<&str>,  
    quoted_message_id: Option<&str>,
) -> Result<AIClassification, String> {
    let current_datetime = get_current_datetime();
    let current_date = get_current_date();
    
    println!("\n\x1b[1;30m┌── 🤖 AI PROCESSING ──────────────────────────\x1b[0m");
    
    let message_display = text
        .replace('\n', "\\n")
        .chars()
        .take(80)
        .collect::<String>();
    
    let message_truncated = if text.len() > 60 {
        format!("\"{}...\"", message_display)
    } else {
        format!("\"{}\"", message_display)
    };
    
    println!("│ 📝 Message  : \x1b[36m{}\x1b[0m", message_truncated);
    
    if let Some(quoted) = quoted_message {
        let quoted_display = quoted
            .replace('\n', "\\n")
            .chars()
            .take(80)
            .collect::<String>();
        
        let quoted_truncated = if quoted.len() > 60 {
            format!("\"{}...\"", quoted_display)
        } else {
            format!("\"{}\"", quoted_display)
        };
        
        println!("│ 💬 Quoted   : \x1b[35m{}\x1b[0m", quoted_truncated);
    }
    
    // STAGE 1: Build context
    let context = match build_context(
        text, 
        sender_id, 
        pool, 
        &*SCHEDULE_ORACLE,
        quoted_message,
        quoted_message_id
    ).await {
        Ok(ctx) => {
            let courses_summary = if ctx.course_hints.is_empty() {
                "none".to_string()
            } else {
                ctx.course_hints
                    .iter()
                    .map(|ch| {
                        if ch.parallel_schedules.is_empty() {
                            let parallel = if ch.parallel_codes.is_empty() {
                                "?".to_string()
                            } else {
                                format!("[{}]", ch.parallel_codes.join(","))
                            };
                            format!("{}:{}", ch.course_name, parallel)
                        } else {
                            let schedules_str = ch.parallel_schedules
                                .iter()
                                .map(|ps| {
                                    if let Some(ref meeting) = ps.next_meeting {
                                        format!("{}:{}", ps.parallel_code, meeting)
                                    } else {
                                        format!("{}:?", ps.parallel_code)
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(",");
                            format!("{}:[{}]", ch.course_name, schedules_str)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            
            let parallel_summary = if ctx.course_hints.is_empty() {
                "none".to_string()
            } else {
                let courses_with_parallels: Vec<String> = ctx.course_hints
                    .iter()
                    .filter(|ch| !ch.parallel_codes.is_empty())
                    .map(|ch| {
                        let course_abbr = ch.course_name
                            .split_whitespace()
                            .take(2)
                            .collect::<Vec<_>>()
                            .join(" ");
                        format!("{}:[{}]", course_abbr, ch.parallel_codes.join(","))
                    })
                    .collect();
                
                if courses_with_parallels.is_empty() {
                    "none".to_string()
                } else {
                    courses_with_parallels.join(", ")
                }
            };
            
            println!("│\n│ ✅ Context  : Detected={} ({}), Schedules=[{}]",
                parallel_summary, ctx.parallel_source, courses_summary);
            
            Some(ctx)
        }
        Err(e) => {
            eprintln!("│\n│ ⚠️  Context failed: {}", e);
            None
        }
    };
    
    let prompt = build_classification_prompt(
        text, 
        available_courses, 
        active_assignments,
        course_map, 
        &current_datetime, 
        &current_date,
        context.as_ref()
    );
    
    println!("│ 🤖 Stage 2  : Extracting with AI...");

    if image_base64.is_some() {
        println!("│ 🖼️  Image    : Attached (may be irrelevant meme)");
    }
    println!("│ 📊 Context  : {} active assignments", active_assignments.len());
    println!("│ 📅 Time     : {}", current_datetime);
    
    // TIER 1: If image present, try Groq vision first
    if let Some(img) = image_base64 {
        match retry_with_backoff(
            || try_groq_vision(&prompt, img),
            "Groq Vision"
        ).await {
            Ok(classification) => {
                match classification {
                    AIClassification::Unrecognized {reason}=> {
                        let reason_display = reason.as_deref().unwrap_or("No reason provided");
                        println!("│ ℹ️  Vision Result: Unrecognized ({})", reason_display);
                        println!("│ 🔄 Retrying with Gemini text-only...");
                        
                        match retry_with_backoff(
                            || try_gemini_models(&prompt),
                            "Gemini"
                        ).await {
                            Ok(text_result) => {
                                match text_result {
                                    AIClassification::Unrecognized {..} => {
                                        println!("│ ⚠️  Gemini: Still unrecognized");
                                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                                        return Ok(AIClassification::Unrecognized{reason});
                                    }
                                    _ => {
                                        log_classification_success(&text_result);
                                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                                        return Ok(text_result);
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("│ ⚠️  Gemini fallback failed: {}", e);
                            }
                        }
                    }
                    _ => {
                        log_classification_success(&classification);
                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                        return Ok(classification);
                    }
                }
            }
            Err(e) => {
                eprintln!("│ ⚠️  Vision model error: {}", e);
                println!("│ 🔄 Trying Gemini text-only...");
                
                match retry_with_backoff(
                    || try_gemini_models(&prompt),
                    "Gemini"
                ).await {
                    Ok(classification) => {
                        log_classification_success(&classification);
                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                        return Ok(classification);
                    }
                    Err(e) => {
                        eprintln!("│ ⚠️  Gemini fallback failed: {}", e);
                    }
                }
            }
        }
    } else {
        // TIER 1: No image - try Gemini first
        match retry_with_backoff(
            || try_gemini_models(&prompt),
            "Gemini"
        ).await {
            Ok(classification) => {
                log_classification_success(&classification);
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Ok(classification);
            }
            Err(e) => {
                eprintln!("│ ⚠️  Gemini failed: {}", e);
                eprintln!("│\n│ 🔄 Falling back to Groq...");
            }
        }
    }
    
    // TIER 2: Groq fallback (reasoning models)
    match retry_with_backoff(
        || try_groq_reasoning(&prompt),
        "Groq Reasoning"
    ).await {
        Ok(classification) => {
            log_classification_success(&classification);
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            return Ok(classification);
        }
        Err(e) => {
            eprintln!("│ ⚠️  Groq Reasoning failed: {}", e);
        }
    }
    
    println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
    Err("All models failed after retries".to_string())
}

// ===== GEMINI MODELS =====

async fn try_gemini_models(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        // Show trying status
        print!("\r│ 🔄 TRYING    : {} (Gemini {}/{})...\x1b[K", model, index + 1, GEMINI_MODELS.len());
        std::io::stdout().flush().ok();
        
        let request_body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": 4096,
                "responseMimeType": "application/json"
            }
        });
        
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );
        
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
                if index < GEMINI_MODELS.len() - 1 {
                    continue; // Try next model silently
                } else {
                    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Gemini models failed\x1b[K\n");
                    std::io::stdout().flush().ok();
                    return Err(format!("Network error: {}", e));
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if index < GEMINI_MODELS.len() - 1 {
                continue; // Try next model silently
            } else {
                print!("\r│ \x1b[33m⚠️  R-LIMIT\x1b[0m   : All Gemini models rate limited\x1b[K\n");
                std::io::stdout().flush().ok();
                return Err("rate limit: All Gemini models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            print!("\r│ \x1b[32m✅ SUCCESS\x1b[0m   : {} (Gemini {}/{})\x1b[K\n", model, index + 1, GEMINI_MODELS.len());
            std::io::stdout().flush().ok();
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_ai_text(&gemini_response)?;
            let classification = parse_classification(ai_text)?;
            
            return Ok(classification);
        }
        
        // Non-success, non-rate-limit error
        if index < GEMINI_MODELS.len() - 1 {
            continue; // Try next model silently
        }
    }
    
    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Gemini models failed\x1b[K\n");
    std::io::stdout().flush().ok();
    Err("All Gemini models failed".to_string())
}

// ===== GROQ REASONING MODELS =====

async fn try_groq_reasoning(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        print!("\r│ 🔄 TRYING    : {} (Groq Reasoning {}/{})...\x1b[K", model, index + 1, GROQ_REASONING_MODELS.len());
        std::io::stdout().flush().ok();
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.6,
            "top_p": 0.95,
            "max_completion_tokens": 8192,
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
                if index < GROQ_REASONING_MODELS.len() - 1 {
                    continue;
                } else {
                    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Groq reasoning models failed\x1b[K\n");
                    std::io::stdout().flush().ok();
                    eprintln!("│ 🔄 Falling back to standard models...");
                    return try_groq_standard_text(prompt).await;
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if index < GROQ_REASONING_MODELS.len() - 1 {
                continue;
            } else {
                print!("\r│ \x1b[33m⚠️  R-LIMIT\x1b[0m   : All Groq reasoning models rate limited\x1b[K\n");
                std::io::stdout().flush().ok();
                eprintln!("│ 🔄 Falling back to standard models...");
                return try_groq_standard_text(prompt).await;
            }
        }
        
        if status.is_success() {
            print!("\r│ \x1b[32m✅ SUCCESS\x1b[0m   : {} (Groq Reasoning {}/{})\x1b[K\n", model, index + 1, GROQ_REASONING_MODELS.len());
            std::io::stdout().flush().ok();
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized { .. }) && !ai_text.contains("unrecognized") {
                if index < GROQ_REASONING_MODELS.len() - 1 {
                    continue;
                }
            }
            
            return Ok(classification);
        }
        
        if index < GROQ_REASONING_MODELS.len() - 1 {
            continue;
        }
    }
    
    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Groq reasoning models failed\x1b[K\n");
    std::io::stdout().flush().ok();
    eprintln!("│ 🔄 Falling back to standard models...");
    try_groq_standard_text(prompt).await
}

// ===== GROQ STANDARD TEXT MODELS =====

async fn try_groq_standard_text(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_TEXT_MODELS.iter().enumerate() {
        print!("\r│ 🔄 TRYING    : {} (Groq Standard {}/{})...\x1b[K", model, index + 1, GROQ_TEXT_MODELS.len());
        std::io::stdout().flush().ok();
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.2,
            "max_tokens": 4096,
            "top_p": 0.95,
            "frequency_penalty": 0.0,
            "presence_penalty": 0.0,
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
                if index < GROQ_TEXT_MODELS.len() - 1 {
                    continue;
                } else {
                    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Groq standard models failed\x1b[K\n");
                    std::io::stdout().flush().ok();
                    return Err("All Groq standard models failed".to_string());
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if index < GROQ_TEXT_MODELS.len() - 1 {
                continue;
            } else {
                print!("\r│ \x1b[33m⚠️  R-LIMIT\x1b[0m   : All Groq standard models rate limited\x1b[K\n");
                std::io::stdout().flush().ok();
                return Err("rate limit: All Groq standard models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            print!("\r│ \x1b[33m⚠️  STANDARD\x1b[0m  : {} (Groq Standard {}/{})\x1b[K\n", model, index + 1, GROQ_TEXT_MODELS.len());
            std::io::stdout().flush().ok();
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized { .. }) && !ai_text.contains("unrecognized") {
                if index < GROQ_TEXT_MODELS.len() - 1 {
                    continue;
                }
            }
            
            return Ok(classification);
        }
        
        if index < GROQ_TEXT_MODELS.len() - 1 {
            continue;
        }
    }
    
    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Groq standard models failed\x1b[K\n");
    std::io::stdout().flush().ok();
    Err("All Groq standard models failed".to_string())
}

// ===== GROQ VISION MODELS =====

async fn try_groq_vision(prompt: &str, image_base64: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_VISION_MODELS.iter().enumerate() {
        print!("\r│ 🔄 TRYING    : {} (Vision {}/{})...\x1b[K", model, index + 1, GROQ_VISION_MODELS.len());
        std::io::stdout().flush().ok();
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/jpeg;base64,{}", image_base64)
                        }
                    }
                ]
            }],
            "temperature": 0.2,
            "max_tokens": 4096,
            "top_p": 0.95,
            "frequency_penalty": 0.0,
            "presence_penalty": 0.0,
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
                if index < GROQ_VISION_MODELS.len() - 1 {
                    continue;
                } else {
                    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Groq vision models failed\x1b[K\n");
                    std::io::stdout().flush().ok();
                    return Err("All Groq vision models failed".to_string());
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if index < GROQ_VISION_MODELS.len() - 1 {
                continue;
            } else {
                print!("\r│ \x1b[33m⚠️  R-LIMIT\x1b[0m   : All Groq vision models rate limited\x1b[K\n");
                std::io::stdout().flush().ok();
                return Err("rate limit: All Groq vision models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            print!("\r│ \x1b[32m✅ SUCCESS\x1b[0m   : {} (Vision {}/{})\x1b[K\n", model, index + 1, GROQ_VISION_MODELS.len());
            std::io::stdout().flush().ok();
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized { .. }) && !ai_text.contains("unrecognized") {
                if index < GROQ_VISION_MODELS.len() - 1 {
                    continue;
                }
            }
            
            return Ok(classification);
        }
        
        if index < GROQ_VISION_MODELS.len() - 1 {
            continue;
        }
    }
    
    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Groq vision models failed\x1b[K\n");
    std::io::stdout().flush().ok();
    Err("All Groq vision models failed".to_string())
}

// ===== DEDUPLICATION AI =====

pub async fn check_duplicate_assignment(
    title: &str,
    description: &str,
    course_name: &str,
    parallel_codes: &[String],
    existing_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
) -> Result<Option<Uuid>, String> {
    
    let new_numbers = extract_numbers(title);
    let new_type = extract_assignment_type(title);
    
    let filtered: Vec<&Assignment> = existing_assignments
        .iter()
        .filter(|a| {
            let same_course = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|name| name.eq_ignore_ascii_case(course_name))
                .unwrap_or(false);
            
            if !same_course { return false; }
            
            if !parallel_codes.is_empty() && !a.parallel_codes.is_empty() {
                let has_overlap = parallel_codes.iter()
                    .any(|new_p| a.parallel_codes.iter()
                        .any(|existing_p| new_p.eq_ignore_ascii_case(existing_p)));
                
                if !has_overlap { return false; }
            }
            
            let existing_numbers = extract_numbers(&a.title);
            if !new_numbers.is_empty() && !existing_numbers.is_empty() {
                if new_numbers != existing_numbers { return false; }
            }
            
            if let Some(ref new_t) = new_type {
                if let Some(existing_t) = extract_assignment_type(&a.title) {
                    if new_t != &existing_t { return false; }
                }
            }
            
            let similarity = calculate_word_overlap(title, &a.title);
            if similarity < 0.2 { return false; }
            
            true
        })
        .collect();
    
    if filtered.is_empty() {
        return Ok(None);
    }
    
    if filtered.len() > 3 {
        return Ok(None);
    }
    
    let filtered_owned: Vec<Assignment> = filtered.into_iter().cloned().collect();
    let prompt = build_duplicate_detection_prompt(
        title,
        description,
        course_name,
        parallel_codes,
        &filtered_owned,
        course_map,
    );
    
    // With retry logic
    retry_with_backoff(
        || try_gemini_duplicate_check(&prompt),
        "Duplicate Check"
    ).await
}

async fn try_gemini_duplicate_check(prompt: &str) -> Result<Option<Uuid>, String> {
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
                "temperature": 0.0,
                "maxOutputTokens": 1024,
                "topP": 0.95,
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
            Err(_) => continue,
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                return Err("rate limit: All Gemini models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Parse error: {}", e))?;
            
            let ai_text = extract_ai_text(&gemini_response)?;
            let result: DuplicateCheckResult = serde_json::from_str(ai_text)
                .map_err(|e| format!("JSON error: {}", e))?;
            
            if result.is_duplicate && result.confidence == "high" {
                if let Some(ref id_str) = result.matched_assignment_id {
                    if let Ok(uuid) = Uuid::parse_str(id_str) {
                        return Ok(Some(uuid));
                    }
                }
            } else if result.is_duplicate {
                println!("⚠️  Low confidence duplicate - Reason: {}", result.reason);
            }
            
            return Ok(None);
        }
        
        if index < GEMINI_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Gemini models failed".to_string())
}

// ===== MATCHING =====

pub async fn match_update_to_assignment(
    changes: &str,
    keywords: &[String],
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    parallel_codes: &[String],
) -> Result<Option<Uuid>, String> {
    let prompt = build_matching_prompt(changes, keywords, active_assignments, course_map, parallel_codes);
    
    println!("\x1b[1;30m┌── 🤖 AI MATCHING ────────────────────────────\x1b[0m");
    println!("│ 🔍 Keywords   : {:?}", keywords);
    
    if !parallel_codes.is_empty() {
        println!("│ 🧩 Parallels  : [{}]", parallel_codes.join(", "));
    }
    
    let result = retry_with_backoff(
        || try_gemini_matching(&prompt),
        "Matching"
    ).await;
    
    println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
    result
}

async fn try_gemini_matching(prompt: &str) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        print!("\r│ 🔄 MATCHING  : {} (Gemini {}/{})...\x1b[K", model, index + 1, GEMINI_MODELS.len());
        std::io::stdout().flush().ok();
        
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );
        
        let request_body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": 4096,
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
                if index < GEMINI_MODELS.len() - 1 {
                    continue;
                } else {
                    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Gemini matching models failed\x1b[K\n");
                    std::io::stdout().flush().ok();
                    return Err("All Gemini models failed for matching".to_string());
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                print!("\r│ \x1b[33m⚠️  R-LIMIT\x1b[0m   : All Gemini matching models rate limited\x1b[K\n");
                std::io::stdout().flush().ok();
                return Err("rate limit: All Gemini models rate limited for matching".to_string());
            }
        }
        
        if status.is_success() {
            print!("\r│ \x1b[32m✅ MATCHED\x1b[0m   : {} (Gemini {}/{})\x1b[K\n", model, index + 1, GEMINI_MODELS.len());
            std::io::stdout().flush().ok();
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_ai_text(&gemini_response)?;
            let result = parse_match_result(ai_text)?;
            
            return Ok(result);
        }
        
        if index < GEMINI_MODELS.len() - 1 {
            continue;
        }
    }
    
    print!("\r│ \x1b[31m❌ FAILED\x1b[0m    : All Gemini matching models failed\x1b[K\n");
    std::io::stdout().flush().ok();
    Err("All Gemini models failed for matching".to_string())
}

// ===== LOGGING HELPER =====

fn log_classification_success(classification: &AIClassification) {
    match classification {
        AIClassification::MultipleAssignments { assignments, .. } => {
            println!("│\n│ ✅ Result   : {} assignments detected", assignments.len());
            for (i, a) in assignments.iter().enumerate() {
                let parallels = if a.parallel_codes.is_empty() {
                    "N/A".to_string()
                } else {
                    format!("[{}]", a.parallel_codes.join(", "))
                };
                println!("│    {}. {} - {} (parallels: {})", i + 1, a.course_name, a.title, parallels);
            }
        }
        AIClassification::AssignmentInfo { course_name, title, parallel_codes, .. } => {
            let course_display = course_name.as_deref().unwrap_or("Unknown");
            let parallels = if parallel_codes.is_empty() {
                "N/A".to_string()
            } else {
                format!("[{}]", parallel_codes.join(", "))
            };
            println!("│\n│ ✅ Result   : Single assignment ({} - {}, parallels: {})", 
                course_display, title, parallels);
        }
        AIClassification::AssignmentUpdate { reference_keywords, parallel_codes, .. } => {
            let parallels = if parallel_codes.is_empty() {
                "N/A".to_string()
            } else {
                format!("[{}]", parallel_codes.join(", "))
            };
            println!("│\n│ ✅ Result   : Update detected (keywords: {:?}, parallels: {})", 
                reference_keywords, parallels);
        }
        AIClassification::Unrecognized { reason } => {
            let reason_display = reason.as_deref().unwrap_or("No reason provided");
            println!("│\n│ ℹ️  Result   : Unrecognized ({})", reason_display);
        }
    }
}