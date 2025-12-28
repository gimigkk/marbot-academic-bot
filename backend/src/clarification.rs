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
    
    // Check title (generic titles are considered "missing")
    let title_lower = assignment.title.to_lowercase();
    if assignment.title.is_empty() || 
       title_lower.contains("tugas baru") ||
       title_lower == "assignment" ||
       title_lower == "tugas" {
        missing.push("title".to_string());
    }
    
    // Check parallel code
    if assignment.parallel_code.is_none() {
        missing.push("parallel_code".to_string());
    }
    
    // Check description
    if let Some(ref desc) = assignment.description {
        let desc_lower = desc.to_lowercase();
        if desc.trim().is_empty() || 
           desc_lower == "no description" ||
           desc_lower == "brief description" {
            missing.push("description".to_string());
        }
    } else {
        missing.push("description".to_string());
    }
    
    missing
}

/// Generate clarification message text
pub fn generate_clarification_message(
    assignment: &AssignmentWithCourse,
    missing_fields: &[String]
) -> String {
    let field_names = missing_fields.iter().map(|f| match f.as_str() {
        "course_name" => "📚 Nama Mata Kuliah",
        "title" => "📝 Judul Tugas",
        "deadline" => "⏰ Deadline",
        "parallel_code" => "🧩 Kode Paralel (K1/K2/K3)",
        "description" => "📄 Deskripsi/Keterangan",
        _ => "❓ Field Unknown"
    }).collect::<Vec<_>>().join("\n• ");
    
    format!(
        "⚠️ *PERLU KLARIFIKASI*\n\
        ━━━━━━━━━━━━━━━━━━\n\
        Tugas baru terdeteksi tapi ada info yang kurang:\n\n\
        🆔 Assignment ID: `{}`\n\
        📌 Judul Saat Ini: {}\n\
        📚 Mata Kuliah: {}\n\n\
        *Info yang dibutuhkan:*\n\
        • {}\n\n\
        💡 *Cara menjawab:*\n\
        Reply/quote pesan ini dengan format:\n\
        `Course: [nama]`\n\
        `Title: [judul]`\n\
        `Deadline: [YYYY-MM-DD]`\n\
        `Parallel: [K1/K2/K3]`\n\
        `Description: [keterangan]`\n\n\
        _Cukup isi field yang kurang saja ya!_",
        assignment.id,
        assignment.title,
        assignment.course_name,
        field_names
    )
}

/// Parse clarification response from user reply
/// Now handles both formats:
/// - Structured: "Parallel: K1"
/// - Simple: "K1" (when only one field is missing)
pub fn parse_clarification_response(text: &str) -> HashMap<String, String> {
    let mut updates = HashMap::new();
    
    // Remove backticks if present
    let text = text.replace('`', "");
    
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        
        // Parse "Field: Value" format
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            
            // Skip if value is empty or is a placeholder
            if value.is_empty() || value.starts_with('[') || value == "..." {
                continue;
            }
            
            match key.as_str() {
                "course" | "mata kuliah" | "matkul" => {
                    updates.insert("course_name".to_string(), value.to_string());
                }
                "title" | "judul" | "nama tugas" => {
                    updates.insert("title".to_string(), value.to_string());
                }
                "deadline" | "due" | "batas waktu" => {
                    updates.insert("deadline".to_string(), value.to_string());
                }
                "parallel" | "paralel" | "kode" | "code" => {
                    updates.insert("parallel_code".to_string(), value.to_string());
                }
                "description" | "deskripsi" | "keterangan" | "desc" => {
                    updates.insert("description".to_string(), value.to_string());
                }
                _ => {}
            }
        } else {
            // No colon found - try to detect what the user meant
            // Check if it looks like a parallel code
            let upper = line.to_uppercase();
            if upper.starts_with('K') && upper.len() == 2 {
                updates.insert("parallel_code".to_string(), upper.to_lowercase());
            }
            // Check if it looks like a date
            else if line.contains('-') && line.len() >= 8 {
                updates.insert("deadline".to_string(), line.to_string());
            }
            // Otherwise treat as description if it's substantial
            else if line.len() > 3 {
                updates.insert("description".to_string(), line.to_string());
            }
        }
    }
    
    updates
}

/// Extract assignment ID from clarification message
pub fn extract_assignment_id_from_message(text: &str) -> Option<Uuid> {
    // Look for pattern: "Assignment ID: `uuid`"
    for line in text.lines() {
        if line.contains("Assignment ID:") {
            if let Some(id_part) = line.split('`').nth(1) {
                if let Ok(uuid) = Uuid::parse_str(id_part.trim()) {
                    return Some(uuid);
                }
            }
        }
    }
    None
}