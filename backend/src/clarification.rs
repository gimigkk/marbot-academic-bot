use crate::models::AssignmentWithCourse;
use uuid::Uuid;
use std::collections::HashMap;

/// Check which fields are missing from an assignment
pub fn identify_missing_fields(assignment: &AssignmentWithCourse) -> Vec<String> {
    let mut missing = Vec::new();
    
    // Check course name
    if assignment.course_name.is_empty() || assignment.course_name == "Unknown Course" {
        missing.push("course_name".to_string());
    }
    
    // Check title (generic/vague titles are considered "missing")
    let title_lower = assignment.title.to_lowercase();
    let is_generic_title = assignment.title.is_empty() || 
        title_lower.contains("tugas baru") ||
        title_lower == "assignment" ||
        title_lower == "tugas" ||
        title_lower == "task" ||
        title_lower == "lkp" ||  // Too generic without context
        title_lower == "pr" ||
        title_lower == "homework" ||
        title_lower.len() < 3;  // Too short
    
    if is_generic_title {
        missing.push("title".to_string());
    }
    
    // Check parallel code
    if assignment.parallel_code.is_none() {
        missing.push("parallel_code".to_string());
    }
    
    // Check description (be stricter about what counts as "valid")
    if let Some(ref desc) = assignment.description {
        let desc_lower = desc.to_lowercase();
        let is_generic_desc = desc.trim().is_empty() || 
            desc_lower == "no description" ||
            desc_lower == "brief description" ||
            desc_lower.contains("assignment") ||
            desc_lower.contains("tugas") ||
            desc.len() < 10;  // Too short to be useful
        
        if is_generic_desc {
            missing.push("description".to_string());
        }
    } else {
        missing.push("description".to_string());
    }
    
    missing
}

/// Generate clarification message text (WhatsApp-friendly formatting)
pub fn generate_clarification_message(
    assignment: &AssignmentWithCourse,
    missing_fields: &[String]
) -> String {
    let field_list = missing_fields.iter().map(|f| match f.as_str() {
        "course_name" => "📚 Nama Mata Kuliah",
        "title" => "📝 Judul Tugas Lengkap",
        "deadline" => "⏰ Deadline",
        "parallel_code" => "🧩 Kode Paralel",
        "description" => "📄 Deskripsi/Keterangan",
        _ => "❓ Unknown"
    }).collect::<Vec<_>>().join("\n");
    
    // Simpler, cleaner format that works better in WhatsApp
    format!(
        "⚠️ *PERLU KLARIFIKASI*\n\
        \n\
        Tugas terdeteksi tapi ada info yang kurang:\n\
        \n\
        📌 *{}* - {}\n\
        🆔 ID: {}\n\
        \n\
        *Info yang dibutuhkan:*\n\
        {}\n\
        \n\
        💡 *Cara reply:*\n\
        Reply pesan ini, lalu tulis info yang kurang.\n\
        \n\
        Contoh jawaban:\n\
        ```\n\
        Title: LKP 14 - Recursion\n\
        Parallel: K1\n\
        Description: Soal ada di slide minggu ke-7\n\
        ```\n\
        \n\
        _(Cukup isi field yang kurang saja!)_",
        assignment.course_name,
        assignment.title,
        assignment.id,
        field_list
    )
}

/// Parse clarification response from user reply
/// Supports multiple formats:
/// - Structured: "Parallel: K1"
/// - Simple: "K1" (when context is clear)
/// - Natural: "paralel k1 aja" or "untuk semua parallel"
pub fn parse_clarification_response(text: &str) -> HashMap<String, String> {
    let mut updates = HashMap::new();
    
    // Remove backticks and extra whitespace
    let text = text.replace('`', "").trim().to_string();
    
    // First pass: Look for structured "Key: Value" format
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            
            // Skip empty or placeholder values
            if value.is_empty() || value.starts_with('[') || value == "..." || value == "-" {
                continue;
            }
            
            match key.as_str() {
                "course" | "mata kuliah" | "matkul" | "mk" => {
                    updates.insert("course_name".to_string(), value.to_string());
                }
                "title" | "judul" | "nama tugas" | "nama" => {
                    updates.insert("title".to_string(), value.to_string());
                }
                "deadline" | "due" | "batas waktu" | "dl" => {
                    updates.insert("deadline".to_string(), value.to_string());
                }
                "parallel" | "paralel" | "kode" | "code" | "kelas" => {
                    let normalized = normalize_parallel_code(value);
                    updates.insert("parallel_code".to_string(), normalized);
                }
                "description" | "deskripsi" | "keterangan" | "desc" | "ket" => {
                    updates.insert("description".to_string(), value.to_string());
                }
                _ => {}
            }
        }
    }
    
    // Second pass: Try to detect unstructured content
    if updates.is_empty() {
        // Check for parallel codes in the entire text
        if let Some(parallel) = detect_parallel_code(&text) {
            updates.insert("parallel_code".to_string(), parallel);
        }
        
        // Check if it looks like a date
        if let Some(date) = detect_date(&text) {
            updates.insert("deadline".to_string(), date);
        }
        
        // If text is substantial and we haven't categorized it, treat as description
        if updates.is_empty() && text.len() > 5 && !text.to_lowercase().starts_with("id:") {
            updates.insert("description".to_string(), text);
        }
    }
    
    updates
}

/// Detect and normalize parallel code from natural text
/// Examples:
/// - "K1" -> "k1"
/// - "parallel 2" -> "k2" (assumes K if not specified)
/// - "untuk semua" -> "all"
/// - "semua parallel" -> "all"
fn detect_parallel_code(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    
    // Check for "all" variations
    if lower.contains("semua") || 
       lower.contains("all") || 
       lower.contains("untuk semua kelas") ||
       lower == "all classes" {
        return Some("all".to_string());
    }
    
    // Look for explicit codes (K1, K2, K3, P1, P2, P3)
    let words: Vec<&str> = text.split_whitespace().collect();
    for word in &words {
        let upper = word.to_uppercase();
        if is_valid_parallel_code(&upper) {
            return Some(upper.to_lowercase());
        }
    }
    
    // Look for patterns like "kelas 1", "parallel 2", etc.
    for (i, word) in words.iter().enumerate() {
        let lower_word = word.to_lowercase();
        if lower_word == "kelas" || lower_word == "parallel" || lower_word == "paralel" {
            if i + 1 < words.len() {
                if let Ok(num) = words[i + 1].parse::<u8>() {
                    if (1..=3).contains(&num) {
                        // Default to K if type not specified
                        return Some(format!("k{}", num));
                    }
                }
            }
        }
    }
    
    None
}

/// Normalize parallel code format
fn normalize_parallel_code(code: &str) -> String {
    let code = code.trim().to_lowercase();
    
    // Handle "all" variations
    if code == "all" || code == "semua" || code == "semua parallel" {
        return "all".to_string();
    }
    
    // Already in correct format (k1, k2, p1, etc.)
    if code.len() == 2 {
        return code;
    }
    
    code
}

/// Detect date in various formats
fn detect_date(text: &str) -> Option<String> {
    // Look for YYYY-MM-DD format
    for word in text.split_whitespace() {
        if word.contains('-') && word.len() >= 8 {
            // Basic validation: should have 2 dashes and be mostly numbers
            let parts: Vec<&str> = word.split('-').collect();
            if parts.len() == 3 {
                if let (Ok(_), Ok(_), Ok(_)) = (
                    parts[0].parse::<u32>(),
                    parts[1].parse::<u32>(),
                    parts[2].parse::<u32>()
                ) {
                    return Some(word.to_string());
                }
            }
        }
    }
    None
}

/// Helper function to validate parallel codes
fn is_valid_parallel_code(code: &str) -> bool {
    if code.to_lowercase() == "all" {
        return true;
    }
    
    if code.len() != 2 {
        return false;
    }
    
    let chars: Vec<char> = code.chars().collect();
    let prefix = chars[0];
    let number = chars[1];
    
    (prefix == 'K' || prefix == 'P') && ('1'..='3').contains(&number)
}

/// Extract assignment ID from clarification message
pub fn extract_assignment_id_from_message(text: &str) -> Option<Uuid> {
    // Look for pattern: "ID: uuid"
    for line in text.lines() {
        if line.contains("ID:") || line.contains("id:") {
            // Try to find UUID after "ID:"
            if let Some(id_part) = line.split("ID:").nth(1) {
                // Clean up the string and try to parse
                let cleaned = id_part.trim().replace('`', "");
                if let Ok(uuid) = Uuid::parse_str(cleaned.trim()) {
                    return Some(uuid);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_parallel_code() {
        assert_eq!(detect_parallel_code("K1"), Some("k1".to_string()));
        assert_eq!(detect_parallel_code("parallel k2"), Some("k2".to_string()));
        assert_eq!(detect_parallel_code("untuk semua"), Some("all".to_string()));
        assert_eq!(detect_parallel_code("kelas 1"), Some("k1".to_string()));
        assert_eq!(detect_parallel_code("P3"), Some("p3".to_string()));
    }

    #[test]
    fn test_parse_structured_response() {
        let text = "Title: LKP 14\nParallel: K1\nDescription: Recursion problems";
        let result = parse_clarification_response(text);
        
        assert_eq!(result.get("title"), Some(&"LKP 14".to_string()));
        assert_eq!(result.get("parallel_code"), Some(&"k1".to_string()));
        assert_eq!(result.get("description"), Some(&"Recursion problems".to_string()));
    }

    #[test]
    fn test_parse_simple_response() {
        let text = "K2";
        let result = parse_clarification_response(text);
        
        assert_eq!(result.get("parallel_code"), Some(&"k2".to_string()));
    }
}