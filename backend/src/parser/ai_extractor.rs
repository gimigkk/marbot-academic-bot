use crate::models::{AIClassification, Assignment};  
use uuid::Uuid;  
use serde::Deserialize;
use serde_json::json;
use chrono::{Utc, FixedOffset};
use std::collections::HashMap;

/// Helper to build course_map from assignments and a course lookup
/// This is a convenience function - you can also pass the map directly
pub fn build_course_map_from_db_results(
    courses: &[(Uuid, String)]  // List of (course_id, course_name) tuples
) -> HashMap<Uuid, String> {
    courses.iter()
        .map(|(id, name)| (*id, name.clone()))
        .collect()
}

// ===== MODEL CONFIGURATION =====

// Groq models (frontline - fast, vision-capable, high rate limits)
// Context windows: Scout 128K, Maverick 128K, Llama 3.3 128K, Mixtral 32K
const GROQ_VISION_MODELS: &[&str] = &[
    "meta-llama/llama-4-scout-17b-16e-instruct",      // 128K context, fast multimodal
    "meta-llama/llama-4-maverick-17b-128e-instruct",  // 128K context, powerful
];

const GROQ_TEXT_MODELS: &[&str] = &[
    "llama-3.3-70b-versatile",  // 128K context
    "mixtral-8x7b-32768",       // 32K context
];

// Gemini models (fallback - reliable, 1M context window for complex reasoning)
const GEMINI_MODELS: &[&str] = &[
    "gemini-3-flash-preview",   // 1M context
    "gemini-2.5-flash",         // 1M context
    "gemini-2.5-flash-lite",    // 1M context
];

// ===== API RESPONSE STRUCTURES =====

// Groq response structure
#[derive(Debug, Deserialize)]
struct GroqResponse {
    choices: Vec<GroqChoice>,
}

#[derive(Debug, Deserialize)]
struct GroqChoice {
    message: GroqMessage,
}

#[derive(Debug, Deserialize)]
struct GroqMessage {
    content: String,
}

// Gemini response structure
#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Content,
}

#[derive(Debug, Deserialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
struct Part {
    text: String,
}

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
// We keep Gemini-only for matching because it needs the bigger brain
// for complex reasoning about temporal context and semantic matching

/// Use AI to match an update to a specific assignment
/// Uses ONLY Gemini models (needs the 1M context for complex matching logic)
pub async fn match_update_to_assignment(
    changes: &str,
    keywords: &[String],
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let prompt = build_matching_prompt(changes, keywords, active_assignments, course_map);
    
    println!("\x1b[1;30m┌── 🤖 AI MATCHING (GEMINI ONLY) ─────────────\x1b[0m");
    println!("│ 🔍 Keywords   : {:?}", keywords);
    
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

// ===== PROMPT BUILDERS =====

/// Build assignment context list for the prompt
/// Limit to 100 most recent to stay within token budgets
fn build_context_assignments_list(
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>
) -> String {
    if active_assignments.is_empty() {
        return "No active assignments in database.".to_string();
    }
    
    // Take up to 100 most recent assignments (sorted by created_at desc in query)
    let assignments_to_show = active_assignments.iter().take(100);
    let count = active_assignments.len().min(100);
    
    let list = assignments_to_show
        .map(|a| {
            let deadline = a.deadline
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "No deadline".to_string());
            let parallel = a.parallel_code.as_deref().unwrap_or("N/A");
            
            // Get course name from map, fallback to "Unknown Course" if not found
            let course_name = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown Course");
            
            format!(
                "- Course: {}, Title: \"{}\", Deadline: {}, Parallel: {}, Desc: \"{}\"",
                course_name,
                a.title,
                deadline,
                parallel,
                truncate_for_log(&a.description, 80)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    if active_assignments.len() > 100 {
        format!("{}\n(Showing {} most recent out of {} total active assignments)", 
            list, count, active_assignments.len())
    } else {
        list
    }
}

/// Build the classification prompt for AI models
fn build_classification_prompt(
    text: &str, 
    available_courses: &str, 
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    current_datetime: &str, 
    current_date: &str
) -> String {
    let assignments_context = build_context_assignments_list(active_assignments, course_map);
    
    format!(
        r#"Classify this WhatsApp academic message. Return ONLY valid JSON, NO markdown.

CURRENT DATE/TIME (GMT+7): {}
TODAY'S DATE: {}

Available courses with their aliases:
{}

ACTIVE ASSIGNMENTS IN DATABASE (for context - check before classifying as new):
{}

Message: "{}"

Output ONE of these exact formats:

NEW assignment (ONLY if this is clearly a FIRST mention NOT in database above):
{{"type":"assignment_info","course_name":"Pemrograman","title":"Tugas Bab 2","deadline":"2025-12-31","description":"Brief description","parallel_code":"K1"}}

UPDATE to existing assignment (if referencing an assignment that EXISTS in database above):
{{"type":"assignment_update","reference_keywords":["pemrograman","bab 2"],"changes":"deadline moved to 2025-12-05","new_deadline":"2025-12-05","new_title":null,"new_description":null,"parallel_code":"K1"}}

Other/unclear:
{{"type":"unrecognized"}}

CRITICAL CLASSIFICATION RULES:
1. **Check ACTIVE ASSIGNMENTS FIRST**: Before classifying as "assignment_info", verify if similar assignment already exists
   - Match by: course name + assignment title/topic + parallel code
   - If found → classify as "assignment_update"
   - If NOT found → classify as "assignment_info"

2. **Reference Keywords for Updates**: Use 2-4 specific keywords that identify the assignment
   - MUST include: course name/alias
   - SHOULD include: specific topic/chapter/week number
   - Example: ["pemrograman", "bab 2"] or ["GKV", "matriks", "pertemuan 4"]

3. **Parallel Code Handling**:
   - ALWAYS include parallel_code in BOTH assignment_info AND assignment_update responses
   - Format: K1, K2, K3, P1, P2, P3 (uppercase K/P + number)
   - If mentioned in message, extract it
   - If not mentioned but can be inferred from existing assignment, include it
   - If unknown, set to null

4. **Title Improvements**: If updating generic title + message has specific details + current title contradicts the update
   - Generic titles: "Tugas baru", "Assignment", "Tugas", "New task"
   - Set "new_title" to descriptive title from update details
   - Example: "Tugas baru" + update mentions "figma prototype" → new_title: "Figma Prototype Pertemuan 4"

5. **Recurring Assignments** (LKP, Lab, Weekly tasks):
   - Check if assignment with same pattern exists (e.g., "LKP 13", "Lab Week 5")
   - If EXISTS → "assignment_update"
   - If NOT EXISTS + clear new assignment language → "assignment_info"
   - If UNCLEAR → prefer "assignment_update" for known recurring patterns

6. **Date Handling**: Convert relative dates to YYYY-MM-DD
   - "hari ini" (today) = {}
   - "besok" (tomorrow) = add 1 day
   - "minggu depan" (next week) = add 7 days
   - "senin/selasa/rabu/etc" = next occurrence of that weekday
   - "kemarin" (yesterday) = minus 1 day

7. **Course Name Normalization**:
   - Always use FORMAL course name in "course_name" field
   - Match aliases case-insensitively: pemrog → Pemrograman, GKV → Grafika Komputer dan Visualisasi

8. **Image Handling**: If message has image, extract ALL visible text and integrate into analysis

EXAMPLES:
Message: "LKP 13 deadline tonight K1"
- Check database: Found "LKP 13" with deadline 2025-12-30
- Result: {{"type":"assignment_update","reference_keywords":["pemrograman","LKP 13"],"changes":"deadline changed to tonight","new_deadline":"2025-12-26","new_title":null,"new_description":null,"parallel_code":"K1"}}

Message: "LKP 14 due next Monday K2"  
- Check database: No "LKP 14" found
- Result: {{"type":"assignment_info","course_name":"Pemrograman","title":"LKP 14","deadline":"2025-12-29","description":"Weekly programming assignment","parallel_code":"K2"}}

Message: "Tugas baru deadline besok" (existing generic title "Tugas baru" in DB with K1)
- Check database: Found generic "Tugas baru" with parallel_code K1
- No specific details to improve title
- Result: {{"type":"assignment_update","reference_keywords":["pemrograman","tugas"],"changes":"deadline set to tomorrow","new_deadline":"2025-12-27","new_title":null,"new_description":null,"parallel_code":"K1"}}

Message: "Itu figma prototype pertemuan 4 ya K3" (existing title "Tugas baru" in DB)
- Check database: Found generic "Tugas baru" 
- Update has specific details and parallel code
- Result: {{"type":"assignment_update","reference_keywords":["desain","tugas"],"changes":"clarified as figma prototype for meeting 4","new_deadline":null,"new_title":"Figma Prototype Pertemuan 4","new_description":"Figma prototype assignment for meeting 4","parallel_code":"K3"}}

Return ONLY valid JSON, no explanations or markdown formatting."#,
        current_datetime,
        current_date,
        available_courses,
        assignments_context,
        text,
        current_date
    )
}

/// Build the matching prompt with temporal context
fn build_matching_prompt(
    changes: &str, 
    keywords: &[String], 
    assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>
) -> String {
    let assignments_list = assignments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let deadline_str = a.deadline
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or("No deadline".to_string());
            
            let parallel_str = a.parallel_code
                .as_deref()
                .unwrap_or("N/A");
            
            // Get course name from map
            let course_name = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown Course");
            
            // Calculate how long ago this assignment was created
            let created_ago = Utc::now().signed_duration_since(a.created_at);
            let time_ago = if created_ago.num_minutes() < 60 {
                format!("{} minutes ago", created_ago.num_minutes())
            } else if created_ago.num_hours() < 24 {
                format!("{} hours ago", created_ago.num_hours())
            } else {
                format!("{} days ago", created_ago.num_days())
            };
            
            format!(
                "Assignment #{}:\n  ID: {}\n  Course: {}\n  Title: \"{}\"\n  Description: \"{}\"\n  Deadline: {}\n  Parallel: {}\n  ⏱️ Created: {} ({})",
                i + 1,
                a.id,
                course_name,
                a.title,
                truncate_for_log(&a.description, 100),
                deadline_str,
                parallel_str,
                a.created_at.format("%Y-%m-%d %H:%M:%S"),
                time_ago
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    
    // Get current time for context
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let current_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
    
    format!(
        r#"You are helping match an update message to the correct assignment in a database.

CURRENT TIME (GMT+7): {}

UPDATE MESSAGE: "{}"
REFERENCE KEYWORDS: {:?}

AVAILABLE ASSIGNMENTS (sorted by recency, newest first):
{}

TASK:
Match this update to the most likely assignment. Consider TEMPORAL CONTEXT as a key factor.

MATCHING CRITERIA (in order of importance):
1. TEMPORAL PROXIMITY: If an assignment was created minutes/hours ago, it's very likely the one being updated
   - Messages sent within hours of assignment creation are HIGHLY likely to be updates
   - Example: Assignment created 5 minutes ago + update message → VERY HIGH probability it's the same assignment
2. KEYWORD MATCH: Keywords mention course name AND assignment topic
3. CONTENT MATCH: Title/description contains topics from keywords
4. PARALLEL CODE: Matches if mentioned in update
5. DEADLINE CONTEXT: Makes sense with any deadline changes mentioned

IMPORTANT TEMPORAL RULES:
- If update message is sent within 30 minutes of assignment creation → HIGH confidence match (user is clarifying)
- Recent assignments (< 1 hour old) should be prioritized over older ones
- Generic titles like "Tugas baru" + recent creation time = likely match for clarification messages

RESPONSE FORMAT:
Return JSON with:
- "assignment_id": the UUID if confident match found, null otherwise
- "confidence": "high" or "low"
- "reason": explain your reasoning (mention temporal context if relevant)

EXAMPLES:
✅ HIGH confidence (temporal):
- Assignment created 5 minutes ago + Keywords ["pertemuan 4"] + changes clarify description → MATCH (user is adding details)
- Generic title "Tugas baru" created recently + specific update → MATCH (clarification of vague initial message)

✅ HIGH confidence (keyword):
- Keywords ["pemrograman", "bab 2"] + Assignment Title "Tugas Bab 2" → MATCH
- Keywords ["GKV", "matriks"] + Assignment Description contains "matriks" → MATCH

❌ LOW confidence:
- Keywords too generic, multiple assignments match
- Assignment is old (> 1 week) and keywords don't match well
- No clear connection between update and available assignments

Return ONLY valid JSON, no markdown:
{{"assignment_id": "uuid-here", "confidence": "high", "reason": "Assignment created 5 minutes ago, user is clarifying 'pertemuan 4' detail"}}
or
{{"assignment_id": null, "confidence": "low", "reason": "No recent assignments match the keywords"}}"#,
        current_time,
        changes, 
        keywords, 
        assignments_list
    )
}

// ===== RESPONSE EXTRACTORS =====

/// Extract text from Groq response
fn extract_groq_text(groq_response: &GroqResponse) -> Result<String, String> {
    groq_response
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .ok_or_else(|| "Groq returned empty response".to_string())
}

/// Extract AI text from Gemini response structure
fn extract_ai_text(gemini_response: &GeminiResponse) -> Result<&str, String> {
    gemini_response
        .candidates
        .first()
        .and_then(|candidate| candidate.content.parts.first())
        .map(|part| part.text.as_str())
        .ok_or_else(|| "Gemini returned empty response".to_string())
}

// ===== PARSERS =====

/// Clean and parse the AI classification from text
fn parse_classification(ai_text: &str) -> Result<AIClassification, String> {
    let cleaned = ai_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    
    if !is_valid_json_object(cleaned) {
        eprintln!("⚠️  Response is not a valid JSON object");
        return Ok(AIClassification::Unrecognized);
    }
    
    match serde_json::from_str::<AIClassification>(cleaned) {
        Ok(classification) => Ok(classification),
        Err(e) => {
            eprintln!("❌ JSON parse error: {}", e);
            eprintln!("   Tried to parse: {}", cleaned);
            Ok(AIClassification::Unrecognized)
        }
    }
}

/// Clean and parse AI matching for update
fn parse_match_result(ai_text: &str) -> Result<Option<Uuid>, String> {
    let cleaned = ai_text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    
    #[derive(Deserialize)]
    struct MatchResult {
        assignment_id: Option<String>,
        confidence: String,
        #[serde(default)]
        reason: Option<String>,
    }
    
    match serde_json::from_str::<MatchResult>(cleaned) {
        Ok(result) => {
            println!("│ 🔍 Confidence : {}", result.confidence);
            if let Some(ref reason) = result.reason {
                println!("│ 📝 Reason     : {}", truncate_for_log(reason, 60));
            }
            
            if result.confidence == "high" {
                if let Some(id_str) = result.assignment_id {
                    Ok(Some(Uuid::parse_str(&id_str).map_err(|e| e.to_string())?))
                } else {
                    Ok(None)
                }
            } else {
                println!("│ ⚠️ Low confidence match");
                Ok(None)
            }
        }
        Err(e) => {
            eprintln!("│ ❌ Failed to parse match result: {}", e);
            Ok(None)
        }
    }
}

// ===== HELPERS =====

/// Get current datetime in GMT+7
fn get_current_datetime() -> String {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    now.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Get current date in GMT+7
fn get_current_date() -> String {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    now.format("%Y-%m-%d").to_string()
}

/// Check if string looks like a valid JSON object
fn is_valid_json_object(s: &str) -> bool {
    s.starts_with('{') 
        && s.ends_with('}') 
        && s.matches('{').count() == s.matches('}').count()
}

/// Truncate text for logging
fn truncate_for_log(text: &str, max_len: usize) -> String {
    let clean_text = text.replace('\n', " ");
    if clean_text.len() <= max_len {
        clean_text
    } else {
        format!("{}...", &clean_text[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_json_object() {
        assert!(is_valid_json_object(r#"{"type":"unrecognized"}"#));
        assert!(is_valid_json_object(r#"{"a":{"b":"c"}}"#));
        assert!(!is_valid_json_object(r#"{"type":"incomplete"#));
        assert!(!is_valid_json_object(r#"not json"#));
        assert!(!is_valid_json_object(r#"["array"]"#));
    }

    #[test]
    fn test_truncate_for_log() {
        assert_eq!(truncate_for_log("short", 10), "short");
        assert_eq!(truncate_for_log("this is a very long text", 10), "this is a ...");
    }
}