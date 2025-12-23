use crate::models::{AIClassification, Assignment};  
use uuid::Uuid;  
use serde::{Deserialize, Serialize};
use serde_json::json;
use chrono::{Utc, FixedOffset};

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

/// Extract structured info from WhatsApp message using Gemini AI
pub async fn extract_with_ai(
    text: &str,
    available_courses: &str,
) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );
    
    // Get current datetime in GMT+7
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let current_datetime = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let current_date = now.format("%Y-%m-%d").to_string();
    
    let prompt = build_classification_prompt(text, available_courses, &current_datetime, &current_date);
    
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
    
    println!("🤖 Sending to Gemini AI...");
    println!("📝 Message: {}", truncate_for_log(text, 100));
    println!("📅 Current time (GMT+7): {}", current_datetime);
    
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    let status = response.status();
    
    if !status.is_success() {
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("⚠️  Rate limit exceeded");
            if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(retry_info) = extract_retry_delay(&error_json) {
                    eprintln!("   Retry after: {}", retry_info);
                }
            }
            return Err("Rate limit exceeded. Try again later.".to_string());
        }
        
        return Err(format!("Gemini API error {}: {}", status, error_text));
    }
    
    let gemini_response: GeminiResponse = response.json().await
        .map_err(|e| format!("Failed to deserialize Gemini response: {}", e))?;
    
    let ai_text = extract_ai_text(&gemini_response)?;
    println!("🤖 Gemini response: {}", ai_text);
    
    parse_classification(ai_text)
}


// ===== MATCH WITH AN EXISTING ASSIGNMENT FOR UPDATE =====

/// Use AI to match an update to a specific assignment
pub async fn match_update_to_assignment(
    changes: &str,
    keywords: &[String],
    active_assignments: &[Assignment],
) -> Result<Option<Uuid>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );
    
    let prompt = build_matching_prompt(changes, keywords, active_assignments);
    
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
    
    println!("🤖 Asking AI to match update to assignment...");
    
    let client = reqwest::Client::new();
    let response = client.post(&url).json(&request_body).send().await
        .map_err(|e| e.to_string())?;
    
    if !response.status().is_success() {
        return Err(format!("AI matching failed: {}", response.status()));
    }
    
    let gemini_response: GeminiResponse = response.json().await
        .map_err(|e| e.to_string())?;
    let ai_text = extract_ai_text(&gemini_response)?;
    
    println!("🤖 AI match result: {}", ai_text);
    
    parse_match_result(ai_text)
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
{{"type":"assignment_update","reference_keywords":["pemrograman","bab 2"],"changes":"deadline moved to 2025-12-05","new_deadline":"2025-12-05","new_description":null}}

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
- "changes" field should briefly describe what changed
- All dates in YYYY-MM-DD format
- Use null for unchanged fields
- parallel_code format: K1, K2, K3, etc (uppercase K)"#,
        current_datetime,
        current_date,
        available_courses,  // ← Added this!
        text,
        current_date
    )
}

/// Build the matching prompt for Gemini
fn build_matching_prompt(changes: &str, keywords: &[String], assignments: &[Assignment]) -> String {
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
            
            format!(
                "Assignment #{}:\n  ID: {}\n  Title: \"{}\"\n  Description: \"{}\"\n  Deadline: {}\n  Parallel: {}",
                i + 1,
                a.id,
                a.title,
                a.description,
                deadline_str,
                parallel_str
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    
    format!(
        r#"You are helping match an update message to the correct assignment in a database.

UPDATE MESSAGE: "{}"
REFERENCE KEYWORDS: {:?}

AVAILABLE ASSIGNMENTS:
{}

TASK:
Look at the keywords and the update message. Find which assignment is being updated.

MATCHING CRITERIA (in order of importance):
1. Keywords mention course name AND assignment topic (e.g., "pemrograman" + "bab 2")
2. Assignment title/description contains the topic mentioned in keywords
3. Parallel code matches if mentioned in update
4. Context makes sense (e.g., deadline change mentions date)

RESPONSE FORMAT:
Return JSON with:
- "assignment_id": the UUID if confident match found, null otherwise
- "confidence": "high" or "low"
- "reason": explain why you matched or didn't match

EXAMPLES:
✅ HIGH confidence:
- Keywords ["pemrograman", "bab 2"] + Assignment Title "Tugas Bab 2" → MATCH
- Keywords ["GKV", "matriks"] + Assignment Description contains "matriks" → MATCH

❌ LOW confidence:
- Keywords too generic, multiple assignments match
- No assignment contains the keywords
- Ambiguous which assignment is meant

Return ONLY valid JSON, no markdown:
{{"assignment_id": "uuid-here", "confidence": "high", "reason": "title matches 'Tugas Bab 2'"}}
or
{{"assignment_id": null, "confidence": "low", "reason": "multiple assignments match keywords"}}"#,
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
    
    println!("🧹 Cleaned: {}", cleaned);
    
    if !is_valid_json_object(cleaned) {
        eprintln!("⚠️  Response is not a valid JSON object");
        eprintln!("   Got: {}", cleaned);
        return Ok(AIClassification::Unrecognized);
    }
    
    match serde_json::from_str::<AIClassification>(cleaned) {
        Ok(classification) => {
            println!("✅ Parsed classification: {:?}", classification);
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
            println!("🔍 Match confidence: {}", result.confidence);
            if let Some(ref reason) = result.reason {
                println!("   Reason: {}", reason);
            }
            
            if result.confidence == "high" {
                if let Some(id_str) = result.assignment_id {
                    Ok(Some(Uuid::parse_str(&id_str).map_err(|e| e.to_string())?))
                } else {
                    Ok(None)
                }
            } else {
                println!("⚠️ AI has low confidence in match");
                Ok(None)
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to parse match result: {}", e);
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
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..max_len])
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