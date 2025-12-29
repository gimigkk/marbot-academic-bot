use crate::models::{AIClassification, Assignment};
use uuid::Uuid;
use serde_json::json;
use std::collections::HashMap;

use super::prompts::{build_classification_prompt, build_matching_prompt};
use super::parsing::*;
use super::{GROQ_REASONING_MODELS, GROQ_VISION_MODELS, GROQ_TEXT_MODELS, GEMINI_MODELS};

// ===== MAIN AI EXTRACTION FUNCTION =====

pub async fn extract_with_ai(
    text: &str,
    available_courses: &str,
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    image_base64: Option<&str>,
) -> Result<AIClassification, String> {
    let current_datetime = get_current_datetime();
    let current_date = get_current_date();
    let prompt = build_classification_prompt(
        text, 
        available_courses, 
        active_assignments,
        course_map,
        &current_datetime, 
        &current_date
    );
    
    println!("\x1b[1;30m┌── 🤖 AI PROCESSING ──────────────────────────\x1b[0m");
    println!("│ 📝 Message  : \x1b[36m\"{}\"\x1b[0m", truncate_for_log(text, 60));
    if image_base64.is_some() {
        println!("│ 🖼️  Image    : Attached (may be irrelevant meme)");
    }
    println!("│ 📊 Context  : {} active assignments", active_assignments.len());
    println!("│ 📅 Time     : {}", current_datetime);
    
    // TIER 1: Try vision model if image present
    if let Some(img) = image_base64 {
        match try_groq_vision(&prompt, img).await {
            Ok(classification) => {
                match classification {
                    AIClassification::Unrecognized => {
                        println!("│ ℹ️  Vision Result: Unrecognized (image likely irrelevant)");
                        println!("│ 🔄 Retrying with text-only analysis...");
                        
                        // FALLBACK: Try reasoning models for text-only
                        match try_groq_reasoning(&prompt).await {
                            Ok(text_result) => {
                                match text_result {
                                    AIClassification::Unrecognized => {
                                        println!("│ ⚠️  Text-only: Still unrecognized");
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
                                eprintln!("│ ⚠️  Text fallback failed: {}", e);
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
                println!("│ 🔄 Trying text-only...");
                
                match try_groq_reasoning(&prompt).await {
                    Ok(classification) => {
                        log_classification_success(&classification);
                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                        return Ok(classification);
                    }
                    Err(e) => {
                        eprintln!("│ ⚠️  Text fallback failed: {}", e);
                    }
                }
            }
        }
    } else {
        // No image, use reasoning models directly
        match try_groq_reasoning(&prompt).await {
            Ok(classification) => {
                log_classification_success(&classification);
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Ok(classification);
            }
            Err(e) => {
                eprintln!("│ ⚠️  Groq Reasoning failed: {}", e);
                eprintln!("│ 🔄 Falling back to Gemini...");
            }
        }
    }
    
    // TIER 2: Gemini fallback
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        println!("│ 🔄 Model    : {} (Gemini Fallback {}/{})", model, index + 1, GEMINI_MODELS.len());
        
        match try_gemini_model(model, &prompt).await {
            Ok(classification) => {
                log_classification_success(&classification);
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Ok(classification);
            }
            Err(e) => {
                eprintln!("│ ❌ Failed   : {}", e);
                if index == GEMINI_MODELS.len() - 1 {
                    println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                    return Err("All models failed".to_string());
                }
            }
        }
    }
    
    println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
    Err("No models available".to_string())
}

// ===== GROQ REASONING MODELS (PRIORITY) =====

async fn try_groq_reasoning(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        println!("│ 🔄 Model    : {} (Reasoning {}/{})", model, index + 1, GROQ_REASONING_MODELS.len());
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.6,  // Reasoning models work better at 0.5-0.7
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
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : Groq Reasoning response");
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            println!("│ 📄 Result   : {}", truncate_for_log(&ai_text, 60));
            
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized) && !ai_text.contains("unrecognized") {
                eprintln!("│ ⚠️  Invalid JSON from Groq, trying next model");
                continue;
            }
            
            return Ok(classification);
        }
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  RATE LIMIT: {}", model);
            if index < GROQ_REASONING_MODELS.len() - 1 {
                continue;
            } else {
                eprintln!("│ 🔄 Reasoning models exhausted, trying standard models...");
                return try_groq_standard_text(prompt).await;
            }
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} - {}", status, truncate_for_log(&error_text, 60));
        
        if index < GROQ_REASONING_MODELS.len() - 1 {
            continue;
        }
    }
    
    eprintln!("│ 🔄 All reasoning models failed, trying standard models...");
    try_groq_standard_text(prompt).await
}

// ===== GROQ STANDARD TEXT MODELS (FALLBACK) =====

async fn try_groq_standard_text(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_TEXT_MODELS.iter().enumerate() {
        println!("│ 🔄 Model    : {} (Standard {}/{})", model, index + 1, GROQ_TEXT_MODELS.len());
        
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
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status.is_success() {
            println!("│ \x1b[33m⚠️  STANDARD\x1b[0m : Using non-reasoning model");
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            println!("│ 📄 Result   : {}", truncate_for_log(&ai_text, 60));
            
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized) && !ai_text.contains("unrecognized") {
                eprintln!("│ ⚠️  Invalid JSON, trying next model");
                continue;
            }
            
            return Ok(classification);
        }
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  RATE LIMIT: {}", model);
            if index < GROQ_TEXT_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Groq standard models rate limited".to_string());
            }
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} - {}", status, truncate_for_log(&error_text, 60));
        
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
        println!("│ 🔄 Model    : {} (Vision {}/{})", model, index + 1, GROQ_VISION_MODELS.len());
        
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
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : Groq Vision response");
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            println!("│ 📄 Result   : {}", truncate_for_log(&ai_text, 60));
            
            let classification = parse_classification(&ai_text)?;
            
            if matches!(classification, AIClassification::Unrecognized) && !ai_text.contains("unrecognized") {
                eprintln!("│ ⚠️  Invalid JSON from Groq, trying next model");
                continue;
            }
            
            return Ok(classification);
        }
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  RATE LIMIT: {}", model);
            if index < GROQ_VISION_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Groq vision models rate limited".to_string());
            }
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} - {}", status, truncate_for_log(&error_text, 60));
        
        if index < GROQ_VISION_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Groq vision models failed".to_string())
}

// ===== GEMINI FALLBACK =====

async fn try_gemini_model(model: &str, prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
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
    let response = client.post(&url).json(&request_body).send().await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    let status = response.status();
    
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("Rate limited".to_string());
    }
    
    if !status.is_success() {
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Status {}: {}", status, truncate_for_log(&error_text, 60)));
    }
    
    println!("│ \x1b[32m✅ SUCCESS\x1b[0m    : Gemini response");
    
    let gemini_response: GeminiResponse = response.json().await
        .map_err(|e| format!("Failed to deserialize: {}", e))?;
    
    let ai_text = extract_ai_text(&gemini_response)?;
    println!("│ 📄 Result   : {}", truncate_for_log(ai_text, 60));
    
    parse_classification(ai_text)
}

// ===== MATCHING (GEMINI ONLY) =====

pub async fn match_update_to_assignment(
    changes: &str,
    keywords: &[String],
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    parallel_code: Option<&str>,
) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let prompt = build_matching_prompt(changes, keywords, active_assignments, course_map, parallel_code);
    
    println!("\x1b[1;30m┌── 🤖 AI MATCHING (GEMINI ONLY) ─────────────\x1b[0m");
    println!("│ 🔍 Keywords   : {:?}", keywords);
    if let Some(pc) = parallel_code {
        println!("│ 🧩 Parallel   : {}", pc);
    }
    
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        println!("│ 🔄 Model      : {} (Attempt {}/{})", model, index + 1, GEMINI_MODELS.len());
        
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
                eprintln!("│ ❌ Failed   : {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m    : Match analysis complete");
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| e.to_string())?;
            let ai_text = extract_ai_text(&gemini_response)?;
            
            let result = parse_match_result(ai_text)?;
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            
            return Ok(result);
        }
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  RATE LIMIT: {}", model);
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Err("All Gemini models rate limited for matching.".to_string());
            }
        }
        
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

// ===== HELPERS =====

fn log_classification_success(classification: &AIClassification) {
    match classification {
        AIClassification::MultipleAssignments { assignments, .. } => {
            println!("│ ✅ Result: {} assignments detected", assignments.len());
            for (i, a) in assignments.iter().enumerate() {
                println!("│    {}. {} - {}", i + 1, a.course_name, a.title);
            }
        }
        AIClassification::AssignmentInfo { course_name, title, .. } => {
            let course_display = course_name.as_deref().unwrap_or("Unknown");
            println!("│ ✅ Result: Single assignment ({} - {})", course_display, title);
        }
        AIClassification::AssignmentUpdate { reference_keywords, .. } => {
            println!("│ ✅ Result: Update detected (keywords: {:?})", reference_keywords);
        }
        AIClassification::Unrecognized => {
            println!("│ ℹ️  Result: Unrecognized");
        }
    }
}
