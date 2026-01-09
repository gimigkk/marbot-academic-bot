use crate::models::AssignmentWithCourse;
use uuid::Uuid;
use std::collections::HashMap;
use chrono::{NaiveDate, NaiveTime, NaiveDateTime};

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
        title_lower == "lkp" ||
        title_lower == "pr" ||
        title_lower == "homework" ||
        title_lower.len() < 3;
    
    if is_generic_title {
        missing.push("title".to_string());
    }
    
    // Check deadline
    if assignment.deadline_is_missing() {
        missing.push("deadline".to_string());
    }
    
    // Check parallel codes - now it's a Vec
    if assignment.parallel_codes.is_empty() {
        missing.push("parallel_codes".to_string());
    }
    
    // Check description
    if let Some(ref desc) = assignment.description {
        let desc_lower = desc.to_lowercase();
        let is_generic_desc = desc.trim().is_empty() || 
            desc_lower == "no description" ||
            desc_lower == "brief description" ||
            desc_lower.contains("assignment") ||
            desc_lower.contains("tugas") ||
            desc.len() < 10;
        
        if is_generic_desc {
            missing.push("description".to_string());
        }
    } else {
        missing.push("description".to_string());
    }
    
    missing
}

pub fn generate_clarification_messages(
    assignment: &AssignmentWithCourse,
    missing_fields: &[String]
) -> (String, String) {
    let field_list = missing_fields.iter().map(|f| match f.as_str() {
        "course_name" => "📚 Nama Mata Kuliah",
        "title" => "📝 Judul Tugas",
        "deadline" => "⏰ Deadline",
        "parallel_codes" => "🧩 Kode Paralel",
        "description" => "📄 Deskripsi",
        _ => "❓ Unknown"
    }).collect::<Vec<_>>().join("\n");
    
    let desc_preview = assignment.description
        .as_ref()
        .map(|d| format!("📄 {}", d))
        .unwrap_or_else(|| "📄 (belum ada deskripsi)".to_string());
    
    // Display parallel codes
    let parallel_display = assignment.format_parallel_display();
    
    // First message: Info about the assignment and what's missing
    let info_message = format!(
        "*[PERLU KLARIFIKASI]*\n\
        `ID: {}`\n\
        \n\
        📌 *{}* - {}\n\
        {}\n\
        🧩 Parallel: {}\n\
        \n\
        *[INFO KURANG:]*\n\
        {}",
        assignment.id, 
        assignment.course_name,
        assignment.title,
        desc_preview,
        parallel_display,
        field_list,
    );
    
    // Build tips based on missing fields
    let mut tips = Vec::new();

    for field in missing_fields {
        match field.as_str() {
            "deadline" => {
                tips.push("• Deadline: `15 01` atau `15 Jan` (bisa tanpa pemisah: `1501`)");
                tips.push("• Waktu: tambah `23:59` atau `23.59` di belakang");
                tips.push("• Update waktu saja: kirim `08:00` atau `14.30`");
            }
            "parallel_codes" => {
                tips.push("• Parallel: `K1`, `K2`, `K1 P1`, atau `all` untuk semua kelas");
            }
            "title" => {
                tips.push("• Title: beri nama yang jelas dan spesifik");
            }
            "course_name" => {
                tips.push("• Course: nama mata kuliah lengkap");
            }
            "description" => {
                tips.push("• Description: jelaskan tugas dengan detail");
            }
            _ => {}
        }
    }

    // Join tips with newlines
    let tips_section = if !tips.is_empty() {
        format!("[*TIPS CEPAT:*]\n{}", tips.join("\n"))
    } else {
        String::new()
    };

    let template_message = format!(
        "{}\n\n_(Reply pesan ini dengan info yang kurang)_",
        tips_section  // Conditionally added
    );
    
    (info_message, template_message)
}

/// Legacy function for backward compatibility
pub fn generate_clarification_message(
    assignment: &AssignmentWithCourse,
    missing_fields: &[String]
) -> String {
    let (info, template) = generate_clarification_messages(assignment, missing_fields);
    format!("{}\n\n{}", info, template)
}

/// Parse clarification response from user reply
/// Returns Ok(updates) if parsed successfully
/// Returns Err("cancelled") if user wants to cancel
/// Returns Err("no_data") if couldn't parse anything useful
/// 
/// If current_deadline is provided and user sends time-only, it will update just the time
pub fn parse_clarification_response(
    text: &str, 
    current_year: i32,
    current_deadline: Option<NaiveDateTime>
) -> Result<HashMap<String, String>, String> {
    let text_lower = text.trim().to_lowercase();
    
    // Check for cancellation keywords
    if text_lower == "cancel" || 
       text_lower == "batal" || 
       text_lower == "batalkan" ||
       text_lower == "tidak" ||
       text_lower == "no" ||
       text_lower == "skip" {
        return Err("cancelled".to_string());
    }
    
    let mut updates = HashMap::new();
    
    // Remove backticks and extra whitespace
    let text = text.replace('`', "").trim().to_string();
    
    // Check for time-only format FIRST (before structured parsing)
    if let Some(time) = detect_time_only(&text) {
        if let Some(existing_deadline) = current_deadline {
            // Keep the date, update only the time
            let new_deadline = existing_deadline.date().and_time(time);
            updates.insert("deadline".to_string(), new_deadline.format("%Y-%m-%d %H:%M").to_string());
            return Ok(updates);
        } else {
            // No existing deadline to update
            return Err("no_date".to_string());
        }
    }
    
    // First pass: Look for structured "Key: Value" format
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("(") || line.starts_with("Format") || line.starts_with("Tips") {
            continue;
        }
        
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            
            // Skip empty, placeholder, or example values
            if value.is_empty() || 
               value.starts_with('[') || 
               value == "..." || 
               value == "-" ||
               value.starts_with("DD ") ||
               value.starts_with("YYYY") ||
               value.starts_with("15 01") && line.to_lowercase().contains("deadline:") {
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
                    // Use the enhanced deadline parser
                    match parse_deadline_flexible(value, current_year) {
                        Ok(parsed) => {
                            updates.insert("deadline".to_string(), parsed);
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to parse deadline '{}': {}", value, e);
                        }
                    }
                }
                "parallel" | "paralel" | "kode" | "code" | "kelas" => {
                    // Parse comma-separated parallel codes
                    let codes = parse_parallel_codes(value);
                    if !codes.is_empty() {
                        updates.insert("parallel_codes".to_string(), codes.join(","));
                    }
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
        // Check for parallel codes
        let parallel_codes = detect_parallel_codes(&text);
        if !parallel_codes.is_empty() {
            updates.insert("parallel_codes".to_string(), parallel_codes.join(","));
        }
        
        // Check for deadlines
        if let Ok(deadline) = parse_deadline_flexible(&text, current_year) {
            updates.insert("deadline".to_string(), deadline);
        }
        
        // If text is substantial and we haven't categorized it, treat as description
        if updates.is_empty() && text.len() > 5 && !text.to_lowercase().starts_with("id:") {
            updates.insert("description".to_string(), text);
        }
    }
    
    if updates.is_empty() {
        return Err("no_data".to_string());
    }
    
    Ok(updates)
}

/// Parse comma-separated parallel codes
fn parse_parallel_codes(input: &str) -> Vec<String> {
    input.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| normalize_parallel_code(s))
        .collect()
}

/// Detect multiple parallel codes from unstructured text
fn detect_parallel_codes(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut codes = Vec::new();
    
    // Check for "all" variations
    if lower.contains("semua") || 
       lower.contains("all") || 
       lower.contains("untuk semua kelas") ||
       lower == "all classes" {
        return vec!["all".to_string()];
    }
    
    // Look for explicit codes
    let words: Vec<&str> = text.split_whitespace().collect();
    for word in &words {
        let upper = word.trim_matches(|c: char| !c.is_alphanumeric()).to_uppercase();
        if is_valid_parallel_code(&upper) {
            let normalized = upper.to_lowercase();
            if !codes.contains(&normalized) {
                codes.push(normalized);
            }
        }
    }
    
    // Look for patterns like "kelas 1", "parallel 2"
    for (i, word) in words.iter().enumerate() {
        let lower_word = word.to_lowercase();
        if lower_word == "kelas" || lower_word == "parallel" || lower_word == "paralel" {
            if i + 1 < words.len() {
                if let Ok(num) = words[i + 1].parse::<u8>() {
                    if (1..=4).contains(&num) {
                        let code = format!("k{}", num);
                        if !codes.contains(&code) {
                            codes.push(code);
                        }
                    }
                }
            }
        }
    }
    
    codes
}

/// Detect if text is time-only format (HH:MM or HH.MM)
fn detect_time_only(text: &str) -> Option<NaiveTime> {
    let text = text.trim();
    
    // Try HH:MM format
    if let Some((h, m)) = text.split_once(':') {
        if let (Ok(hour), Ok(minute)) = (h.parse::<u32>(), m.parse::<u32>()) {
            if hour < 24 && minute < 60 {
                return NaiveTime::from_hms_opt(hour, minute, 0);
            }
        }
    }
    
    // Try HH.MM format
    if let Some((h, m)) = text.split_once('.') {
        if let (Ok(hour), Ok(minute)) = (h.parse::<u32>(), m.parse::<u32>()) {
            if hour < 24 && minute < 60 {
                return NaiveTime::from_hms_opt(hour, minute, 0);
            }
        }
    }
    
    None
}

/// Flexible deadline parsing supporting multiple formats
fn parse_deadline_flexible(text: &str, current_year: i32) -> Result<String, String> {
    // Month name mappings
    let month_map: HashMap<&str, u32> = [
        // Indonesian
        ("januari", 1), ("februari", 2), ("maret", 3), ("april", 4),
        ("mei", 5), ("juni", 6), ("juli", 7), ("agustus", 8),
        ("september", 9), ("oktober", 10), ("november", 11), ("desember", 12),
        // Abbreviations
        ("jan", 1), ("feb", 2), ("mar", 3), ("apr", 4), ("jun", 6),
        ("jul", 7), ("agu", 8), ("aug", 8), ("sep", 9), ("okt", 10),
        ("oct", 10), ("nov", 11), ("des", 12), ("dec", 12),
        // English
        ("january", 1), ("february", 2), ("march", 3), ("may", 5),
        ("june", 6), ("july", 7), ("august", 8), ("october", 10), ("december", 12),
    ].iter().cloned().collect();
    
    let text = text.trim();
    
    // Extract time first
    let (date_part, time) = extract_time(text);
    
    // Parse date
    let date = parse_date(&date_part, &month_map, current_year)?;
    
    // Format result
    let formatted = if let Some(t) = time {
        format!("{} {}", date.format("%Y-%m-%d"), t.format("%H:%M"))
    } else {
        date.format("%Y-%m-%d").to_string()
    };
    
    Ok(formatted)
}

fn extract_time(text: &str) -> (String, Option<NaiveTime>) {
    // Look for HH:MM or HH.MM patterns
    for word in text.split_whitespace() {
        // Try with : separator
        if let Some((h, m)) = word.split_once(':') {
            if let (Ok(hour), Ok(minute)) = (h.parse::<u32>(), m.parse::<u32>()) {
                if hour < 24 && minute < 60 {
                    if let Some(time) = NaiveTime::from_hms_opt(hour, minute, 0) {
                        let cleaned = text.replace(&format!("{}:{:02}", hour, minute), "")
                                         .replace(&format!("{}:{}", hour, minute), "")
                                         .trim()
                                         .to_string();
                        return (cleaned, Some(time));
                    }
                }
            }
        }
        
        // Try with . separator
        if let Some((h, m)) = word.split_once('.') {
            if let (Ok(hour), Ok(minute)) = (h.parse::<u32>(), m.parse::<u32>()) {
                if hour < 24 && minute < 60 {
                    if let Some(time) = NaiveTime::from_hms_opt(hour, minute, 0) {
                        let cleaned = text.replace(&format!("{}.{:02}", hour, minute), "")
                                         .replace(&format!("{}.{}", hour, minute), "")
                                         .trim()
                                         .to_string();
                        return (cleaned, Some(time));
                    }
                }
            }
        }
    }
    
    (text.to_string(), None)
}

fn parse_date(text: &str, month_map: &HashMap<&str, u32>, current_year: i32) -> Result<NaiveDate, String> {
    let text = text.trim().to_lowercase();
    
    // Try month names first
    let words: Vec<&str> = text.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        let clean_word = word.trim_matches(|c: char| !c.is_alphanumeric());
        
        if let Some(&month) = month_map.get(clean_word) {
            // Check previous word for day
            if i > 0 {
                if let Ok(day) = words[i - 1].parse::<u32>() {
                    if day >= 1 && day <= 31 {
                        return NaiveDate::from_ymd_opt(current_year, month, day)
                            .ok_or_else(|| "Invalid date".to_string());
                    }
                }
            }
            
            // Check next word for day
            if i + 1 < words.len() {
                if let Ok(day) = words[i + 1].parse::<u32>() {
                    if day >= 1 && day <= 31 {
                        return NaiveDate::from_ymd_opt(current_year, month, day)
                            .ok_or_else(|| "Invalid date".to_string());
                    }
                }
            }
        }
    }
    
    // Try numeric formats
    let normalized = text.replace('-', " ")
                         .replace('/', " ")
                         .replace('.', " ")
                         .replace(',', " ");
    
    let numbers: Vec<u32> = normalized.split_whitespace()
                                      .filter_map(|s| s.parse::<u32>().ok())
                                      .collect();
    
    // Two separate numbers: DD MM
    if numbers.len() >= 2 {
        let day = numbers[0];
        let month = numbers[1];
        
        if day >= 1 && day <= 31 && month >= 1 && month <= 12 {
            return NaiveDate::from_ymd_opt(current_year, month, day)
                .ok_or_else(|| "Invalid date".to_string());
        }
    }
    
    // Single number without separator: DDMM
    if numbers.len() == 1 {
        let num = numbers[0];
        
        if num >= 101 && num <= 3112 {
            let day = num / 100;
            let month = num % 100;
            
            if day >= 1 && day <= 31 && month >= 1 && month <= 12 {
                return NaiveDate::from_ymd_opt(current_year, month, day)
                    .ok_or_else(|| "Invalid date".to_string());
            }
        }
    }
    
    Err("Could not parse date".to_string())
}

fn normalize_parallel_code(code: &str) -> String {
    let code = code.trim().to_lowercase();
    
    if code == "all" || code == "semua" || code == "semua parallel" {
        return "all".to_string();
    }
    
    if code.len() == 2 {
        return code;
    }
    
    code
}

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
    
    (prefix == 'K' || prefix == 'P' || prefix == 'R') && ('1'..='4').contains(&number)
}

pub fn extract_assignment_id_from_message(text: &str) -> Option<Uuid> {
    let cleaned_text = text.replace('`', "");
    
    for line in cleaned_text.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.contains("id:") {
            if let Some(id_part) = line.split(':').nth(1) {
                let id_str = id_part.trim();
                if let Ok(uuid) = Uuid::parse_str(id_str) {
                    return Some(uuid);
                }
            }
        }
    }
    
    for word in cleaned_text.split_whitespace() {
        if let Ok(uuid) = Uuid::parse_str(word) {
            return Some(uuid);
        }
    }
    
    None
}

/// Generate cancellation message
pub fn generate_cancellation_message(assignment_id: Uuid) -> String {
    format!(
        "❌ *KLARIFIKASI DIBATALKAN*\n\
        \n\
        Tugas dengan ID `{}` tidak akan disimpan.\n\
        \n\
        💡 Tugas tetap terdeteksi jika muncul lagi nanti.",
        assignment_id
    )
}

/// Generate message when clarification parsing fails
pub fn generate_parse_failed_message() -> String {
    "⚠️ *FORMAT TIDAK DIKENALI*\n\
    \n\
    Maaf, aku tidak bisa memahami format yang kamu kirim.\n\
    \n\
    📌 *Tips:*\n\
    • Reply template yang sudah dikirim\n\
    • Edit bagian yang diperlukan saja\n\
    • Update waktu saja: kirim `08:00` atau `14.30`\n\
    • Atau ketik `batal` untuk membatalkan\n\
    \n\
    Contoh format yang benar:\n\
    ```\n\
    ID: [uuid]\n\
    Deadline: 15 01 23:59\n\
    Parallel: K1, K2\n\
    ```".to_string()
}

/// Generate message when time-only update is attempted without existing deadline
pub fn generate_no_date_message() -> String {
    "⚠️ *TIDAK ADA TANGGAL*\n\
    \n\
    Kamu mengirim waktu saja, tapi tugas ini belum punya tanggal deadline.\n\
    \n\
    📌 Kirim format lengkap:\n\
    ```\n\
    Deadline: 14 Jan 08:00\n\
    ```\n\
    \n\
    Atau:\n\
    ```\n\
    Deadline: 14 01 08:00\n\
    ```".to_string()
}