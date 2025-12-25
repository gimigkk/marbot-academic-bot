use crate::models::{AIClassification, Assignment};  
use uuid::Uuid;  
use serde::Deserialize;
use serde_json::json;
use chrono::{Utc, FixedOffset};

// ===== MODEL CONFIGURATION =====

// Groq models (frontline - fast, vision-capable, high rate limits)
const GROQ_VISION_MODELS: &[&str] = &[
    "meta-llama/llama-4-scout-17b-16e-instruct",      // NEW: Llama 4 Scout (fast, multimodal)
    "meta-llama/llama-4-maverick-17b-128e-instruct",  // NEW: Llama 4 Maverick (powerful)
];

const GROQ_TEXT_MODELS: &[&str] = &[
    "llama-3.3-70b-versatile",
    "mixtral-8x7b-32768",
];

// Gemini models (fallback - reliable, good for structured data)
const GEMINI_MODELS: &[&str] = &[
    "gemini-3-flash-preview",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
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
/// Uses Groq (vision/text) as frontline, Gemini as fallback
pub async fn extract_with_ai(
    text: &str,
    available_courses: &str,
    image_base64: Option<&str>, // Base64 encoded image (if present)
) -> Result<AIClassification, String> {
    let current_datetime = get_current_datetime();
    let current_date = get_current_date();
    let prompt = build_classification_prompt(text, available_courses, &current_datetime, &current_date);
    
    println!("\x1b[1;30m┌── 🤖 AI PROCESSING ──────────────────────────\x1b[0m");
    println!("│ 📝 Message  : \x1b[36m\"{}\"\x1b[0m", truncate_for_log(text, 60));
    if image_base64.is_some() {
        println!("│ 🖼️  Image   : Detected");
    }
    println!("│ 📅 Time     : {}", current_datetime);
    
    // TIER 1: Try Groq first (vision if image, text otherwise)
    if let Some(img) = image_base64 {
        match try_groq_vision(&prompt, img).await {
            Ok(classification) => {
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Ok(classification);
            }
            Err(e) => {
                eprintln!("│ ⚠️  Groq Vision failed: {}", e);
                eprintln!("│ 🔄 Falling back to Gemini...");
            }
        }
    } else {
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
    
    // TIER 2: Fallback to Gemini models
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
                    return Err("All models (Groq + Gemini) failed".to_string());
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
    
    println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : Gemini response");
    
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
/// Uses ONLY Gemini models (needs the bigger brain for complex matching)
pub async fn match_update_to_assignment(
    changes: &str,
    keywords: &[String],
    active_assignments: &[Assignment],
) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let prompt = build_matching_prompt(changes, keywords, active_assignments);
    
    println!("\x1b[1;30m┌── 🤖 AI MATCHING (GEMINI ONLY) ─────────────\x1b[0m");
    println!("│ 🔍 Keywords : {:?}", keywords);
    
    // Use ONLY Gemini for matching (needs better reasoning)
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        println!("│ 🔄 Model    : {} (Attempt {}/{})", model, index + 1, GEMINI_MODELS.len());
        
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
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : Match analysis complete");
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| e.to_string())?;
            let ai_text = extract_ai_text(&gemini_response)?;
            
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            return parse_match_result(ai_text);
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

/// Build the classification prompt for AI models
fn build_classification_prompt(text: &str, available_courses: &str, current_datetime: &str, current_date: &str) -> String {
    format!(
        r#"Classify this WhatsApp academic message. Return ONLY valid JSON, NO markdown.

CURRENT DATE/TIME (GMT+7): {}
TODAY'S DATE: {}

Available courses with their aliases:
{}

Message: "{}"

Output ONE of these exact formats:

NEW assignment:
{{"type":"assignment_info","course_name":"Pemrograman","title":"Tugas Bab 2","deadline":"2025-12-31","description":"Brief description","parallel_code":"K1"}}

UPDATE to existing assignment:
{{"type":"assignment_update","reference_keywords":["pemrograman","bab 2"],"changes":"deadline moved to 2025-12-05","new_deadline":"2025-12-05","new_title":null,"new_description":null}}

Other/unclear:
{{"type":"unrecognized"}}

RULES:
- Use FORMAL course name in "course_name" field (e.g., "Pemrograman" not "pemrog")
- Match course aliases case-insensitively (pemrog → Pemrograman, GKV → Grafika Komputer dan Visualisasi)
- For relative dates:
  * "hari ini" (today) = {}
  * "besok" (tomorrow) = add 1 day
  * "minggu depan" (next week) = add 7 days
  * "senin/selasa/etc" = find next occurrence of that day
- For recurring assignments (LKP, weekly tasks):
  * If message contains assignment name + deadline but NO prior context, classify as "assignment_info"
  * Example: "LKP 13 deadline tonight" → assignment_info (could be first mention of LKP 13)
  * Only use "assignment_update" if message clearly references a change/reminder to existing assignment
- For "assignment_update":
  * "reference_keywords" must include course name/alias AND specific assignment identifiers
  * Example: ["pemrograman", "bab 2"] or ["GKV", "matriks"]
  * Use 2-4 keywords that uniquely identify the assignment
  * If the update provides specific details that would make a better title than the existing generic one (e.g., "Tugas baru"), set "new_title" to a descriptive title
  * Example: existing title "Tugas baru" + update mentions "figma prototype pertemuan 4" → new_title: "Figma Prototype Pertemuan 4"
  * Only set "new_title" if current title is too generic (like "Tugas baru", "Assignment", "Tugas") AND update provides specific details
- "changes" field should briefly describe what changed
- All dates in YYYY-MM-DD format
- Use null for unchanged fields
- parallel_code format: K1, K2, K3, P1, P2, P3 etc (uppercase K/P)
- If message contains an image, extract ALL text from the image and include it in your analysis"#,
        current_datetime,
        current_date,
        available_courses,
        text,
        current_date
    )
}

/// Build the matching prompt with temporal context
fn build_matching_prompt(
    changes: &str, 
    keywords: &[String], 
    assignments: &[Assignment]
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
                "Assignment #{}:\n  ID: {}\n  Title: \"{}\"\n  Description: \"{}\"\n  Deadline: {}\n  Parallel: {}\n  ⏱️ Created: {} ({})",
                i + 1,
                a.id,
                a.title,
                a.description,
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