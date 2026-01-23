use crate::models::AIClassification;
use uuid::Uuid;
use serde::Deserialize;
use chrono::{Utc, FixedOffset};
use crate::tui::JobLogger;

// ===== API RESPONSE STRUCTURES =====

#[derive(Debug, Deserialize)]
pub struct GroqResponse {
    pub choices: Vec<GroqChoice>,
}

#[derive(Debug, Deserialize)]
pub struct GroqChoice {
    pub message: GroqMessage,
}

#[derive(Debug, Deserialize)]
pub struct GroqMessage {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct GeminiResponse {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    #[serde(default)]
    pub prompt_feedback: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct Candidate {
    pub content: Option<Content>,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub safety_ratings: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct Content {
    #[serde(default)]
    pub parts: Option<Vec<Part>>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Part {
    pub text: String,
}

// ===== DUPLICATE CHECK RESULT =====

#[derive(Debug, Deserialize)]
pub(super) struct DuplicateCheckResult {
    pub is_duplicate: bool,
    pub confidence: String,
    #[serde(alias = "reasoning", alias = "explanation")]
    pub reason: String,
    #[serde(default)]
    pub matched_assignment_id: Option<String>,
}

// ===== RESPONSE EXTRACTORS =====

pub fn extract_groq_text(groq_response: &GroqResponse) -> Result<String, String> {
    groq_response
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .ok_or_else(|| "Groq returned empty response".to_string())
}

pub fn extract_ai_text(gemini_response: &GeminiResponse) -> Result<&str, String> {
    // Check if response has any candidates
    if gemini_response.candidates.is_empty() {
        return Err("Gemini returned no candidates (possibly filtered by safety)".to_string());
    }
    
    // Get first candidate
    let candidate = &gemini_response.candidates[0];
    
    // Check finish reason for errors
    if let Some(ref finish_reason) = candidate.finish_reason {
        if finish_reason != "STOP" && finish_reason != "MAX_TOKENS" {
            return Err(format!("Gemini stopped with reason: {}", finish_reason));
        }
    }
    
    // Extract content
    let content = candidate.content.as_ref()
        .ok_or_else(|| "Gemini candidate missing content field".to_string())?;
    
    // Extract parts
    let parts = content.parts.as_ref()
        .ok_or_else(|| "Gemini content missing parts field".to_string())?;
    
    // Get first part's text
    parts
        .first()
        .map(|part| part.text.as_str())
        .ok_or_else(|| "Gemini returned empty parts array".to_string())
}

// ===== PARSERS =====

pub(super) fn parse_classification(ai_text: &str) -> Result<AIClassification, String> {
    let cleaned = ai_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    
    if !is_valid_json_object(cleaned) {
        eprintln!("⚠️  Response is not a valid JSON object");
        return Ok(AIClassification::Unrecognized { 
            reason: Some("Invalid JSON response from AI".to_string()),
            category: Default::default()
        });
    }
    
    match serde_json::from_str::<AIClassification>(cleaned) {
        Ok(classification) => {
            // Clean up parallel codes after deserialization
            Ok(classification.clean_parallel_codes())
        }
        Err(e) => {
            eprintln!("❌ JSON parse error: {}", e);
            eprintln!("   Tried to parse: {}", cleaned);
            Ok(AIClassification::Unrecognized { 
                reason: Some(format!("JSON parsing failed: {}", e)),
                category: Default::default()
            })
        }
    }
}

pub(super) fn parse_match_result(ai_text: &str, logger: &JobLogger) -> Result<Option<(Uuid, String)>, String> {
    let cleaned = ai_text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    
    // Use serde_json::Value for flexible parsing
    match serde_json::from_str::<serde_json::Value>(cleaned) {
        Ok(json_value) => {
            // Extract fields manually to handle various formats
            let confidence = json_value["confidence"]
                .as_str()
                .unwrap_or("low")
                .to_string();
            
            let assignment_id = json_value["assignment_id"]
                .as_str()
                .map(|s| s.to_string());
            
            // Handle reason as either string or object
            let reason_text = match &json_value["reason"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Object(_) => {
                    // If it's an object, try common keys or serialize it
                    json_value["reason"]["explanation"]
                        .as_str()
                        .or_else(|| json_value["reason"]["text"].as_str())
                        .or_else(|| json_value["reason"]["details"].as_str())
                        .unwrap_or("No reason provided")
                        .to_string()
                }
                _ => "No reason provided".to_string()
            };
            
            logger.log(&format!("│ 🔍 Confidence : {}", confidence));
            logger.log(&format!("│ 📝 Reason     : {}", truncate_for_log(&reason_text, 60)));
            
            if confidence == "high" {
                if let Some(id_str) = assignment_id {
                    match Uuid::parse_str(&id_str) {
                        Ok(uuid) => Ok(Some((uuid, reason_text))),
                        Err(e) => {
                            logger.log(&format!("│ ⚠️ Invalid UUID: {}", e));
                            Ok(None)
                        }
                    }
                } else {
                    logger.log("│ ⚠️ High confidence but no assignment_id provided");
                    Ok(None)
                }
            } else {
                logger.log("│ ⚠️ Low confidence match");
                Ok(None)
            }
        }
        Err(e) => {
            logger.log(&format!("│ ❌ Failed to parse match result: {}", e));
            logger.log(&format!("│ 📄 Raw response: {}", truncate_for_log(cleaned, 100)));
            Ok(None)
        }
    }
}

// ===== NUMBER EXTRACTION =====

/// Extract all numbers from a string using character-based parsing
pub fn extract_numbers(text: &str) -> Vec<u32> {
    let mut numbers = Vec::new();
    let mut current_number = String::new();
    
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            current_number.push(ch);
        } else if !current_number.is_empty() {
            if let Ok(num) = current_number.parse::<u32>() {
                numbers.push(num);
            }
            current_number.clear();
        }
    }
    
    // Don't forget the last number if string ends with digits
    if !current_number.is_empty() {
        if let Ok(num) = current_number.parse::<u32>() {
            numbers.push(num);
        }
    }
    
    numbers
}

// ===== ASSIGNMENT TYPE EXTRACTION =====

pub fn extract_assignment_type(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    let types = [
        ("quiz", vec!["quiz", "kuis"]),
        ("exam", vec!["ujian", "uts", "uas", "exam", "test"]),
        ("lab", vec!["lkp", "lab", "praktikum", "praktik"]),
        ("homework", vec!["tugas", "assignment", "homework", "pr"]),
        ("project", vec!["project", "proyek", "ta", "skripsi"]),
        ("report", vec!["laporan", "report", "makalah", "paper"]),
        ("presentation", vec!["presentasi", "presentation", "demo"]),
    ];
    
    for (category, keywords) in types.iter() {
        for keyword in keywords {
            if lower.contains(keyword) {
                return Some(category.to_string());
            }
        }
    }
    None
}

// ===== SIMILARITY CALCULATION =====

pub fn calculate_word_overlap(s1: &str, s2: &str) -> f32 {
    // Create owned strings first to avoid temporary value issues
    let s1_lower = s1.to_lowercase();
    let s2_lower = s2.to_lowercase();
    
    let words1: std::collections::HashSet<&str> = s1_lower
        .split_whitespace()
        .collect();
    let words2: std::collections::HashSet<&str> = s2_lower
        .split_whitespace()
        .collect();
    
    if words1.is_empty() || words2.is_empty() {
        return 0.0;
    }
    
    let common = words1.intersection(&words2).count() as f32;
    let total = words1.len().max(words2.len()) as f32;
    
    common / total
}

// ===== HELPERS =====

pub(super) fn get_current_datetime() -> String {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    now.format("%A, %Y-%m-%d %H:%M:%S").to_string()
}

pub(super) fn get_current_date() -> String {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    now.format("%Y-%m-%d").to_string()
}

pub(super) fn is_valid_json_object(s: &str) -> bool {
    s.starts_with('{') 
        && s.ends_with('}') 
        && s.matches('{').count() == s.matches('}').count()
}

pub(super) fn truncate_for_log(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        text.to_string()
    } else {
        // Use chars().take() to handle Unicode properly
        text.chars().take(max_chars).collect::<String>() + "..."
    }
}