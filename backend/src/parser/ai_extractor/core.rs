use crate::models::{AIClassification, Assignment};
use uuid::Uuid;
use serde_json::json;
use std::collections::HashMap;
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
) -> Result<AIClassification, String> {
    let current_datetime = get_current_datetime();
    let current_date = get_current_date();
    
    println!("\n\x1b[1;30m┌── 🤖 AI PROCESSING ──────────────────────────\x1b[0m");
    println!("│ 📝 Message  : \x1b[36m\"{}\"\x1b[0m", truncate_for_log(text, 60));
    
    // Show quoted message if present
    if let Some(quoted) = quoted_message {
        println!("│ 💬 Quoted   : \x1b[35m\"{}\"\x1b[0m", truncate_for_log(quoted, 60));
    }
    
    // STAGE 1: Build context with quoted message
    let context = match build_context(
        text, 
        sender_id, 
        pool, 
        &*SCHEDULE_ORACLE,
        quoted_message  
    ).await {
        Ok(ctx) => {
            // Build a detailed, compact context summary with per-parallel schedules
            let courses_summary = if ctx.course_hints.is_empty() {
                "none".to_string()
            } else {
                ctx.course_hints
                    .iter()
                    .map(|ch| {
                        if ch.parallel_schedules.is_empty() {
                            // No schedule info - show parallels if available
                            let parallel = if ch.parallel_codes.is_empty() {
                                "?".to_string()
                            } else {
                                format!("[{}]", ch.parallel_codes.join(","))
                            };
                            format!("{}:{}", ch.course_name, parallel)
                        } else {
                            // Has per-parallel schedules - show each parallel's meeting time
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
            
            let parallel_display = if ctx.parallel_codes.is_empty() {
                "none".to_string()
            } else {
                format!("[{}]", ctx.parallel_codes.join(", "))
            };
            
            println!("│\n│ ✅ Context  : Parallel={} ({}), Courses=[{}]",
                parallel_display, ctx.parallel_source, courses_summary);
            
            if let Some(ref quoted_summary) = ctx.quoted_message_summary {
                println!("│              Quoted: {}", quoted_summary);
            }
            
            Some(ctx)
        }
        Err(e) => {
            eprintln!("│\n│ ⚠️  Context failed: {}", e);
            None
        }
    };
    
    // Single prompt with optional context
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
    
    // TIER 1: If image present, try Groq vision first, then fallback to Gemini text-only
    if let Some(img) = image_base64 {
        match try_groq_vision(&prompt, img).await {
            Ok(classification) => {
                match classification {
                    AIClassification::Unrecognized => {
                        println!("│ ℹ️  Vision Result: Unrecognized (image likely irrelevant)");
                        println!("│ 🔄 Retrying with Gemini text-only...");
                        
                        match try_gemini_models(&prompt).await {
                            Ok(text_result) => {
                                match text_result {
                                    AIClassification::Unrecognized => {
                                        println!("│ ⚠️  Gemini: Still unrecognized");
                                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                                        return Ok(AIClassification::Unrecognized);
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
                
                match try_gemini_models(&prompt).await {
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
        match try_gemini_models(&prompt).await {
            Ok(classification) => {
                log_classification_success(&classification);
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Ok(classification);
            }
            Err(e) => {
                eprintln!("│ ⚠️  Gemini failed: {}", e);
                eprintln!("\n│ 🔄 Falling back to Groq...");
            }
        }
    }
    
    // TIER 2: Groq fallback (reasoning models)
    match try_groq_reasoning(&prompt).await {
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
    Err("All models failed".to_string())
}

// ===== GEMINI MODELS (PRIORITY) =====

async fn try_gemini_models(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        let request_body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": 4096,
                "responseMimeType": "application/json"
            }
        });
        
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, api_key
        );
        
        let client = reqwest::Client::new();
        let response = match client.post(&url).json(&request_body).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {} (Gemini {}/{})", model, index + 1, GEMINI_MODELS.len());
                eprintln!("│    Error: {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  R-LIMIT  : {} (Gemini {}/{})", model, index + 1, GEMINI_MODELS.len());
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Gemini models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : {} (Gemini {}/{})", model, index + 1, GEMINI_MODELS.len());
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_ai_text(&gemini_response)?;
            
            let classification = parse_classification(ai_text)?;
            
            return Ok(classification);
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} (Gemini {}/{}) - {}", model, index + 1, GEMINI_MODELS.len(), status);
        eprintln!("│    {}", truncate_for_log(&error_text, 60));
        
        if index < GEMINI_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Gemini models failed".to_string())
}

// ===== GROQ REASONING MODELS (FALLBACK) =====

async fn try_groq_reasoning(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
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
            Err(e) => {
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {} (Groq Reasoning {}/{})", model, index + 1, GROQ_REASONING_MODELS.len());
                eprintln!("│    Error: {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  R-LIMIT  : {} (Groq Reasoning {}/{})", model, index + 1, GROQ_REASONING_MODELS.len());
            if index < GROQ_REASONING_MODELS.len() - 1 {
                continue;
            } else {
                eprintln!("│ 🔄 Reasoning models exhausted, trying standard models...");
                return try_groq_standard_text(prompt).await;
            }
        }
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : {} (Groq Reasoning {}/{})", model, index + 1, GROQ_REASONING_MODELS.len());
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized) && !ai_text.contains("unrecognized") {
                eprintln!("│ ⚠️  Invalid JSON from Groq, trying next model");
                continue;
            }
            
            return Ok(classification);
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} (Groq Reasoning {}/{}) - {}", model, index + 1, GROQ_REASONING_MODELS.len(), status);
        eprintln!("│    {}", truncate_for_log(&error_text, 60));
        
        if index < GROQ_REASONING_MODELS.len() - 1 {
            continue;
        }
    }
    
    eprintln!("│ 🔄 All reasoning models failed, trying standard models...");
    try_groq_standard_text(prompt).await
}

// ===== GROQ STANDARD TEXT MODELS (FINAL FALLBACK) =====

async fn try_groq_standard_text(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_TEXT_MODELS.iter().enumerate() {
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.2,
            "max_tokens": 4096,
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
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {} (Groq Standard {}/{})", model, index + 1, GROQ_TEXT_MODELS.len());
                eprintln!("│    Error: {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  R-LIMIT  : {} (Groq Standard {}/{})", model, index + 1, GROQ_TEXT_MODELS.len());
            if index < GROQ_TEXT_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Groq standard models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            println!("│ \x1b[33m⚠️  STANDARD\x1b[0m : {} (Groq Standard {}/{})", model, index + 1, GROQ_TEXT_MODELS.len());
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized) && !ai_text.contains("unrecognized") {
                eprintln!("│ ⚠️  Invalid JSON, trying next model");
                continue;
            }
            
            return Ok(classification);
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} (Groq Standard {}/{}) - {}", model, index + 1, GROQ_TEXT_MODELS.len(), status);
        eprintln!("│    {}", truncate_for_log(&error_text, 60));
        
        if index < GROQ_TEXT_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Groq standard models failed".to_string())
}

// ===== GROQ VISION MODELS =====

async fn try_groq_vision(prompt: &str, image_base64: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_VISION_MODELS.iter().enumerate() {
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
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {} (Vision {}/{})", model, index + 1, GROQ_VISION_MODELS.len());
                eprintln!("│    Error: {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  R-LIMIT  : {} (Vision {}/{})", model, index + 1, GROQ_VISION_MODELS.len());
            if index < GROQ_VISION_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Groq vision models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : {} (Vision {}/{})", model, index + 1, GROQ_VISION_MODELS.len());
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized) && !ai_text.contains("unrecognized") {
                eprintln!("│ ⚠️  Invalid JSON from Groq, trying next model");
                continue;
            }
            
            return Ok(classification);
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} (Vision {}/{}) - {}", model, index + 1, GROQ_VISION_MODELS.len(), status);
        eprintln!("│    {}", truncate_for_log(&error_text, 60));
        
        if index < GROQ_VISION_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Groq vision models failed".to_string())
}

// ===== MATCHING (GEMINI ONLY) =====

pub async fn match_update_to_assignment(
    changes: &str,
    keywords: &[String],
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    parallel_codes: &[String],
) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let prompt = build_matching_prompt(changes, keywords, active_assignments, course_map, parallel_codes);
    
    println!("\x1b[1;30m┌── 🤖 AI MATCHING (GEMINI ONLY) ─────────────\x1b[0m");
    println!("│ 🔍 Keywords   : {:?}", keywords);
    
    if !parallel_codes.is_empty() {
        println!("│ 🧩 Parallels  : [{}]", parallel_codes.join(", "));
    }
    
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, api_key
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
        let response = match client.post(&url).json(&request_body).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("│ ❌ Failed     : {} (Attempt {}/{})", model, index + 1, GEMINI_MODELS.len());
                eprintln!("│    Error: {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  R-LIMIT   : {} (Attempt {}/{})", model, index + 1, GEMINI_MODELS.len());
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Err("All Gemini models rate limited for matching.".to_string());
            }
        }
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m    : {} (Attempt {}/{})", model, index + 1, GEMINI_MODELS.len());
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| e.to_string())?;
            let ai_text = extract_ai_text(&gemini_response)?;
            
            let result = parse_match_result(ai_text)?;
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            
            return Ok(result);
        }
        
        eprintln!("│ ❌ ERROR     : {} (Attempt {}/{}) - {}", model, index + 1, GEMINI_MODELS.len(), status);
        
        if index < GEMINI_MODELS.len() - 1 {
            continue;
        } else {
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            return Err(format!("AI matching failed with all models: {}", status));
        }
    }
    
    println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
    Err("No models available for matching".to_string())
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
    
    // ===== PRE-FILTERING =====
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
    
    // ===== AI CHECK =====
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set".to_string())?;
    
    let filtered_owned: Vec<Assignment> = filtered.into_iter().cloned().collect();
    let prompt = build_duplicate_detection_prompt(
        title,
        description,
        course_name,
        parallel_codes,
        &filtered_owned,
        course_map,
    );
    
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, api_key
        );
        
        let request_body = serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.0,
                "maxOutputTokens": 1024,
                "responseMimeType": "application/json"
            }
        });
        
        let client = reqwest::Client::new();
        let response = match client.post(&url).json(&request_body).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        
        let status = response.status();
        
        if status.is_success() {
            let gemini_response: super::parsing::GeminiResponse = response.json().await
                .map_err(|e| format!("Parse error: {}", e))?;
            
            let ai_text = extract_ai_text(&gemini_response)?;
            
            let result: DuplicateCheckResult = serde_json::from_str(ai_text)
                .map_err(|e| format!("JSON error: {}", e))?;
            
            if result.is_duplicate && result.confidence == "high" {
                if let Some(id_str) = result.matched_assignment_id {
                    if let Ok(uuid) = Uuid::parse_str(&id_str) {
                        println!("🔍 Duplicate detected: {} - Reason: {}", title, result.reason);
                        return Ok(Some(uuid));
                    }
                }
            } else if result.is_duplicate {
                println!("⚠️  Low confidence duplicate: {} - Reason: {}", title, result.reason);
            }
            
            return Ok(None);
        }
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS 
            && index < GEMINI_MODELS.len() - 1 {
            continue;
        }
        
        if index == GEMINI_MODELS.len() - 1 {
            return Err("All models failed".to_string());
        }
    }
    
    Err("No models available".to_string())
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
        AIClassification::Unrecognized => {
            println!("│\n│ ℹ️  Result   : Unrecognized");
        }
    }
}