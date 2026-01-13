use crate::models::{AIClassification, Assignment};
use uuid::Uuid;
use serde_json::json;
use std::collections::HashMap;
use std::io::{Write, stdout};
use sqlx::PgPool;

use super::schedule_oracle::ScheduleOracle;
use once_cell::sync::Lazy;

use super::prompts::*;
use super::parsing::*;
use super::{GROQ_REASONING_MODELS, GROQ_VISION_MODELS, GROQ_TEXT_MODELS, GEMINI_MODELS};
use super::context_builder::build_context;

use std::sync::Mutex;

// A small global lock to serialize overwrite-style prints so concurrent println! calls
// from other tasks don't stomp the countdown/TRYING line.
static PRINT_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub static SCHEDULE_ORACLE: Lazy<ScheduleOracle> = Lazy::new(|| {
    ScheduleOracle::load_from_file("schedule.json")
        .expect("Failed to load schedule.json")
});

// ===== ANSI COLORS =====
const RESET: &str = "\x1b[0m";
const GRAY: &str = "\x1b[1;30m";
const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const BLUE: &str = "\x1b[34m";

// ===== RETRY CONFIGURATION =====
const MAX_RETRIES: u32 = 4;

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
    
    println!("\n{}┌── 🤖 AI PROCESSING ──────────────────────────{}", GRAY, RESET);
    
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
    
    println!("│ {}📝 Message{} : {}{}{}", CYAN, RESET, CYAN, message_truncated, RESET);
    
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
        
        println!("│ {}💬 Quoted{} : {}{}{}", MAGENTA, RESET, MAGENTA, quoted_truncated, RESET);
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
            
            println!("│");
            println!("│ {}✅ Context{} : Detected={} ({}), Schedules=[{}]",
                GREEN, RESET, parallel_summary, ctx.parallel_source, courses_summary);
            
            Some(ctx)
        }
        Err(e) => {
            println!("│");
            eprintln!("│ {}⚠️  Context failed{}: {}", YELLOW, RESET, e);
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
    
    println!("│ {}🤖 Stage 2{} : Extracting with AI...", BLUE, RESET);

    if image_base64.is_some() {
        println!("│ {}🖼️  Image{} : Attached (may be irrelevant meme)", MAGENTA, RESET);
    }
    println!("│ {}📊 Context{} : {} active assignments", CYAN, RESET, active_assignments.len());
    println!("│ {}📅 Time{} : {}", CYAN, RESET, current_datetime);
    println!("│");
    
    // Try all models with retries
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            retry_with_countdown(attempt).await;
        }
        
        // TIER 1: If image present, try Groq vision first
        if let Some(img) = image_base64 {
            match try_groq_vision(&prompt, img).await {
                Ok(classification) => {
                    match classification {
                        AIClassification::Unrecognized { reason, ..} => {
                            let reason_display = reason.as_deref().unwrap_or("No reason provided");
                            println!("│ {}ℹ️  Vision Result{}: Unrecognized ({})", BLUE, RESET, reason_display);
                            println!("│ {}🔄 Retrying{} with Gemini text-only...", YELLOW, RESET);
                            println!("│");
                            
                            match try_gemini_models(&prompt).await {
                                Ok(text_result) => {
                                    match text_result {
                                        AIClassification::Unrecognized { reason, category } => {
                                            println!("│ {}⚠️  Gemini{}: Still unrecognized", YELLOW, RESET);
                                            println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
                                            return Ok(AIClassification::Unrecognized { reason, category });
                                        }
                                        _ => {
                                            log_classification_success(&text_result);
                                            println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
                                            return Ok(text_result);
                                        }
                                    }
                                }
                                Err(_) => {
                                    // Continue to Groq fallback
                                }
                            }
                        }
                        _ => {
                            log_classification_success(&classification);
                            println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
                            return Ok(classification);
                        }
                    }
                }
                Err(_) => {
                    // Try Gemini text-only
                    match try_gemini_models(&prompt).await {
                        Ok(classification) => {
                            log_classification_success(&classification);
                            println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
                            return Ok(classification);
                        }
                        Err(_) => {
                            // Continue to Groq fallback
                        }
                    }
                }
            }
        } else {
            // TIER 1: No image - try Gemini first
            match try_gemini_models(&prompt).await {
                Ok(classification) => {
                    log_classification_success(&classification);
                    println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
                    return Ok(classification);
                }
                Err(_) => {
                    println!("│ {}🔄 Falling back{} to Groq...", YELLOW, RESET);
                    println!("│");
                }
            }
        }
        
        // TIER 2: Groq fallback (reasoning models)
        match try_groq_reasoning(&prompt).await {
            Ok(classification) => {
                log_classification_success(&classification);
                println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
                return Ok(classification);
            }
            Err(_) => {
                // All models failed this attempt
                if attempt < MAX_RETRIES - 1 {
                    println!("│ {}⚠️ All models failed{} - will retry ({}/{})", 
                             YELLOW, RESET, attempt + 1, MAX_RETRIES - 1);
                }
            }
        }
    }
    
    println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
    Err("All models failed after retries".to_string())
}

// ===== RETRY HELPERS =====

async fn retry_with_countdown(attempt: u32) {
    let delay = 10 * attempt as u64;

    // Print initial separator
    println!("│");
    
    // Print the first countdown line
    println!("│ {}⏳ RETRY #{}{} - Waiting {} seconds...", YELLOW, attempt, RESET, delay);

    // Countdown loop
    for remaining in (1..=delay).rev().skip(1) {
        // Small sleep first
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        
        // Lock, move up, clear line, print new countdown
        {
            let _guard = PRINT_LOCK.lock().unwrap();
            print!("\x1b[1A\x1b[2K│ {}⏳ RETRY #{}{} - Waiting {} seconds...\n", 
                   YELLOW, attempt, RESET, remaining);
            let _ = stdout().flush();
        }
    }

    // Wait final second
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Clear the countdown line
    {
        let _guard = PRINT_LOCK.lock().unwrap();
        print!("\x1b[1A\x1b[2K");
        let _ = stdout().flush();
    }

    println!("│");
}

// ===== CONTROLLED LOGGING =====

/// Maintain a TRYING-line overwriteable display within a loop.
/// - If `last_trying` is true we will move the cursor up, clear that line, and print the new TRYING.
/// - Otherwise we just print the TRYING line normally and set last_trying to true.
fn print_trying_line(model: &str, index: usize, total: usize, last_trying: &mut bool) {
    let formatted = format!("{}🔄 TRYING{} : {} ({} / {})", BLUE, RESET, model, index, total);

    // Acquire lock so we don't stomp a countdown or be stomped while printing.
    let _guard = PRINT_LOCK.lock().unwrap();

    if *last_trying {
        // Move up one line and clear it, then print the new trying line.
        // This preserves your visual "single-line TRYING" behaviour.
        print!("\x1b[1A\x1b[2K│ {}\n", formatted);
    } else {
        println!("│ {}", formatted);
        *last_trying = true;
    }
    stdout().flush().ok();

    // guard dropped at end of scope
}

/// Overwrite and remove the previous TRYING line (if any) without printing anything.
/// Useful when you want to silently move to the next TRYING line.
fn clear_previous_trying(last_trying: &mut bool) {
    if *last_trying {
        // Acquire lock so we don't clear a countdown in the middle of its tick
        let _guard = PRINT_LOCK.lock().unwrap();

        // Move up and clear the "│ ..." line
        print!("\x1b[1A\x1b[2K");
        stdout().flush().ok();
        *last_trying = false;

        // guard dropped
    }
}

// ===== GEMINI MODELS =====

async fn try_gemini_models(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let mut last_trying = false;
    let mut all_rate_limited = true;
    
    for (idx, model) in GEMINI_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GEMINI_MODELS.len(), &mut last_trying);
        
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
                all_rate_limited = false;
                clear_previous_trying(&mut last_trying);
                println!("│ {}❌ FAILED{} : {} - Network error", RED, RESET, model);
                if index < GEMINI_MODELS.len() {
                    continue;
                } else {
                    return Err(format!("Network error: {}", e));
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_previous_trying(&mut last_trying);
            if index < GEMINI_MODELS.len() {
                continue;
            } else {
                continue;
            }
        }
        
        if status.is_success() {
            //all_rate_limited = false;
            clear_previous_trying(&mut last_trying);
            println!("│ {}✅ SUCCESS{} : {} (Gemini {}/{})", GREEN, RESET, model, index, GEMINI_MODELS.len());
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_ai_text(&gemini_response)?;
            let classification = parse_classification(ai_text)?;
            
            return Ok(classification);
        }
        
        all_rate_limited = false;
        clear_previous_trying(&mut last_trying);
        println!("│ {}❌ FAILED{} : {} - HTTP {}", RED, RESET, model, status);
        if index < GEMINI_MODELS.len() {
            continue;
        }
    }
    
    if all_rate_limited {
        println!("│ {}⚠️ R-LIMIT{} : on all gemini models", YELLOW, RESET);
        return Err("rate limit".to_string());
    }
    
    Err("All Gemini models failed".to_string())
}

// ===== GROQ REASONING =====

async fn try_groq_reasoning(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    let mut last_trying = false;
    let mut all_rate_limited = true;
    
    for (idx, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_REASONING_MODELS.len(), &mut last_trying);
        stdout().flush().ok();
        
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
                all_rate_limited = false;
                clear_previous_trying(&mut last_trying);
                println!("│ {}❌ FAILED{} : {} - Network error", RED, RESET, model);
                if index < GROQ_REASONING_MODELS.len() {
                    continue;
                } else {
                    println!("│ {}🔄 Falling back{} to standard models...", YELLOW, RESET);
                    println!("│");
                    return try_groq_standard_text(prompt).await;
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_previous_trying(&mut last_trying);
            if index < GROQ_REASONING_MODELS.len() {
                continue;
            } else {
                continue;
            }
        }
        
        if status.is_success() {
            all_rate_limited = false;
            clear_previous_trying(&mut last_trying);
            println!("│ {}✅ SUCCESS{} : {} (Groq Reasoning {}/{})", GREEN, RESET, model, index, GROQ_REASONING_MODELS.len());
            
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
        
        all_rate_limited = false;
        clear_previous_trying(&mut last_trying);
        println!("│ {}❌ FAILED{} : {} - HTTP {}", RED, RESET, model, status);
        if index < GROQ_REASONING_MODELS.len() {
            continue;
        }
    }
    
    if all_rate_limited {
        println!("│ {}⚠️ R-LIMIT{} : on all Groq reasoning models", YELLOW, RESET);
        println!("│ {}🔄 Falling back{} to standard models...", YELLOW, RESET);
        println!("│");
        return try_groq_standard_text(prompt).await;
    }
    
    println!("│ {}🔄 Falling back{} to standard models...", YELLOW, RESET);
    println!("│");
    try_groq_standard_text(prompt).await
}

// ===== GROQ STANDARD TEXT =====

async fn try_groq_standard_text(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    let mut last_trying = false;
    
    for (idx, model) in GROQ_TEXT_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_TEXT_MODELS.len(), &mut last_trying);
        stdout().flush().ok();
        
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
                clear_previous_trying(&mut last_trying);
                println!("│ {}❌ FAILED{} : {} - Network error", RED, RESET, model);
                if index < GROQ_TEXT_MODELS.len() {
                    continue;
                } else {
                    return Err("All Groq standard models failed".to_string());
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_previous_trying(&mut last_trying);
            if index < GROQ_TEXT_MODELS.len() {
                continue;
            } else {
                return Err("rate limit".to_string());
            }
        }
        
        if status.is_success() {
            clear_previous_trying(&mut last_trying);
            println!("│ {}✅ SUCCESS{} : {} (Groq Standard {}/{})", GREEN, RESET, model, index, GROQ_TEXT_MODELS.len());
            
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
        
        clear_previous_trying(&mut last_trying);
        println!("│ {}❌ FAILED{} : {} - HTTP {}", RED, RESET, model, status);
        if index < GROQ_TEXT_MODELS.len() {
            continue;
        }
    }
    
    Err("All Groq standard models failed".to_string())
}

// ===== GROQ VISION =====

async fn try_groq_vision(prompt: &str, image_base64: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    let mut last_trying = false;
    
    for (idx, model) in GROQ_VISION_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_VISION_MODELS.len(), &mut last_trying);
        stdout().flush().ok();
        
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
                clear_previous_trying(&mut last_trying);
                println!("│ {}❌ FAILED{} : {} - Network error", RED, RESET, model);
                if index < GROQ_VISION_MODELS.len() {
                    continue;
                } else {
                    return Err("All Groq vision models failed".to_string());
                }
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_previous_trying(&mut last_trying);
            if index < GROQ_VISION_MODELS.len() {
                continue;
            } else {
                return Err("rate limit".to_string());
            }
        }
        
        if status.is_success() {
            clear_previous_trying(&mut last_trying);
            println!("│ {}✅ SUCCESS{} : {} (Vision {}/{})", GREEN, RESET, model, index, GROQ_VISION_MODELS.len());
            
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
        
        clear_previous_trying(&mut last_trying);
        println!("│ {}❌ FAILED{} : {} - HTTP {}", RED, RESET, model, status);
        if index < GROQ_VISION_MODELS.len() {
            continue;
        }
    }
    
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
    
    // Helper function to check if parallel codes overlap (handles "all" case)
    fn parallels_overlap(new_codes: &[String], existing_codes: &[String]) -> bool {
        // CRITICAL: Clean both arrays first - if either has "all", convert to ["all"]
        let clean_new = if new_codes.iter().any(|c| c.eq_ignore_ascii_case("all")) {
            vec!["all".to_string()]
        } else {
            new_codes.to_vec()
        };
        
        let clean_existing = if existing_codes.iter().any(|c| c.eq_ignore_ascii_case("all")) {
            vec!["all".to_string()]
        } else {
            existing_codes.to_vec()
        };
        
        // If either has "all", they overlap
        if clean_new.iter().any(|c| c == "all") || clean_existing.iter().any(|c| c == "all") {
            return true;
        }
        
        // If both are empty, consider them overlapping (no parallel restriction)
        if clean_new.is_empty() && clean_existing.is_empty() {
            return true;
        }
        
        // If one is empty and the other isn't, no overlap
        if clean_new.is_empty() || clean_existing.is_empty() {
            return false;
        }
        
        // Check for actual overlap
        clean_new.iter().any(|new_p| 
            clean_existing.iter().any(|existing_p| 
                new_p.eq_ignore_ascii_case(existing_p)
            )
        )
    }
    
    let filtered: Vec<&Assignment> = existing_assignments
        .iter()
        .filter(|a| {
            // 1. Must be same course
            let same_course = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|name| name.eq_ignore_ascii_case(course_name))
                .unwrap_or(false);
            
            if !same_course { return false; }
            
            // 2. Check parallel overlap (with "all" handling)
            if !parallels_overlap(parallel_codes, &a.parallel_codes) {
                return false;
            }
            
            // 3. Check numbers - if both have numbers, they must match
            let existing_numbers = extract_numbers(&a.title);
            if !new_numbers.is_empty() && !existing_numbers.is_empty() {
                if new_numbers != existing_numbers { 
                    return false; 
                }
            }
            
            // 4. Check assignment type - must match
            if let Some(ref new_t) = new_type {
                if let Some(existing_t) = extract_assignment_type(&a.title) {
                    if new_t != &existing_t { 
                        return false; 
                    }
                }
            }
            
            // 5. Check word overlap - basic similarity
            let similarity = calculate_word_overlap(title, &a.title);
            if similarity < 0.2 { 
                return false; 
            }
            
            true
        })
        .collect();
    
    if filtered.is_empty() {
        return Ok(None);
    }
    
    // Limit to top 3 most similar for AI check
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
    
    // RETRY LOGIC (same as classification)
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            retry_with_countdown(attempt).await;
        }
        
        // Try Gemini first
        match try_gemini_duplicate_check(&prompt).await {
            Ok(result) => return Ok(result),
            Err(e) if e == "rate limit" => {
                println!("{}🔄 Falling back{} to Groq for duplicate check...", YELLOW, RESET);
            }
            Err(_) => {}
        }
        
        // Fallback to Groq
        match try_groq_duplicate_check(&prompt).await {
            Ok(result) => return Ok(result),
            Err(_) => {
                if attempt < MAX_RETRIES - 1 {
                    println!("│ {}⚠️ Duplicate check failed{} - will retry ({}/{})", 
                             YELLOW, RESET, attempt + 1, MAX_RETRIES - 1);
                }
            }
        }
    }
    
    // After all retries exhausted, log critical error
    eprintln!("│ {}❌ CRITICAL{}: Duplicate check failed after {} retries", RED, RESET, MAX_RETRIES);
    Err("All duplicate check attempts failed".to_string())
}

async fn try_gemini_duplicate_check(prompt: &str) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set".to_string())?;
    
    let mut last_trying = false;
    let mut all_rate_limited = true;
    
    for (idx, model) in GEMINI_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GEMINI_MODELS.len(), &mut last_trying);
        
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
            Err(_) => {
                all_rate_limited = false;
                clear_previous_trying(&mut last_trying);
                println!("│ {}❌ FAILED{} : {} - Network error", RED, RESET, model);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_previous_trying(&mut last_trying);
            continue;
        }
        
        if status.is_success() {
            //all_rate_limited = false;
            clear_previous_trying(&mut last_trying);
            println!("{}✅ SUCCESS{} : {} (Gemini {}/{})", GREEN, RESET, model, index, GEMINI_MODELS.len());
            
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
                println!("│ {}⚠️  Low confidence duplicate{} - Reason: {}", YELLOW, RESET, result.reason);
            }
            
            return Ok(None);
        }
        
        all_rate_limited = false;
        clear_previous_trying(&mut last_trying);
        println!("│ {}❌ FAILED{} : {} - HTTP {}", RED, RESET, model, status);
    }
    
    if all_rate_limited {
        return Err("rate limit".to_string());
    }
    
    Err("All Gemini models failed".to_string())
}

// GROQ FALLBACK FOR DUPLICATION CHECK
async fn try_groq_duplicate_check(prompt: &str) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    let mut last_trying = false;
    
    for (idx, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_REASONING_MODELS.len(), &mut last_trying);
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.0,
            "max_completion_tokens": 1024,
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
                clear_previous_trying(&mut last_trying);
                println!("│ {}❌ FAILED{} : {} - Network error", RED, RESET, model);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_previous_trying(&mut last_trying);
            continue;
        }
        
        if status.is_success() {
            clear_previous_trying(&mut last_trying);
            println!("{}✅ SUCCESS{} : {} (Groq {}/{})", GREEN, RESET, model, index, GROQ_REASONING_MODELS.len());
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Parse error: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            let result: DuplicateCheckResult = serde_json::from_str(&ai_text)
                .map_err(|e| format!("JSON error: {}", e))?;
            
            if result.is_duplicate && result.confidence == "high" {
                if let Some(ref id_str) = result.matched_assignment_id {
                    if let Ok(uuid) = Uuid::parse_str(id_str) {
                        return Ok(Some(uuid));
                    }
                }
            } else if result.is_duplicate {
                println!("│ {}⚠️  Low confidence duplicate{} - Reason: {}", YELLOW, RESET, result.reason);
            }
            
            return Ok(None);
        }
        
        clear_previous_trying(&mut last_trying);
        println!("│ {}❌ FAILED{} : {} - HTTP {}", RED, RESET, model, status);
    }
    
    Err("All Groq models failed".to_string())
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
    
    println!("{}┌── 🤖 AI MATCHING ────────────────────────────{}", GRAY, RESET);
    println!("│ {}🔍 Keywords{} : {:?}", CYAN, RESET, keywords);
    
    if !parallel_codes.is_empty() {
        println!("│ {}🧩 Parallels{} : [{}]", MAGENTA, RESET, parallel_codes.join(", "));
    }
    println!("│");
    
    // RETRY LOGIC (same as classification and duplicate check)
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            retry_with_countdown(attempt).await;
        }
        
        // Try Gemini first
        match try_gemini_matching(&prompt).await {
            Ok(result) => {
                println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
                return Ok(result);
            }
            Err(e) if e == "rate limit" => {
                println!("│ {}🔄 Falling back{} to Groq for matching...", YELLOW, RESET);
            }
            Err(_) => {}
        }
        
        // Fallback to Groq
        match try_groq_matching(&prompt).await {
            Ok(result) => {
                println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
                return Ok(result);
            }
            Err(_) => {
                if attempt < MAX_RETRIES - 1 {
                    println!("│ {}⚠️ Matching failed{} - will retry ({}/{})", 
                             YELLOW, RESET, attempt + 1, MAX_RETRIES - 1);
                }
            }
        }
    }
    
    // After all retries exhausted
    eprintln!("│ {}❌ CRITICAL{}: Matching failed after {} retries", RED, RESET, MAX_RETRIES);
    println!("{}└──────────────────────────────────────────────{}", GRAY, RESET);
    Err("All matching attempts failed".to_string())
}

async fn try_gemini_matching(prompt: &str) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let mut last_trying = false;
    let mut all_rate_limited = true;
    
    for (idx, model) in GEMINI_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GEMINI_MODELS.len(), &mut last_trying);
        
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
                all_rate_limited = false;
                clear_previous_trying(&mut last_trying);
                println!("│ {}❌ FAILED{} : {} - Network error", RED, RESET, model);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_previous_trying(&mut last_trying);
            continue;
        }
        
        if status.is_success() {
            //all_rate_limited = false;
            clear_previous_trying(&mut last_trying);
            println!("│ {}✅ SUCCESS{} : {} (Gemini {}/{})", GREEN, RESET, model, index, GEMINI_MODELS.len());
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_ai_text(&gemini_response)?;
            let result = parse_match_result(ai_text)?;
            
            return Ok(result);
        }
        
        all_rate_limited = false;
        clear_previous_trying(&mut last_trying);
        println!("│ {}❌ FAILED{} : {} - HTTP {}", RED, RESET, model, status);
    }
    
    if all_rate_limited {
        return Err("rate limit".to_string());
    }
    
    Err("All Gemini models failed for matching".to_string())
}

// GROQ FALLBACK FOR MATCHING
async fn try_groq_matching(prompt: &str) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    let mut last_trying = false;
    
    for (idx, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_REASONING_MODELS.len(), &mut last_trying);
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.2,
            "max_completion_tokens": 4096,
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
                clear_previous_trying(&mut last_trying);
                println!("│ {}❌ FAILED{} : {} - Network error", RED, RESET, model);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_previous_trying(&mut last_trying);
            continue;
        }
        
        if status.is_success() {
            clear_previous_trying(&mut last_trying);
            println!("│ {}✅ SUCCESS{} : {} (Groq {}/{})", GREEN, RESET, model, index, GROQ_REASONING_MODELS.len());
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            let result = parse_match_result(&ai_text)?;
            
            return Ok(result);
        }
        
        clear_previous_trying(&mut last_trying);
        println!("│ {}❌ FAILED{} : {} - HTTP {}", RED, RESET, model, status);
    }
    
    Err("All Groq models failed for matching".to_string())
}

// ===== LOGGING HELPER =====

fn log_classification_success(classification: &AIClassification) {
    println!("│");
    match classification {
        AIClassification::MultipleAssignments { assignments, .. } => {
            println!("│ {}✅ Result{} : {} assignments detected", GREEN, RESET, assignments.len());
            for (i, a) in assignments.iter().enumerate() {
                let parallels = if a.parallel_codes.is_empty() {
                    "N/A".to_string()
                } else {
                    format!("[{}]", a.parallel_codes.join(", "))
                };
                println!("│    {}{}. {}{} - {} (parallels: {})", 
                    CYAN, i + 1, a.course_name, RESET, a.title, parallels);
            }
        }
        AIClassification::AssignmentInfo { course_name, title, parallel_codes, .. } => {
            let course_display = course_name.as_deref().unwrap_or("Unknown");
            let parallels = if parallel_codes.is_empty() {
                "N/A".to_string()
            } else {
                format!("[{}]", parallel_codes.join(", "))
            };
            println!("│ {}✅ Result{} : Single assignment ({} - {}, parallels: {})", 
                GREEN, RESET, course_display, title, parallels);
        }
        AIClassification::AssignmentUpdate { reference_keywords, parallel_codes, .. } => {
            let parallels = if parallel_codes.is_empty() {
                "N/A".to_string()
            } else {
                format!("[{}]", parallel_codes.join(", "))
            };
            println!("│ {}✅ Result{} : Update detected (keywords: {:?}, parallels: {})", 
                GREEN, RESET, reference_keywords, parallels);
        }
        AIClassification::Unrecognized { reason, category } => {
            use crate::models::UnrecognizedCategory;
            
            match category {
                UnrecognizedCategory::Informal => {
                    println!("│ {}ℹ️  Result{} : Informal chat (no academic context)", BLUE, RESET);
                }
                UnrecognizedCategory::AcademicRelated => {
                    let reason_display = reason.as_deref().unwrap_or("No reason provided");
                    println!("│ {}ℹ️  Result{} : Academic-related but not assignment ({})", 
                        BLUE, RESET, reason_display);
                }
            }
        }
    }
}
