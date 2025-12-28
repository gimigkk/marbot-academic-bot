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
    parallel_code: Option<&str>,  // ✅ ADDED PARAMETER
) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let prompt = build_matching_prompt(changes, keywords, active_assignments, course_map, parallel_code);
    
    println!("\x1b[1;30m┌── 🤖 AI MATCHING (GEMINI ONLY) ─────────────\x1b[0m");
    println!("│ 🔍 Keywords   : {:?}", keywords);
    if let Some(pc) = parallel_code {
        println!("│ 🧩 Parallel   : {}", pc);  // ✅ SHOW PARALLEL CODE
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
        r#"You are a bilingual (Indonesian/English) academic assistant that extracts structured assignment information from WhatsApp messages.

CONTEXT
═══════════════════════════════════════════════════════════════════
Current time (GMT+7): {}
Today's date: {}

Message: "{}"

Available courses:
{}

Active assignments (recent):
{}

TASK
═══════════════════════════════════════════════════════════════════
Classify this message as:
1. **NEW_ASSIGNMENT** - Announcing a new task
2. **UPDATE_ASSIGNMENT** - Modifying/clarifying existing assignment
3. **UNRECOGNIZED** - Not about assignments

CLASSIFICATION GUIDELINES
═══════════════════════════════════════════════════════════════════

NEW_ASSIGNMENT signals:
• "ada tugas baru", "new assignment", clear announcement
• Contains: course + deadline + description
• Sequential numbering not in DB (LKP 15 when only LKP 14 exists)

UPDATE_ASSIGNMENT patterns:
• Direct: "LKP 13 deadline berubah"
• Descriptive: "Tugas Pemrog yang [description]" - references existing work
• Clarification: "jadinya", "ternyata", "sebenarnya"
• Changes: "ganti", "diundur", "dimajuin", "revisi"

**Matching logic for updates:**
Use semantic understanding, not exact strings:
• "coding pake kertas" can match "Coding on Paper Assignment"
• Match by: course + identifying keywords (topic/number)
• If reasonable match in DB → UPDATE

UNRECOGNIZED:
• No course mentioned, social chat, vague references without context

PARALLEL CODES
═══════════════════════════════════════════════════════════════════
Valid codes (lowercase): k1, k2, k3, p1, p2, p3, all, null

• k1-k3, p1-p3: specific sections
• all: applies to all sections ("untuk semua parallel")
• null: not specified

Different codes = different assignments (K1 ≠ K2)

DATE PARSING (relative to {})
═══════════════════════════════════════════════════════════════════
• "hari ini"/"today" → {}
• "besok"/"tomorrow" → +1 day
• "lusa" → +2 days  
• "minggu depan" → +7 days
• Day names → next occurrence

Output: YYYY-MM-DD or null

OUTPUT FORMATS
═══════════════════════════════════════════════════════════════════

NEW_ASSIGNMENT:
{{"type":"assignment_info","course_name":"Pemrograman","title":"LKP 14","deadline":"2025-12-31","description":"Brief description","parallel_code":"k1"}}

UPDATE_ASSIGNMENT:
{{"type":"assignment_update","reference_keywords":["CourseName","identifier"],"changes":"what changed","new_deadline":"2025-12-30","new_title":null,"new_description":null,"parallel_code":"all"}}

UNRECOGNIZED:
{{"type":"unrecognized"}}

EXAMPLES
═══════════════════════════════════════════════════════════════════

Example 1 - Clear NEW:
"Ada tugas baru Pemrograman LKP 15 deadline minggu depan"
→ {{"type":"assignment_info","course_name":"Pemrograman","title":"LKP 15","deadline":"2026-01-04","description":"Programming assignment","parallel_code":null}}

Example 2 - Descriptive UPDATE:
"Tugas Pemrog yang coding pake kertas jadinya untuk semua parallel"
(DB has Pemrograman coding assignment)
→ {{"type":"assignment_update","reference_keywords":["Pemrograman","coding","kertas"],"changes":"scope changed to all parallel classes","new_deadline":null,"new_title":null,"new_description":null,"parallel_code":"all"}}

Example 3 - Different course, same topic = NEW:
"Pemrograman prototype deadline besok"
(DB has "UX Design prototype" only)
→ {{"type":"assignment_info","course_name":"Pemrograman","title":"Prototype","deadline":"2025-12-29","description":"Programming prototype","parallel_code":null}}

Example 4 - Too vague:
"deadline besok ya"
→ {{"type":"unrecognized"}}

PRINCIPLES
═══════════════════════════════════════════════════════════════════
1. **Semantic over literal**: Understand intent, not just keywords
2. **Context matters**: Use DB to inform decisions
3. **Confidence-based**: High confidence → classify; Low → UNRECOGNIZED
4. **Course boundaries**: Never match updates across different courses
5. **When uncertain**: NEW > UPDATE (avoid bad matches); Classification > UNRECOGNIZED (avoid noise)

Return ONLY valid JSON. No markdown, no explanations."#,
        current_datetime,
        current_date,
        text,
        available_courses,
        assignments_context,
        current_date,
        current_date
    )
}

/// Build the matching prompt for assignment updates
fn build_matching_prompt(
    changes: &str, 
    keywords: &[String], 
    assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    parallel_code: Option<&str>,  
) -> String {
    let assignments_list = assignments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let parallel_str = a.parallel_code
                .as_deref()
                .unwrap_or("N/A");
            
            let course_name = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown Course");
            
            let created_ago = Utc::now().signed_duration_since(a.created_at);
            let time_ago = if created_ago.num_minutes() < 60 {
                format!("{} min ago", created_ago.num_minutes())
            } else if created_ago.num_hours() < 24 {
                format!("{} hr ago", created_ago.num_hours())
            } else {
                format!("{} days ago", created_ago.num_days())
            };
            
            format!(
                "#{}: {} | {} | \"{}\" | Parallel: {} | {}",
                i + 1,
                a.id,
                course_name,
                a.title,
                parallel_str,
                time_ago
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let current_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
    
    let parallel_info = parallel_code
        .map(|pc| format!("Parallel code in update: {}", pc))
        .unwrap_or_else(|| "Parallel code: (not specified)".to_string());
    
    format!(
        r#"Match this update to an existing assignment.

CONTEXT
═══════════════════════════════════════════════════════════════════
Time: {}
Update: "{}"
Keywords: {:?}
{}

Assignments:
{}

TASK
═══════════════════════════════════════════════════════════════════
Find which assignment this update refers to, or return null if no match.

MATCHING STRATEGY
═══════════════════════════════════════════════════════════════════

Step 1: Course Filter
• First keyword = course name
• Only consider assignments from that course

Step 2: Semantic Content Match
• Match by MEANING, not exact strings
• "coding kertas" matches "Coding on Paper"
• "matriks" matches "Matrix Operations"
• Look for keywords in title/description

Step 3: Parallel Code Handling

**Two cases:**

A) **Scope Change** - Update is CHANGING parallel code
   Signals: "jadinya untuk [code]", "untuk semua", changes mention "scope"
   Strategy: IGNORE current parallel code, match by content only
   Why: The parallel code is what's being updated
   
B) **Parallel-Specific Update** - Update applies to specific parallel
   Signals: "[code] deadline [X]", no scope change language
   Strategy: Must match parallel code exactly
   Why: Update only applies to that section

For this update:
Changes: "{}"
Parallel: {}
→ If changes mention "scope" or update is "untuk [code]" → Case A
→ Otherwise → Case B

Step 4: Confidence
• Course + content match → HIGH
• Missing course → NULL
• Content mismatch → NULL
• Recency is tiebreaker only

OUTPUT FORMAT
═══════════════════════════════════════════════════════════════════

Match found:
{{"assignment_id":"uuid","confidence":"high","reason":"Course and content match"}}

No match:
{{"assignment_id":null,"confidence":"low","reason":"Why no match"}}

EXAMPLES
═══════════════════════════════════════════════════════════════════

Example 1 - Scope change (Case A):
Keywords: ["Pemrograman","coding","kertas"]
Changes: "scope changed to k2"
Parallel: k2
Assignment: Pemrograman "Coding on Paper" Parallel: k1

→ Match! (ignore parallel mismatch - it's being changed)
{{"assignment_id":"uuid-1","confidence":"high","reason":"Course and content match, scope being changed to k2"}}

Example 2 - Parallel-specific (Case B):
Keywords: ["Pemrograman","LKP 13"]
Changes: "deadline extended"
Parallel: k2
Assignments:
  - Pemrograman "LKP 13" Parallel: k1
  - Pemrograman "LKP 13" Parallel: k2

→ Match #2 (must match parallel for Case B)
{{"assignment_id":"uuid-2","confidence":"high","reason":"Course, content, and parallel all match"}}

Example 3 - No match:
Keywords: ["Pemrograman","matriks"]
Assignments: UX Design "Prototype"

→ No match (wrong course)
{{"assignment_id":null,"confidence":"low","reason":"No Pemrograman assignments found"}}

PRINCIPLES
═══════════════════════════════════════════════════════════════════
• Think like a human: "Tugas X jadinya untuk K2" = find X, change its parallel to K2
• Semantic matching: meaning > exact words
• Course boundaries: never match across courses
• Recency helps but doesn't override content mismatch

Return ONLY valid JSON."#,
        current_time,
        changes,
        keywords,
        parallel_info,
        assignments_list,
        changes,
        parallel_code.unwrap_or("not specified")
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