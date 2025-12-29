use crate::models::{AIClassification, Assignment};
use uuid::Uuid;
use serde_json::json;
use std::collections::HashMap;

use super::prompts::{build_classification_prompt, build_matching_prompt};
use super::parsing::*;
use super::{GROQ_VISION_MODELS, GROQ_TEXT_MODELS, GEMINI_MODELS};

// ===== MAIN AI EXTRACTION FUNCTION =====

/// Extract structured info from WhatsApp message
/// Strategy: Groq first (fast, good context), Gemini fallback (huge context, strong reasoning)
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
                        // This is EXPECTED behavior when image is an irrelevant meme
                        println!("│ ℹ️  Vision Result: Unrecognized (image likely irrelevant)");
                        println!("│ 🔄 Retrying with text-only analysis...");
                        
                        // FALLBACK: Process text-only since image was distracting
                        match try_groq_text(&prompt).await {
                            Ok(text_result) => {
                                match text_result {
                                    AIClassification::Unrecognized => {
                                        // Both failed - truly unrecognized
                                        println!("│ ⚠️  Text-only: Still unrecognized");
                                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                                        return Ok(AIClassification::Unrecognized);
                                    }
                                    _ => {
                                        // Text-only succeeded! Image was just a distraction
                                        println!("│ ✅ Text-only: Assignment detected (meme was distraction)");
                                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                                        return Ok(text_result);
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("│ ⚠️  Text fallback failed: {}", e);
                                // Continue to Gemini
                            }
                        }
                    }
                    _ => {
                        // Vision found something useful (image had real info)
                        println!("│ ✅ Vision: Assignment extracted from image+text");
                        println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                        return Ok(classification);
                    }
                }
            }
            Err(e) => {
                eprintln!("│ ⚠️  Vision model error: {}", e);
                println!("│ 🔄 Trying text-only...");
                
                // Try text-only before Gemini
                match try_groq_text(&prompt).await {
                    Ok(classification) => {
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
        // No image, standard text processing
        match try_groq_text(&prompt).await {
            Ok(classification) => {
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Ok(classification);
            }
            Err(e) => {
                eprintln!("│ ⚠️  Groq Text failed: {}", e);
                eprintln!("│ 🔄 Falling back to Gemini...");
            }
        }
    }
    
    // TIER 2: Gemini fallback
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        println!("│ 🔄 Model    : {} (Gemini Fallback {}/{})", model, index + 1, GEMINI_MODELS.len());
        
        match try_gemini_model(model, &prompt).await {
            Ok(classification) => {
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

// ===== GROQ IMPLEMENTATION =====

/// Try Groq vision models with fallback
async fn try_groq_vision(prompt: &str, image_base64: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_VISION_MODELS.iter().enumerate() {
        println!("│ 🔄 Model    : {} (Vision {}/{})", model, index + 1, GROQ_VISION_MODELS.len());
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": prompt
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:image/jpeg;base64,{}", image_base64)
                            }
                        }
                    ]
                }
            ],
            "temperature": 0.2,
            "max_tokens": 4096,
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
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : Groq Vision response");
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize Groq response: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            println!("│ 📄 Result   : {}", truncate_for_log(&ai_text, 60));
            
            let classification = parse_classification(&ai_text)?;
            
            // Validate JSON quality
            if matches!(classification, AIClassification::Unrecognized) && !ai_text.contains("unrecognized") {
                eprintln!("│ ⚠️  Invalid JSON from Groq, trying next model");
                continue;
            }
            
            return Ok(classification);
        }
        
        // Handle rate limit
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

/// Try Groq text models with fallback
async fn try_groq_text(prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;
    
    for (index, model) in GROQ_TEXT_MODELS.iter().enumerate() {
        println!("│ 🔄 Model    : {} (Text {}/{})", model, index + 1, GROQ_TEXT_MODELS.len());
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.2,
            "max_tokens": 4096,
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
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : Groq Text response");
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize Groq response: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            println!("│ 📄 Result   : {}", truncate_for_log(&ai_text, 60));
            
            let classification = parse_classification(&ai_text)?;
            
            // Validate JSON quality
            if matches!(classification, AIClassification::Unrecognized) && !ai_text.contains("unrecognized") {
                eprintln!("│ ⚠️  Invalid JSON from Groq, trying next model");
                continue;
            }
            
            return Ok(classification);
        }
        
        // Handle rate limit
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  RATE LIMIT: {}", model);
            if index < GROQ_TEXT_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Groq text models rate limited".to_string());
            }
        }
        
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("│ ❌ ERROR    : {} - {}", status, truncate_for_log(&error_text, 60));
        
        if index < GROQ_TEXT_MODELS.len() - 1 {
            continue;
        }
    }
    
    Err("All Groq text models failed".to_string())
}

// ===== GEMINI IMPLEMENTATION =====

/// Try a single Gemini model
async fn try_gemini_model(model: &str, prompt: &str) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );
    
    let request_body = json!({
        "contents": [{
            "parts": [{
                "text": prompt
            }]
        }],
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

// ===== MATCH WITH EXISTING ASSIGNMENT (GEMINI ONLY) =====

/// Use AI to match an update to a specific assignment
/// Uses ONLY Gemini models (needs the 1M context for complex matching logic)
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
    
    // Use ONLY Gemini for matching (needs better reasoning + 1M context)
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        println!("│ 🔄 Model      : {} (Attempt {}/{})", model, index + 1, GEMINI_MODELS.len());
        
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, api_key
        );
        
        let request_body = json!({
            "contents": [{
                "parts": [{
                    "text": prompt
                }]
            }],
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
            
            // Parse result BEFORE closing the box (so confidence/reason appear inside)
            let result = parse_match_result(ai_text)?;
            
            // Now close the box after all output
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            
            return Ok(result);
        }
        
        // Handle rate limit
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("│ ⚠️  RATE LIMIT: {}", model);
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Err("All Gemini models rate limited for matching.".to_string());
            }
        }
        
        // Try next model
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