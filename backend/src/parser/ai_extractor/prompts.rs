use crate::models::Assignment;
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{Utc, FixedOffset};

/// Build assignment context list for the prompt
/// Limit to 100 most recent to stay within token budgets
fn build_context_assignments_list(
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>
) -> String {
    if active_assignments.is_empty() {
        return "No active assignments in database.".to_string();
    }
    
    // Take up to 100 most recent assignments (sorted by created_at desc in query)
    let assignments_to_show = active_assignments.iter().take(100);
    let count = active_assignments.len().min(100);
    
    let list = assignments_to_show
        .map(|a| {
            let deadline = a.deadline
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "No deadline".to_string());
            let parallel = a.parallel_code.as_deref().unwrap_or("N/A");
            
            // Get course name from map, fallback to "Unknown Course" if not found
            let course_name = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown Course");
            
            format!(
                "- Course: {}, Title: \"{}\", Deadline: {}, Parallel: {}, Desc: \"{}\"",
                course_name,
                a.title,
                deadline,
                parallel,
                truncate_for_log(&a.description, 80)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    if active_assignments.len() > 100 {
        format!("{}\n(Showing {} most recent out of {} total active assignments)", 
            list, count, active_assignments.len())
    } else {
        list
    }
}

/// Truncate text for logging
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
    current_date: &str
) -> String {
    let assignments_context = build_context_assignments_list(active_assignments, course_map);
    
    format!(
        r#"You are a bilingual (Indonesian/English) academic assistant that extracts structured assignment information from WhatsApp messages.

CONTEXT
═══════════════════════════════════════════════════════════════════
Current time (GMT+7): {}
Today's date: {}

Message: "{}"

Available courses:
{}

Active assignments (recent):
{}

TASK
═══════════════════════════════════════════════════════════════════
Classify this message as:
1. **NEW_ASSIGNMENT** - Announcing a new task
2. **UPDATE_ASSIGNMENT** - Modifying/clarifying existing assignment
3. **UNRECOGNIZED** - Not about assignments

CLASSIFICATION GUIDELINES
═══════════════════════════════════════════════════════════════════

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

• k1-k3, p1-p3: specific sections
• all: applies to all sections ("untuk semua parallel")
• null: not specified

Different codes = different assignments (K1 ≠ K2)

DATE PARSING (relative to {})
═══════════════════════════════════════════════════════════════════
• "hari ini"/"today" → {}
• "besok"/"tomorrow" → +1 day
• "lusa" → +2 days  
• "minggu depan" → +7 days
• Day names → next occurrence

Output: YYYY-MM-DD or null

OUTPUT FORMATS
═══════════════════════════════════════════════════════════════════

NEW_ASSIGNMENT:
{{"type":"assignment_info","course_name":"Pemrograman","title":"LKP 14","deadline":"2025-12-31","description":"Brief description","parallel_code":"k1"}}

UPDATE_ASSIGNMENT:
{{"type":"assignment_update","reference_keywords":["CourseName","identifier"],"changes":"what changed","new_deadline":"2025-12-30","new_title":null,"new_description":null,"parallel_code":"all"}}

UNRECOGNIZED:
{{"type":"unrecognized"}}

EXAMPLES
═══════════════════════════════════════════════════════════════════

Example 1 - Clear NEW:
"Ada tugas baru Pemrograman LKP 15 deadline minggu depan"
→ {{"type":"assignment_info","course_name":"Pemrograman","title":"LKP 15","deadline":"2026-01-04","description":"Programming assignment","parallel_code":null}}

Example 2 - Descriptive UPDATE:
"Tugas Pemrog yang coding pake kertas jadinya untuk semua parallel"
(DB has Pemrograman coding assignment)
→ {{"type":"assignment_update","reference_keywords":["Pemrograman","coding","kertas"],"changes":"scope changed to all parallel classes","new_deadline":null,"new_title":null,"new_description":null,"parallel_code":"all"}}

Example 3 - Different course, same topic = NEW:
"Pemrograman prototype deadline besok"
(DB has "UX Design prototype" only)
→ {{"type":"assignment_info","course_name":"Pemrograman","title":"Prototype","deadline":"2025-12-29","description":"Programming prototype","parallel_code":null}}

Example 4 - Too vague:
"deadline besok ya"
→ {{"type":"unrecognized"}}

PRINCIPLES
═══════════════════════════════════════════════════════════════════
1. **Semantic over literal**: Understand intent, not just keywords
2. **Context matters**: Use DB to inform decisions
3. **Confidence-based**: High confidence → classify; Low → UNRECOGNIZED
4. **Course boundaries**: Never match updates across different courses
5. **When uncertain**: NEW > UPDATE (avoid bad matches); Classification > UNRECOGNIZED (avoid noise)

Return ONLY valid JSON. No markdown, no explanations."#,
        current_datetime,
        current_date,
        text,
        available_courses,
        assignments_context,
        current_date,
        current_date
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
    let assignments_list = assignments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let parallel_str = a.parallel_code
                .as_deref()
                .unwrap_or("N/A");
            
            let course_name = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown Course");
            
            let created_ago = Utc::now().signed_duration_since(a.created_at);
            let time_ago = if created_ago.num_minutes() < 60 {
                format!("{} min ago", created_ago.num_minutes())
            } else if created_ago.num_hours() < 24 {
                format!("{} hr ago", created_ago.num_hours())
            } else {
                format!("{} days ago", created_ago.num_days())
            };
            
            format!(
                "#{}: {} | {} | \"{}\" | Parallel: {} | {}",
                i + 1,
                a.id,
                course_name,
                a.title,
                parallel_str,
                time_ago
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let current_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
    
    let parallel_info = parallel_code
        .map(|pc| format!("Parallel code in update: {}", pc))
        .unwrap_or_else(|| "Parallel code: (not specified)".to_string());
    
    format!(
        r#"Match this update to an existing assignment.

CONTEXT
═══════════════════════════════════════════════════════════════════
Time: {}
Update: "{}"
Keywords: {:?}
{}

Assignments:
{}

TASK
═══════════════════════════════════════════════════════════════════
Find which assignment this update refers to, or return null if no match.

MATCHING STRATEGY
═══════════════════════════════════════════════════════════════════

Step 1: Course Filter
• First keyword = course name
• Only consider assignments from that course

Step 2: Semantic Content Match
• Match by MEANING, not exact strings
• "coding kertas" matches "Coding on Paper"
• "matriks" matches "Matrix Operations"
• Look for keywords in title/description

Step 3: Parallel Code Handling

**Two cases:**

A) **Scope Change** - Update is CHANGING parallel code
   Signals: "jadinya untuk [code]", "untuk semua", changes mention "scope"
   Strategy: IGNORE current parallel code, match by content only
   Why: The parallel code is what's being updated
   
B) **Parallel-Specific Update** - Update applies to specific parallel
   Signals: "[code] deadline [X]", no scope change language
   Strategy: Must match parallel code exactly
   Why: Update only applies to that section

For this update:
Changes: "{}"
Parallel: {}
→ If changes mention "scope" or update is "untuk [code]" → Case A
→ Otherwise → Case B

Step 4: Confidence
• Course + content match → HIGH
• Missing course → NULL
• Content mismatch → NULL
• Recency is tiebreaker only

OUTPUT FORMAT
═══════════════════════════════════════════════════════════════════

Match found:
{{"assignment_id":"uuid","confidence":"high","reason":"Course and content match"}}

No match:
{{"assignment_id":null,"confidence":"low","reason":"Why no match"}}

EXAMPLES
═══════════════════════════════════════════════════════════════════

Example 1 - Scope change (Case A):
Keywords: ["Pemrograman","coding","kertas"]
Changes: "scope changed to k2"
Parallel: k2
Assignment: Pemrograman "Coding on Paper" Parallel: k1

→ Match! (ignore parallel mismatch - it's being changed)
{{"assignment_id":"uuid-1","confidence":"high","reason":"Course and content match, scope being changed to k2"}}

Example 2 - Parallel-specific (Case B):
Keywords: ["Pemrograman","LKP 13"]
Changes: "deadline extended"
Parallel: k2
Assignments:
  - Pemrograman "LKP 13" Parallel: k1
  - Pemrograman "LKP 13" Parallel: k2

→ Match #2 (must match parallel for Case B)
{{"assignment_id":"uuid-2","confidence":"high","reason":"Course, content, and parallel all match"}}

Example 3 - No match:
Keywords: ["Pemrograman","matriks"]
Assignments: UX Design "Prototype"

→ No match (wrong course)
{{"assignment_id":null,"confidence":"low","reason":"No Pemrograman assignments found"}}

PRINCIPLES
═══════════════════════════════════════════════════════════════════
• Think like a human: "Tugas X jadinya untuk K2" = find X, change its parallel to K2
• Semantic matching: meaning > exact words
• Course boundaries: never match across courses
• Recency helps but doesn't override content mismatch

Return ONLY valid JSON."#,
        current_time,
        changes,
        keywords,
        parallel_info,
        assignments_list,
        changes,
        parallel_code.unwrap_or("not specified")
    )
}