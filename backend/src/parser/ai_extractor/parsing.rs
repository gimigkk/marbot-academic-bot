use crate::models::AIClassification;
use uuid::Uuid;
use serde::Deserialize;
use chrono::{Utc, FixedOffset};

// ===== API RESPONSE STRUCTURES =====

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

// ===== DUPLICATE CHECK RESULT =====

#[derive(Debug, Deserialize)]
pub(super) struct DuplicateCheckResult {
    pub is_duplicate: bool,
    pub confidence: String,
    pub reason: String,
    #[serde(default)]
    pub matched_assignment_id: Option<String>,
}

// ===== RESPONSE EXTRACTORS =====

pub(super) fn extract_groq_text(groq_response: &GroqResponse) -> Result<String, String> {
    groq_response
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .ok_or_else(|| "Groq returned empty response".to_string())
}

pub(super) fn extract_ai_text(gemini_response: &GeminiResponse) -> Result<&str, String> {
    gemini_response
        .candidates
        .first()
        .and_then(|candidate| candidate.content.parts.first())
        .map(|part| part.text.as_str())
        .ok_or_else(|| "Gemini returned empty response".to_string())
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
    now.format("%Y-%m-%d %H:%M:%S").to_string()
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
    fn test_extract_numbers() {
        assert_eq!(extract_numbers("LKP 15"), vec![15]);
        assert_eq!(extract_numbers("LKP 15 and LKP 17"), vec![15, 17]);
        assert_eq!(extract_numbers("Assignment 1, 2, 3"), vec![1, 2, 3]);
        assert_eq!(extract_numbers("No numbers here"), Vec::<u32>::new());
        assert_eq!(extract_numbers("2025-01-15"), vec![2025, 1, 15]);
    }

    #[test]
    fn test_extract_assignment_type() {
        assert_eq!(extract_assignment_type("LKP 15"), Some("lab".to_string()));
        assert_eq!(extract_assignment_type("Quiz 1"), Some("quiz".to_string()));
        assert_eq!(extract_assignment_type("Tugas Pemrograman"), Some("homework".to_string()));
    }

    #[test]
    fn test_word_overlap() {
        assert!(calculate_word_overlap("LKP 15", "LKP 15") > 0.9);
        assert!(calculate_word_overlap("Quiz Data Structures", "Quiz Algorithms") > 0.3);
        assert!(calculate_word_overlap("Quiz", "Lab") < 0.1);
    }
}