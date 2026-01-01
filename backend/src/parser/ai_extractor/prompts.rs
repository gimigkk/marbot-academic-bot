use crate::models::Assignment;
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{Utc, FixedOffset, Duration}; 
use super::context_builder::{MessageContext};

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
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
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
    if clean_text.len() <= max_len { 
        clean_text 
    } else { 
        format!("{}...", &clean_text[..max_len]) 
    }
}

/// Build the classification prompt for AI models
pub fn build_classification_prompt(
    text: &str, 
    available_courses: &str, 
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    current_datetime: &str, 
    current_date: &str,
    context: Option<&MessageContext>,
) -> String {
    let assignments_context = build_context_assignments_list(active_assignments, course_map);

    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    
    let tomorrow_str = (now + Duration::days(1)).format("%Y-%m-%d").to_string();
    let lusa_str = (now + Duration::days(2)).format("%Y-%m-%d").to_string();
    let next_week_str = (now + Duration::days(7)).format("%Y-%m-%d").to_string();
    
    let context_hints = if let Some(ctx) = context {
        let mut hints = String::from("\n\nRESOLVED CONTEXT (HINTS - USE AS REFERENCE WHEN NEEDED)\n");
        hints.push_str("═══════════════════════════════════════════════════════════════════\n");
        
        if let Some(ref parallel) = ctx.parallel_code {
            hints.push_str(&format!(
                "✓ Global Parallel: {} (confidence: {:.0}%, source: {})\n",
                parallel, ctx.parallel_confidence * 100.0, ctx.parallel_source
            ));
        }
        
        if !ctx.course_hints.is_empty() {
            hints.push_str("\n✓ Per-Course Hints:\n");
            for course_hint in &ctx.course_hints {
                hints.push_str(&format!("  • {}", course_hint.course_name));
                
                if let Some(ref parallel) = course_hint.parallel_code {
                    hints.push_str(&format!(" → Parallel: {}", parallel));
                }
                
                if let Some(ref deadline) = course_hint.deadline_hint {
                    hints.push_str(&format!(" → Suggested deadline: {}", deadline));
                }
                
                hints.push('\n');
            }
        }
        
        if let Some(ref deadline) = ctx.deadline_hint {
            hints.push_str(&format!(
                "\n✓ Deadline Suggestion (single assignment): {}\n",
                deadline
            ));
        }
        
        hints.push_str("\n⚠️ HOW TO USE HINTS:\n");
        hints.push_str("- Hints are SUGGESTIONS based on schedule/patterns\n");
        hints.push_str("- For \"sebelum pertemuan\"/\"before next meeting\": Use the suggested deadline if available\n");
        hints.push_str("- For explicit dates (\"besok\", \"5 Januari\"): Calculate yourself using reference dates above\n");
        hints.push_str("- For parallels: Use hint when not explicitly mentioned in message\n");
        hints.push_str("- IMPORTANT: Deadline format must be YYYY-MM-DD HH:MM (include time from hint)\n");
        hints.push_str("═══════════════════════════════════════════════════════════════════");
        hints
    } else {
        String::new()
    };
    
    format!(
        r#"You are a bilingual (Indonesian/English) academic assistant that extracts structured assignment information from WhatsApp messages.

CONTEXT
═══════════════════════════════════════════════════════════════════
Current time (GMT+7): {}
Today's date: {}

REFERENCE DATES (USE THESE EXACT DATES - END OF DAY 23:59):
- Besok / Tomorrow : {} 23:59
- Lusa / Day after tomorrow : {} 23:59
- Minggu depan / Next week : {} 23:59

Message: "{}"

Available courses:
{}

Active assignments (recent):
{}{}

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
- Numbered lists: "1. Pemrog LKP 14...\n2. Kalkulus Tugas 3..."
- Multiple course mentions: "Pemrog dan Fisika ada tugas"
- Bullet points with different assignments
- "ada 2 tugas", "3 assignments today"

Extract each as separate assignment with ALL fields (course, title, deadline, description, parallel)

**DEADLINE HANDLING:**
- **If no deadline info exists in EITHER the message OR hints → deadline MUST be NULL**
- If deadline hint is provided in RESOLVED CONTEXT, you MAY use it if appropriate
- For dates WITHOUT specific time (e.g., "besok", "deadline Jumat") → USE 23:59 (end of day)
- For dates WITH specific time (e.g., "jam 10 pagi") → USE that time
- NEVER hallucinate dates when none are mentioned

NEW_ASSIGNMENT signals:
- "ada tugas baru", "new assignment", clear announcement
- Contains: course + deadline + description
- Sequential numbering not in DB (LKP 15 when only LKP 14 exists)

UPDATE_ASSIGNMENT patterns:
- **Explicit change words**: "berubah", "ganti", "diundur", "dimajuin", "revisi", "update", "correction"
- **Clarification with reference**: "Tugas yang kemarin", "assignment from yesterday"
- **MUST have change language** - don't assume update just because assignment exists

**Key distinction**:
- "Ada tugas LKP 15 lagi" → NEW (re-announcement, check for duplicate)
- "LKP 15 deadline berubah" → UPDATE (explicit change)

**Matching logic for updates:**
Use semantic understanding, not exact strings:
- "coding pake kertas" can match "Coding on Paper Assignment"
- Match by: course + identifying keywords (topic/number)
- If reasonable match in DB → UPDATE

UNRECOGNIZED:
- No course mentioned, social chat, vague references without context

PARALLEL CODES
═══════════════════════════════════════════════════════════════════
Valid codes (lowercase): k1, k2, k3, p1, p2, p3, r1, r2, r3, all, null
Different codes = different assignments (K1 ≠ K2)

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
    {{ "course_name": "Pemrograman", "title": "LKP 14", "deadline": "2025-12-31 08:00", "description": "Programming lab assignment 14", "parallel_code": "k1" }},
    {{ "course_name": "Kalkulus", "title": "Problem Set 5", "deadline": null, "description": "Calculus problem set 5", "parallel_code": null }}
  ]
}}

NEW_ASSIGNMENT (single):
{{"type":"assignment_info","course_name":"Pemrograman","title":"LKP 14","deadline":"2025-12-31 23:59","description":"Programming lab assignment 14","parallel_code":"k1"}}

UPDATE_ASSIGNMENT:
{{"type":"assignment_update","reference_keywords":["CourseName","identifier"],"changes":"what changed","new_deadline":"2025-12-30 14:00","new_title":null,"new_description":null,"parallel_code":"all"}}

UNRECOGNIZED:
{{"type":"unrecognized"}}

PRINCIPLES
═══════════════════════════════════════════════════════════════════
1. **Check for multiple assignments FIRST** before single assignment
2. **Semantic over literal**: Understand intent, not just keywords
3. **Context matters**: Use DB and RESOLVED CONTEXT hints
4. **ALWAYS GENERATE DESCRIPTIONS**: Never leave description field empty
5. **Deadline format**: YYYY-MM-DD HH:MM (use provided time from hints, 23:59 for dates without time, NULL if no info)
6. **Confidence-based**: High confidence → classify; Low → UNRECOGNIZED
7. **Course boundaries**: Never match updates across different courses
8. **When uncertain**: NEW > UPDATE (avoid bad matches); Classification > UNRECOGNIZED (avoid noise)

Return ONLY valid JSON. No markdown, no explanations."#,
        current_datetime,
        current_date,
        tomorrow_str,
        lusa_str,
        next_week_str,
        text,
        available_courses,
        assignments_context,
        context_hints
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

/// Build a STRICT duplicate detection prompt
pub fn build_duplicate_detection_prompt(
    title: &str,
    description: &str,
    course_name: &str,
    parallel_code: Option<&str>,
    existing_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
) -> String {
    let assignments_list = existing_assignments.iter().enumerate().map(|(i, a)| {
        let parallel_str = a.parallel_code.as_deref().unwrap_or("null");
        let course = a.course_id.and_then(|id| course_map.get(&id)).map(|s| s.as_str()).unwrap_or("Unknown");
        
        let desc_preview = if a.description.is_empty() { 
            "(no description)".to_string() 
        } else { 
            a.description.chars().take(100).collect::<String>()
        };
        
        format!("{}. ID: {} | Course: {} | Title: \"{}\" | Parallel: {} | Desc: \"{}\"", 
            i + 1, a.id, course, a.title, parallel_str, desc_preview)
    }).collect::<Vec<_>>().join("\n");
    
    let parallel_info = parallel_code
        .map(|pc| format!("Parallel: {}", pc))
        .unwrap_or_else(|| "Parallel: null".to_string());
    
    format!(
        r#"STRICT DUPLICATE DETECTION

NEW ASSIGNMENT:
Course: {}
Title: "{}"
Description: "{}"
{}

CANDIDATES (pre-filtered by course/parallel/numbers/type):
{}

CRITICAL RULES:
═══════════════════════════════════════════════════════════════════
1. Sequential numbers = DIFFERENT (LKP 15 ≠ LKP 14 ≠ LKP 17)
2. Assignment types must match (quiz ≠ lab ≠ homework)
3. Topics must be similar
4. When uncertain → NOT duplicate (safer to create new)

TRUE DUPLICATES (rare cases only):
- Exact match: "LKP 15" = "LKP 15" ✓
- Semantic match: "Lab Report 3" = "Laboratory Report 3" ✓
- Reannouncement: "Quiz tomorrow" posted twice ✓
- Clarification: "Quiz 5 updated" vs "Quiz 5" ✓

NOT DUPLICATES:
- Different numbers: "LKP 15" ≠ "LKP 14" ✗
- Different types: "Quiz 5" ≠ "Lab 5" ✗
- Different topics: "Data Structures" ≠ "Algorithms" ✗

OUTPUT FORMAT:
{{
  "is_duplicate": boolean,
  "confidence": "high" | "medium" | "low",
  "reason": "detailed explanation",
  "matched_assignment_id": "uuid" or null
}}

Be STRICT. Default to false. Only mark as duplicate with HIGH confidence."#,
        course_name,
        title,
        description,
        parallel_info,
        assignments_list
    )
}