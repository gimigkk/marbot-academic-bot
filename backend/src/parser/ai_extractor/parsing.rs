use crate::models::AIClassification;
use uuid::Uuid;
use serde::Deserialize;
use chrono::{Utc, FixedOffset};

// ===== API RESPONSE STRUCTURES =====

// Groq response structure
#[derive(Debug, Deserialize)]
pub(super) struct GroqResponse {
    pub choices: Vec<GroqChoice>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GroqChoice {
    pub message: GroqMessage,
}

#[derive(Debug, Deserialize)]
pub(super) struct GroqMessage {
    pub content: String,
}

// Gemini response structure
#[derive(Debug, Deserialize)]
pub(super) struct GeminiResponse {
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Candidate {
    pub content: Content,
}

#[derive(Debug, Deserialize)]
pub(super) struct Content {
    pub parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Part {
    pub text: String,
}

// ===== RESPONSE EXTRACTORS =====

/// Extract text from Groq response
pub(super) fn extract_groq_text(groq_response: &GroqResponse) -> Result<String, String> {
    groq_response
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .ok_or_else(|| "Groq returned empty response".to_string())
}

/// Extract AI text from Gemini response structure
pub(super) fn extract_ai_text(gemini_response: &GeminiResponse) -> Result<&str, String> {
    gemini_response
        .candidates
        .first()
        .and_then(|candidate| candidate.content.parts.first())
        .map(|part| part.text.as_str())
        .ok_or_else(|| "Gemini returned empty response".to_string())
}

// ===== PARSERS =====

/// Clean and parse the AI classification from text
pub(super) fn parse_classification(ai_text: &str) -> Result<AIClassification, String> {
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
pub(super) fn parse_match_result(ai_text: &str) -> Result<Option<Uuid>, String> {
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
pub(super) fn get_current_datetime() -> String {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    now.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Get current date in GMT+7
pub(super) fn get_current_date() -> String {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    now.format("%Y-%m-%d").to_string()
}

/// Check if string looks like a valid JSON object
pub(super) fn is_valid_json_object(s: &str) -> bool {
    s.starts_with('{') 
        && s.ends_with('}') 
        && s.matches('{').count() == s.matches('}').count()
}

/// Truncate text for logging
pub(super) fn truncate_for_log(text: &str, max_len: usize) -> String {
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