// backend/src/parser/ai_extractor/prompts.rs

use crate::models::Assignment;
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{Utc, FixedOffset, Duration}; // ✅ Tambah Duration

/// Build assignment context list for the prompt
fn build_context_assignments_list(
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>
) -> String {
    if active_assignments.is_empty() {
        return "No active assignments in database.".to_string();
    }
    
    let assignments_to_show = active_assignments.iter().take(100);
    let count = active_assignments.len().min(100);
    
    let list = assignments_to_show
        .map(|a| {
            let deadline = a.deadline
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "No deadline".to_string());
            let parallel = a.parallel_code.as_deref().unwrap_or("N/A");
            
            let course_name = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown Course");
            
            format!(
                "- Course: {}, Title: \"{}\", Deadline: {}, Parallel: {}, Desc: \"{}\"",
                course_name, a.title, deadline, parallel, truncate_for_log(&a.description, 80)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    if active_assignments.len() > 100 {
        format!("{}\n(Showing {} most recent out of {} total active assignments)", list, count, active_assignments.len())
    } else {
        list
    }
}

fn truncate_for_log(text: &str, max_len: usize) -> String {
    let clean_text = text.replace('\n', " ");
    if clean_text.len() <= max_len { clean_text } else { format!("{}...", &clean_text[..max_len]) }
}

/// Build the classification prompt for AI models
pub fn build_classification_prompt(
    text: &str, 
    available_courses: &str, 
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    current_datetime: &str, 
    current_date: &str
) -> String {
    let assignments_context = build_context_assignments_list(active_assignments, course_map);

    // ===== ✅ CONTEXT INJECTION (Rust Calculation) =====
    // Hitung tanggal pasti menggunakan Rust (WIB/GMT+7)
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    
    let tomorrow_str = (now + Duration::days(1)).format("%Y-%m-%d").to_string();
    let lusa_str = (now + Duration::days(2)).format("%Y-%m-%d").to_string();
    let next_week_str = (now + Duration::days(7)).format("%Y-%m-%d").to_string();
    // ===================================================
    
    format!(
        r#"You are a bilingual (Indonesian/English) academic assistant that extracts structured assignment information from WhatsApp messages.

CONTEXT
═══════════════════════════════════════════════════════════════════
Current time (GMT+7): {}
Today's date: {}

REFERENCE DATES (USE THESE EXACT DATES):
• Besok / Tomorrow : {}
• Lusa / Day after tomorrow : {}
• Minggu depan / Next week : {}

Message: "{}"

Available courses:
{}

Active assignments (recent):
{}

TASK
═══════════════════════════════════════════════════════════════════
Classify this message as:
1. **MULTIPLE_ASSIGNMENTS** - Message contains 2+ assignments (CHECK FIRST)
2. **NEW_ASSIGNMENT** - Announcing a single new task
3. **UPDATE_ASSIGNMENT** - Modifying/clarifying existing assignment
4. **UNRECOGNIZED** - Not about assignments

CLASSIFICATION GUIDELINES
═══════════════════════════════════════════════════════════════════

**MULTIPLE_ASSIGNMENTS (PRIORITY CHECK):**
Signals:
• Numbered lists: "1. Pemrog LKP 14...\n2. Kalkulus Tugas 3..."
• Multiple course mentions: "Pemrog dan Fisika ada tugas"
• Bullet points with different assignments
• "ada 2 tugas", "3 assignments today"

Extract each as separate assignment with ALL fields (course, title, deadline, description, parallel)

NEW_ASSIGNMENT signals:
• "ada tugas baru", "new assignment", clear announcement
• Contains: course + deadline + description
• Sequential numbering not in DB (LKP 15 when only LKP 14 exists)

UPDATE_ASSIGNMENT patterns:
• Direct: "LKP 13 deadline berubah"
• Descriptive: "Tugas Pemrog yang [description]" - references existing work
• Clarification: "jadinya", "ternyata", "sebenarnya"
• Changes: "ganti", "diundur", "dimajuin", "revisi"

**Matching logic for updates:**
Use semantic understanding, not exact strings:
• "coding pake kertas" can match "Coding on Paper Assignment"
• Match by: course + identifying keywords (topic/number)
• If reasonable match in DB → UPDATE

UNRECOGNIZED:
• No course mentioned, social chat, vague references without context

PARALLEL CODES
═══════════════════════════════════════════════════════════════════
Valid codes (lowercase): k1, k2, k3, p1, p2, p3, all, null
Different codes = different assignments (K1 ≠ K2)

DATE PARSING (LOOK AT "REFERENCE DATES" ABOVE)
═══════════════════════════════════════════════════════════════════
• "hari ini"/"today" → Use "Today's date" from CONTEXT
• "besok"/"tomorrow" → Use "Besok" date from REFERENCE DATES above
• "lusa" → Use "Lusa" date from REFERENCE DATES above
• "minggu depan" → Use "Minggu depan" date from REFERENCE DATES
• Day names (e.g., "Senin") → Calculate next occurrence strictly based on calendar.

CRITICAL: Do NOT calculate dates manually if a reference is provided above. Copy the exact YYYY-MM-DD string.

**CRITICAL: DESCRIPTION FIELD IS MANDATORY**
═══════════════════════════════════════════════════════════════════
**NEVER leave description empty or null.** Always generate a meaningful description.
If minimal, use: "[Course] [assignment type] [identifier]"

OUTPUT FORMATS
═══════════════════════════════════════════════════════════════════

MULTIPLE_ASSIGNMENTS:
{{
  "type": "multiple_assignments",
  "assignments": [
    {{ "course_name": "Pemrograman", "title": "LKP 14", "deadline": "2025-12-31", "description": "Programming lab assignment 14", "parallel_code": "k1" }},
    {{ "course_name": "Kalkulus", "title": "Problem Set 5", "deadline": "2026-01-02", "description": "Calculus problem set 5", "parallel_code": null }}
  ]
}}

NEW_ASSIGNMENT (single):
{{"type":"assignment_info","course_name":"Pemrograman","title":"LKP 14","deadline":"2025-12-31","description":"Programming lab assignment 14","parallel_code":"k1"}}

UPDATE_ASSIGNMENT:
{{"type":"assignment_update","reference_keywords":["CourseName","identifier"],"changes":"what changed","new_deadline":"2025-12-30","new_title":null,"new_description":null,"parallel_code":"all"}}

UNRECOGNIZED:
{{"type":"unrecognized"}}

PRINCIPLES
═══════════════════════════════════════════════════════════════════
1. **Check for multiple assignments FIRST** before single assignment
2. **Semantic over literal**: Understand intent, not just keywords
3. **Context matters**: Use DB to inform decisions
4. **ALWAYS GENERATE DESCRIPTIONS**: Never leave description field empty
5. **Confidence-based**: High confidence → classify; Low → UNRECOGNIZED
6. **Course boundaries**: Never match updates across different courses
7. **When uncertain**: NEW > UPDATE (avoid bad matches); Classification > UNRECOGNIZED (avoid noise)

Return ONLY valid JSON. No markdown, no explanations."#,
        current_datetime,
        current_date,
        tomorrow_str,   // ✅ INJECTED
        lusa_str,       // ✅ INJECTED
        next_week_str,  // ✅ INJECTED
        text,
        available_courses,
        assignments_context
    )
}

/// Build the matching prompt for assignment updates
pub fn build_matching_prompt(
    changes: &str, 
    keywords: &[String], 
    assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    parallel_code: Option<&str>,  
) -> String {
    let assignments_list = assignments.iter().enumerate().map(|(i, a)| {
        let parallel_str = a.parallel_code.as_deref().unwrap_or("N/A");
        let course_name = a.course_id.and_then(|id| course_map.get(&id)).map(|s| s.as_str()).unwrap_or("Unknown Course");
        
        let created_ago = Utc::now().signed_duration_since(a.created_at);
        let time_ago = if created_ago.num_minutes() < 60 { format!("{} min ago", created_ago.num_minutes()) }
            else if created_ago.num_hours() < 24 { format!("{} hr ago", created_ago.num_hours()) }
            else { format!("{} days ago", created_ago.num_days()) };
        
        let desc_preview = if a.description.is_empty() { "(no description)".to_string() } else { truncate_for_log(&a.description, 60) };
        
        format!("#{}: {} | {} | \"{}\" | Parallel: {} | Desc: \"{}\" | {}", i + 1, a.id, course_name, a.title, parallel_str, desc_preview, time_ago)
    }).collect::<Vec<_>>().join("\n");
    
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let current_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
    
    let parallel_info = parallel_code.map(|pc| format!("Parallel code in update: {}", pc)).unwrap_or_else(|| "Parallel code: (not specified)".to_string());
    
    format!(
        r#"Match this update to an existing assignment.
CONTEXT
Time: {} | Update: "{}" | Keywords: {:?}
{}
Assignments:
{}
TASK: Find which assignment this update refers to, or return null if no match.
OUTPUT: {{"assignment_id":"uuid","confidence":"high","reason":"..."}} or {{"assignment_id":null,"confidence":"low","reason":"..."}}
Return ONLY valid JSON."#,
        current_time, changes, keywords, parallel_info, assignments_list
    )
}