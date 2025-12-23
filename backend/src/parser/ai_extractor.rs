use crate::models::{AIClassification, Assignment};  
use uuid::Uuid;  
use serde::{Deserialize, Serialize};
use serde_json::json;

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
    available_courses: &str,  // e.g., "Grafkom, Basdat, Probstat"
) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;
    
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash-exp:generateContent?key={}",
        api_key
    );
    
    // Pass courses to prompt builder
    let prompt = build_classification_prompt(text, available_courses);
    
    let request_body = json!({
        "contents": [{
            "parts": [{
                "text": prompt
            }]
        }],
        "generationConfig": {
            "temperature": 0.1,
            "maxOutputTokens": 512,
            "responseMimeType": "application/json"
        }
    });
    
    println!("🤖 Sending to Gemini AI...");
    println!("📝 Message: {}", truncate_for_log(text, 100));
    
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
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash-exp:generateContent?key={}",
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
            "temperature": 0.1,
            "maxOutputTokens": 512,
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

/// Build the classification prompt for Gemini
/// TODO GILANG: Change from course name to alias, go to crud.rs
fn build_classification_prompt(text: &str, available_courses: &str) -> String {
    format!(
        r#"Classify this WhatsApp academic message. Return ONLY valid JSON, NO markdown.

Available courses: {}

Message: "{}"

Output ONE of these exact formats:

NEW assignment:
{{"type":"assignment_info","course_name":"CourseName","title":"Brief title","deadline":"2025-12-31","description":"One line desc","parallel_code":"K1"}}

UPDATE to existing assignment (deadline change, cancellation, clarification):
{{"type":"assignment_update","reference_keywords":["grafkom","ray tracing"],"changes":"deadline moved to Monday","new_deadline":"2025-12-31","new_description":null}}

Other/unclear:
{{"type":"unrecognized"}}

RULES:
- "course_name" must match one of the available courses (case-insensitive)
- "assignment_update" for: deadline changes, cancellations, corrections, clarifications
- "reference_keywords" should be 2-4 words that identify the original assignment
- Keep all fields under 100 characters
- Use null for unchanged fields"#,
        available_courses,
        text
    )
}

/// Build the matching prompt for Gemini
fn build_matching_prompt(changes: &str, keywords: &[String], assignments: &[Assignment]) -> String {
    let assignments_list = assignments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            format!(
                "{}. ID: {}, Title: \"{}\", Description: \"{}\", Deadline: {}",
                i + 1,
                a.id,
                a.title,
                a.description,
                a.deadline.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or("None".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    format!(
        r#"Match this update to the correct assignment.

Update: "{}"
Keywords: {:?}

Active assignments:
{}

Return JSON with the assignment ID if confident, or null if uncertain:
{{"assignment_id": "uuid-here", "confidence": "high"}}
or
{{"assignment_id": null, "confidence": "low", "reason": "ambiguous match"}}

Only return high confidence if you're certain which assignment this refers to."#,
        changes, keywords, assignments_list
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
    // Clean up markdown code blocks if present
    let cleaned = ai_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    
    println!("🧹 Cleaned: {}", cleaned);
    
    // Basic JSON validation
    if !is_valid_json_object(cleaned) {
        eprintln!("⚠️  Response is not a valid JSON object");
        eprintln!("   Got: {}", cleaned);
        return Ok(AIClassification::Unrecognized);
    }
    
    // Parse JSON into AIClassification
    match serde_json::from_str::<AIClassification>(cleaned) {
        Ok(classification) => {
            println!("✅ Parsed classification: {:?}", classification);
            Ok(classification)
        }
        Err(e) => {
            eprintln!("❌ JSON parse error: {}", e);
            eprintln!("   Tried to parse: {}", cleaned);
            
            // Fallback to unrecognized instead of failing
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
    }
    
    match serde_json::from_str::<MatchResult>(cleaned) {
        Ok(result) => {
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

/// Truncate text for logging (avoid spam in console)
fn truncate_for_log(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..max_len])
    }
}

// ===== TESTS =====

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