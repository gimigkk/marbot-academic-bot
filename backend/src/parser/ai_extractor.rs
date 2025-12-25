use crate::models::{AIClassification, Assignment};  
use uuid::Uuid;  
use serde::{Deserialize};
use serde_json::json;
use chrono::{Utc, FixedOffset};

// ===== GEMINI MODEL CONFIGURATION =====

const GEMINI_MODELS: &[&str] = &[
    "gemini-3-flash-preview",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
];

// ===== GEMINI API RESPONSE STRUCTURES =====

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

/// Extract structured info from WhatsApp message using Gemini AI with model fallback
pub async fn extract_with_ai(
    text: &str,
    available_courses: &str,
) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    // Get current datetime in GMT+7
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let current_datetime = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let current_date = now.format("%Y-%m-%d").to_string();
    
    let prompt = build_classification_prompt(text, available_courses, &current_datetime, &current_date);
    
    // LOGGING KEREN DIMULAI DISINI
    println!("\x1b[1;30m┌── 🤖 AI PROCESSING ──────────────────────────\x1b[0m");
    println!("│ 📝 Message  : \x1b[36m\"{}\"\x1b[0m", truncate_for_log(text, 60));
    println!("│ 📅 Time     : {}", current_datetime);
    
    // Try each model in sequence until one succeeds
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
                eprintln!("│ \x1b[31m❌ REQUEST FAILED\x1b[0m : {}", e);
                continue;
            }
        };
        
        let status = response.status();
        
        if status.is_success() {
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : Response received");
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize Gemini response: {}", e))?;
            
            let ai_text = extract_ai_text(&gemini_response)?;
            println!("│ 📄 Result   : {}", truncate_for_log(ai_text, 60));
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            
            return parse_classification(ai_text);
        }
        
        // Handle rate limit
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Unknown error".to_string());
            
            eprintln!("│ ⚠️  RATE LIMIT: {}", model);
            
            if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(retry_info) = extract_retry_delay(&error_json) {
                    eprintln!("│    Retry after: {}", retry_info);
                }
            }
            
            // If this is not the last model, try the next one
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Err("All models are rate limited. Try again later.".to_string());
            }
        }
        
        // Handle other errors
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        
        eprintln!("│ ❌ ERROR    : {} - {}", status, error_text);
        
        // Try next model
        if index < GEMINI_MODELS.len() - 1 {
            continue;
        } else {
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            return Err(format!("All models failed. Last error: {} - {}", status, error_text));
        }
    }
    
    println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
    Err("No models available".to_string())
}


// ===== MATCH WITH AN EXISTING ASSIGNMENT FOR UPDATE =====

/// Use AI to match an update to a specific assignment with model fallback
pub async fn match_update_to_assignment(
    changes: &str,
    keywords: &[String],
    active_assignments: &[Assignment],
) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let prompt = build_matching_prompt(changes, keywords, active_assignments);
    
    println!("\x1b[1;30m┌── 🤖 AI MATCHING ────────────────────────────\x1b[0m");
    println!("│ 🔍 Keywords : {:?}", keywords);
    
    // Try each model in sequence until one succeeds
    for (index, model) in GEMINI_MODELS.iter().enumerate() {
        println!("│ 🔄 Model    : {}", model);
        
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
            println!("│ \x1b[32m✅ SUCCESS\x1b[0m  : Match Found");
            
            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| e.to_string())?;
            let ai_text = extract_ai_text(&gemini_response)?;
            
            println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
            return parse_match_result(ai_text);
        }
        
        // Handle rate limit
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // If this is not the last model, try the next one
            if index < GEMINI_MODELS.len() - 1 {
                continue;
            } else {
                println!("\x1b[1;30m└──────────────────────────────────────────────\x1b[0m");
                return Err("All models are rate limited for matching.".to_string());
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

// ===== HELPER FUNCTIONS =====

/// Build the classification prompt for Gemini with current datetime context and course aliases
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
- parallel_code format: K1, K2, K3, P1, P2, P3 etc (uppercase K/P)"#,
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


/// Extract retry delay from rate limit error
fn extract_retry_delay(error_json: &serde_json::Value) -> Option<String> {
    error_json
        .get("error")?
        .get("details")?
        .as_array()?
        .iter()
        .find(|item| {
            item.get("@type")
                .and_then(|t| t.as_str())
                == Some("type.googleapis.com/google.rpc.RetryInfo")
        })?
        .get("retryDelay")?
        .as_str()
        .map(|s| s.to_string())
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

/// Clean and parse the AI classification from text
fn parse_classification(ai_text: &str) -> Result<AIClassification, String> {
    let cleaned = ai_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    
    // println!("🧹 Cleaned: {}", cleaned); // Commented out to reduce noise
    
    if !is_valid_json_object(cleaned) {
        eprintln!("⚠️  Response is not a valid JSON object");
        // eprintln!("   Got: {}", cleaned);
        return Ok(AIClassification::Unrecognized);
    }
    
    match serde_json::from_str::<AIClassification>(cleaned) {
        Ok(classification) => {
            // println!("✅ Parsed classification: {:?}", classification);
            Ok(classification)
        }
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

/// Check if string looks like a valid JSON object
fn is_valid_json_object(s: &str) -> bool {
    s.starts_with('{') 
        && s.ends_with('}') 
        && s.matches('{').count() == s.matches('}').count()
}

/// Truncate text for logging
fn truncate_for_log(text: &str, max_len: usize) -> String {
    let clean_text = text.replace('\n', " "); // Remove newlines for cleaner logs
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