use crate::models::Assignment;
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{Utc, FixedOffset, Duration}; 
use super::context_builder::{MessageContext};

/// Build assignment context list for the prompt
fn build_context_assignments_list(
    assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>
) -> String {
    if assignments.is_empty() {
        return "No assignment in database.".to_string();
    }
    
    let assignments_to_show = assignments.iter().take(20);
    let count = assignments.len().min(20);
    
    let list = assignments_to_show
        .map(|a| {
            let deadline = a.deadline
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "No deadline".to_string());
            
            let parallel = if a.parallel_codes.is_empty() {
                "N/A".to_string()
            } else {
                format!("[{}]", a.parallel_codes.join(", "))
            };
            
            let course_name = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown Course");
            
            format!(
                "- Course: {}, Title: \"{}\", Deadline: {}, Parallels: {}, Desc: \"{}\"",
                course_name, a.title, deadline, parallel, truncate_for_log(&a.description, 80)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    if assignments.len() > 20 {
        format!("{}\n(Showing {} most recent out of {} total assignments)", list, count, assignments.len())
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
    assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    current_datetime: &str, 
    current_date: &str,
    context: Option<&MessageContext>,
) -> String {
    let assignments_context = build_context_assignments_list(assignments, course_map);

    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    
    let tomorrow_str = (now + Duration::days(1)).format("%Y-%m-%d").to_string();
    let lusa_str = (now + Duration::days(2)).format("%Y-%m-%d").to_string();
    let next_week_str = (now + Duration::days(7)).format("%Y-%m-%d").to_string();
    
    let context_hints = if let Some(ctx) = context {
        let mut hints = String::from("\n\nRESOLVED CONTEXT HINTS\n");
        hints.push_str("═══════════════════════════════════════════════════════════════════\n");
        
        // Quoted message context
        if let Some(ref quoted) = ctx.quoted_message_summary {
            hints.push_str("QUOTED MESSAGE REFERENCE:\n");
            hints.push_str(&format!("  {}\n", quoted));
            hints.push_str("  User is replying to/updating this assignment\n\n");
        }
        
        // Global parallels
        if !ctx.parallel_codes.is_empty() {
            hints.push_str(&format!(
                "Global Parallels: [{}] (confidence: {:.0}%, source: {})\n",
                ctx.parallel_codes.join(", "), 
                ctx.parallel_confidence * 100.0, 
                ctx.parallel_source
            ));
        }
        
        // Per-course schedule information
        if !ctx.course_hints.is_empty() {
            hints.push_str("\nPer-Course Schedule Information:\n");
            for course_hint in &ctx.course_hints {
                hints.push_str(&format!("  Course: {}\n", course_hint.course_name));
                
                if !course_hint.parallel_codes.is_empty() {
                    hints.push_str(&format!("    Parallels: [{}]\n", course_hint.parallel_codes.join(", ")));
                }
                
                hints.push_str(&format!("    Deadline Type: {}\n", course_hint.deadline_type));
                
                // Per-parallel schedules
                if !course_hint.parallel_schedules.is_empty() {
                    hints.push_str("    Next Meetings:\n");
                    for ps in &course_hint.parallel_schedules {
                        if let Some(ref meeting) = ps.next_meeting {
                            hints.push_str(&format!("      - {}: {}\n", ps.parallel_code.to_uppercase(), meeting));
                        } else {
                            hints.push_str(&format!("      - {}: No schedule found\n", ps.parallel_code.to_uppercase()));
                        }
                    }
                }
                
                hints.push('\n');
            }
        }
        
        if let Some(ref deadline) = ctx.deadline_hint {
            hints.push_str(&format!(
                "Deadline Suggestion (single assignment only): {}\n\n",
                deadline
            ));
        }
        
        hints.push_str("HOW TO USE THESE HINTS:\n");
        hints.push_str("- Hints are SUGGESTIONS based on patterns and quoted messages\n");
        hints.push_str("- QUOTED MESSAGE: If present, user is updating/referencing that assignment\n");
        hints.push_str("- For 'ketika praktikum'/'saat kelas'/'during class': Use the next meeting time\n");
        hints.push_str("- For explicit times ('besok jam 13', '5 Jan 14:00'): Use that time WITH schedule date if available\n");
        hints.push_str("- If parallels have DIFFERENT meeting times: SPLIT into separate assignments\n");
        hints.push_str("  Example: P1→Thu 10:00, P2→Thu 13:00, P3→Tue 13:00\n");
        hints.push_str("  Create 2 assignments: [P3→Tue 13:00] and [P1,P2→Thu 10:00]\n");
        hints.push_str("- For parallels: Use hint when not explicitly mentioned\n");
        hints.push_str("═══════════════════════════════════════════════════════════════════");
        hints
    } else {
        String::new()
    };
    
    format!(
        r#"You are a bilingual (Indonesian/English) academic assistant that extracts structured assignment information from WhatsApp messages.

═══════════════════════════════════════════════════════════════════
DEFINITION: WHAT IS AN ASSIGNMENT?
═══════════════════════════════════════════════════════════════════

An assignment is academic work that students must PRODUCE, SUBMIT, and have EVALUATED.

THREE MANDATORY REQUIREMENTS (all must be true):
1. ACTION REQUIRED: Students must CREATE something (not just attend/read/watch)
2. DELIVERABLE EXISTS: Concrete output is produced/submitted  
3. EVALUATION EXPECTED: Work will be checked, graded, or assessed

Decision Question: "Will the instructor check if students submitted something?"
- YES → Assignment
- NO → Not an assignment

ASSIGNMENTS (require deliverable work):
- Lab reports, homework, essays, presentations WITH submission requirement
- Quizzes, exams, projects, coding assignments, problem sets
- "Submit X by date Y", "hand in Z", "upload your work", "turn in"
- "Kumpulkan laporan", "submit report", "deadline tugas"
- "Tugas dikumpulkan ketika praktikum" (assignment collected during class)

NOT ASSIGNMENTS (no deliverable to submit):
- CLASS SESSIONS: "praktikum besok", "kelas hari ini", "lecture tomorrow", "lab session Friday"
- ATTENDANCE: "come to class", "hadir ke lab", "meeting at 10am", "pertemuan Senin"
- ANNOUNCEMENTS: "topik minggu ini", "we'll discuss X", "next week's topic"
- READING/VIEWING: "baca chapter 5", "watch this video", "prepare reading" (without submission)
- RESOURCES: "here are the slides", "check the forum", "see course website"

CRITICAL DISTINCTION:
- "Praktikum besok" → Class schedule (NO assignment)
- "Tugas dikumpulkan ketika praktikum" → Assignment (deliverable EXISTS)

═══════════════════════════════════════════════════════════════════
CONTEXT
═══════════════════════════════════════════════════════════════════
Current time (GMT+7): {}
Today's date: {}

REFERENCE DATES (for deadline calculation):
- Besok / Tomorrow: {} 23:59 (end of day)
- Lusa / Day after tomorrow: {} 23:59 (end of day)
- Minggu depan / Next week: {} 23:59 (end of day)

Message to classify: "{}"

Available courses:
{}

Recent Assignments:
{}{}

═══════════════════════════════════════════════════════════════════
CLASSIFICATION TASK
═══════════════════════════════════════════════════════════════════

Classify this message using the PRIORITY ORDER below. Return ONE of:
1. UNRECOGNIZED - Not about assignments (checked FIRST)
2. MULTIPLE_ASSIGNMENTS - Contains 2+ distinct assignments
3. ASSIGNMENT_INFO - Announcing single new assignment
4. ASSIGNMENT_UPDATE - Modifying existing assignment

═══════════════════════════════════════════════════════════════════
PRIORITY 1: ASSIGNMENT VALIDATION (CHECK FIRST - MANDATORY)
═══════════════════════════════════════════════════════════════════

Before any classification, apply the THREE REQUIREMENTS test:

Question 1: Does this require students to CREATE work? (not just attend/read)
Question 2: Is there a DELIVERABLE to submit? (not just participation)  
Question 3: Will it be GRADED/CHECKED? (not just presence)

If ANY answer is NO → IMMEDIATELY classify as UNRECOGNIZED

Common patterns to REJECT (false positives):
- "Praktikum [course] besok" → Class schedule, no deliverable mentioned
- "Kelas [course] hari Rabu" → Attendance announcement
- "Meeting with advisor tomorrow" → Attendance
- "Baca chapter 5" → Reading (no submission requirement)
- "Topik diskusi minggu ini" → Informational
- "Pertemuan zoom jam 2" → Class session

Common patterns to ACCEPT (true assignments):
- "Tugas dikumpulkan ketika praktikum" → Has deliverable (collected during class)
- "Submit before next class" → Has deliverable
- "Kumpulkan di pertemuan berikutnya" → Has deliverable

ONLY proceed to PRIORITY 2 if all three requirements are met.

═══════════════════════════════════════════════════════════════════
PRIORITY 2: CLASSIFICATION LOGIC
═══════════════════════════════════════════════════════════════════

STEP A: Check for QUOTED MESSAGE context (if present in hints)
- If QUOTED MESSAGE REFERENCE exists, user is replying to previous assignment
- Common reply patterns:
  * "diundur" / "berubah" / "changed" → UPDATE to quoted assignment
  * "diperjelas" / "clarification" → UPDATE with details
  * "ada lagi" / "another one" → NEW assignment (not updating quoted one)
- Extract course/parallel info from quoted context for better matching

STEP B: Check for MULTIPLE_ASSIGNMENTS
Signals indicating multiple assignments:
- Numbered lists: "1. Pemrog LKP 14...\n2. Kalkulus Tugas 3..."
- Bullet points with different assignments
- Multiple course mentions: "Pemrog dan Fisika ada tugas"
- Explicit count: "ada 2 tugas", "3 assignments today"

CRITICAL: Apply THREE REQUIREMENTS test to EACH item
- Verify each item requires deliverable work
- Informational messages are NOT assignments
- Extract only items where students must submit work

HANDLING MULTIPLE PARALLELS WITH DIFFERENT SCHEDULES:
When announcement targets multiple parallels AND deadline is "ketika praktikum"/"saat kelas":
- If context shows DIFFERENT meeting times → SPLIT into separate assignments
- Group parallels with SAME deadline together

Example:
  Message: "P1, P2, P3 submit ketika praktikum"
  Context: P1→Thu 10:00, P2→Thu 13:00, P3→Tue 13:00
  Create TWO assignments:
    1. {{"parallel_codes": ["p3"], "deadline": "2026-01-07 13:00", ...}}
    2. {{"parallel_codes": ["p1", "p2"], "deadline": "2026-01-09 10:00", ...}}

STEP C: Distinguish NEW vs UPDATE
NEW_ASSIGNMENT signals:
- "ada tugas baru", "new assignment", clear announcement
- Contains: course + description (deadline optional)
- Sequential numbering not in DB (LKP 15 when only LKP 14 exists)
- "ada lagi" when replying → NEW, not update

UPDATE_ASSIGNMENT patterns:
- Explicit change language: "berubah", "ganti", "diundur", "dimajuin", "revisi", "update", "correction"
- Clarification with reference: "Tugas yang kemarin", "assignment from yesterday"
- Replying to quoted message with change indicators
- MUST have change language (don't assume update just because assignment exists)

Matching logic for updates:
- Use semantic understanding (not exact strings)
- "coding pake kertas" can match "Coding on Paper Assignment"
- Match by: course + identifying keywords (topic/number)
- If QUOTED MESSAGE present: strongly prioritize that assignment
- Must have reasonable match in DB

Key distinction:
- "Ada tugas LKP 15 lagi" → NEW (re-announcement, check for duplicate)
- "LKP 15 deadline berubah" → UPDATE (explicit change)
- Replying with "diundur" → UPDATE (use quoted context)
- Replying with "ada lagi yang ini" → NEW (different assignment)

═══════════════════════════════════════════════════════════════════
PRIORITY 3: EXTRACTION RULES
═══════════════════════════════════════════════════════════════════

TITLE EXTRACTION (CRITICAL - AVOID GENERIC TITLES):
The title should be SPECIFIC and IDENTIFIABLE. Users will see this in a list. Minimum of 2 words, good at 3 words.

BAD TITLES (too generic):
- "Tugas" (what assignment?)
- "Tugas Pemrograman" (which one?)
- "Assignment" (not specific)
- "Praktikum" (which praktikum?)
- "Latihan" (which exercise?)

GOOD TITLES (specific and identifiable):
- "LKP 15" (lab assignment number)
- "Quiz 3" (quiz number)
- "Tugas Berpasangan" (describes type)
- "Laporan Praktikum P2" (specific report)
- "Coding on Paper" (describes content)
- "Tugas Individu Pertemuan 8" (specific meeting)
- "Project Fase 2" (project phase)
- "Essay Final" (final essay)
- "Tugas Kelompok" (group assignment)

TITLE EXTRACTION RULES:
1. Look for IDENTIFIERS first:
   - Numbers: "LKP 15", "Quiz 3", "Problem Set 5", "Latihan #5"
   - Names: "Coding on Paper", "Binary Search Tree Assignment"
   - Phases: "Project Fase 2", "Milestone 3"
   - Meetings: "Pertemuan 8", "Week 10"

2. If no identifier, use DESCRIPTIVE TYPE:
   - "Tugas Berpasangan" (pair work)
   - "Tugas Individu" (individual work)
   - "Tugas Kelompok" (group work)
   - "Laporan Praktikum" (lab report)
   - "Essay Final" (final essay)

3. NEVER use just "Tugas" or "Assignment" alone
   - If minimal info: Add context like "Tugas Besar", "Tugas Akhir", "Mini Project"

4. Keep titles CONCISE (2-5 words max)
   - "Tugas Individu Pertemuan 8" ✓
   - "Tugas individu untuk pertemuan ke-8 yang harus dikerjakan sendiri" ✗

DEADLINE EXTRACTION (priority order):

COMMON ABBREVIATIONS IN INFORMAL MESSAGES:
- "dl" typically means "deadline", not "dulu" (first/earlier)
- Context: "tugas X dl besok" → assignment X, deadline tomorrow
- Parse abbreviations based on context and position in sentence
- "class" typically means class.ipb.ac.id while "kelas" means an offline classroom/lecture/pertemuan

1. WHEN-DURING patterns ("ketika", "saat", "during"):
   IF message says "ketika praktikum"/"saat kelas"/"during class"/"waktu pertemuan":
   → Use EXACT schedule hint time from context
   Example: "dikumpulkan ketika praktikum" + Context "K1: 2026-01-12 08:00" 
   → deadline is "2026-01-12 08:00"

2. EXPLICIT TIME with relative date ("besok jam X", "Jumat pukul Y"):
   IF message has both date AND time:
   → Check if schedule hint date is close to mentioned date
   → IF schedule date exists AND is within 7 days of relative date:
      Use schedule DATE with message TIME
      Example: "besok jam 13:00" + Schedule "2026-01-12 08:00" 
      → Use "2026-01-12 13:00" (schedule date, message time)
   → ELSE: Use calculated relative date with message time
      Example: "besok jam 13:00" (no nearby schedule) → "2026-01-06 13:00"

3. DATE ONLY without specific time ("besok", "Friday", "minggu depan"):
   → Use 23:59 (end of day) with calculated date
   Example: "deadline besok" → "2026-01-06 23:59"

4. NO deadline information:
   → Use null (do NOT guess or invent)

Format: YYYY-MM-DD HH:MM (always include time component)

PARALLEL CODES:
- Valid codes (lowercase): k1, k2, k3, k4, p1, p2, p3, p4, r1, r2, r3, r4, all
- Return as ARRAY (assignments can target multiple parallels)
- Extract from message or use context hint if not explicitly mentioned
- Look in course abbreviation section (e.g., "GRAFKOM K2" → ["k2"])

Examples:
- "Tugas untuk k1 dan k2" → ["k1", "k2"]
- "GRAFKOM K2" → ["k2"]
- "Semua kelas" → ["all"]
- No mention + no context → []

DESCRIPTION FIELD (MANDATORY):
- NEVER leave empty or null
- Generate meaningful description from message content
- Include submission details if mentioned
- If minimal info: "[Course] assignment - [brief context]"
- If has enough info: "[helpful context outside of the title]"

═══════════════════════════════════════════════════════════════════
OUTPUT FORMATS
═══════════════════════════════════════════════════════════════════

UNRECOGNIZED (for non-assignments):
{{"type": "unrecognized"}}

MULTIPLE_ASSIGNMENTS:
{{
  "type": "multiple_assignments",
  "assignments": [
    {{
      "course_name": "Pemrograman",
      "title": "LKP 14",
      "deadline": "2025-12-31 08:00",
      "description": "Programming lab assignment 14",
      "parallel_codes": ["k1", "k2"]
    }},
    {{
      "course_name": "Kalkulus",
      "title": "Problem Set 5",
      "deadline": null,
      "description": "Calculus problem set 5",
      "parallel_codes": []
    }}
  ]
}}

ASSIGNMENT_INFO (single new assignment):
{{
  "type": "assignment_info",
  "course_name": "Pemrograman",
  "title": "LKP 14",
  "deadline": "2025-12-31 23:59",
  "description": "Programming lab assignment 14",
  "parallel_codes": ["k1"]
}}

ASSIGNMENT_UPDATE (modify existing):
{{
  "type": "assignment_update",
  "reference_keywords": ["CourseName", "identifier"],
  "changes": "what changed",
  "new_deadline": "2025-12-30 14:00",
  "new_title": null,
  "new_description": null,
  "parallel_codes": ["all"]
}}

═══════════════════════════════════════════════════════════════════
CORE PRINCIPLES
═══════════════════════════════════════════════════════════════════

1. ALWAYS validate THREE REQUIREMENTS before classifying
2. Check QUOTED MESSAGE context first (if present)
3. Check for MULTIPLE_ASSIGNMENTS before single assignment
4. Split assignments when parallels have different schedules
5. For "ketika praktikum" patterns: Use schedule time EXACTLY
6. For "besok jam X" patterns: Use schedule DATE with message TIME if nearby
7. NEVER use generic titles like "Tugas" alone - be SPECIFIC
8. Extract parallel codes from course abbreviations (e.g., "GRAFKOM K2")
9. Use semantic understanding (not literal matching)
10. When uncertain: NEW > UPDATE (avoid bad matches)
11. When uncertain: UNRECOGNIZED > false positive
12. Course boundaries: Never match updates across different courses

Return ONLY valid JSON. No markdown, no explanations, no commentary."#,
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
    parallel_codes: &[String],
) -> String {
    let assignments_list = assignments.iter().enumerate().map(|(i, a)| {
        let parallel_str = if a.parallel_codes.is_empty() {
            "N/A".to_string()
        } else {
            format!("[{}]", a.parallel_codes.join(", "))
        };
        
        let course_name = a.course_id.and_then(|id| course_map.get(&id)).map(|s| s.as_str()).unwrap_or("Unknown Course");
        
        let created_ago = Utc::now().signed_duration_since(a.created_at);
        let time_ago = if created_ago.num_minutes() < 60 { format!("{} min ago", created_ago.num_minutes()) }
            else if created_ago.num_hours() < 24 { format!("{} hr ago", created_ago.num_hours()) }
            else { format!("{} days ago", created_ago.num_days()) };
        
        let desc_preview = if a.description.is_empty() { "(no description)".to_string() } else { truncate_for_log(&a.description, 60) };
        
        format!("#{}: {} | {} | \"{}\" | Parallels: {} | Desc: \"{}\" | {}", i + 1, a.id, course_name, a.title, parallel_str, desc_preview, time_ago)
    }).collect::<Vec<_>>().join("\n");
    
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let current_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
    
    let parallel_info = if parallel_codes.is_empty() {
        "Parallel codes: (not specified)".to_string()
    } else {
        format!("Parallel codes in update: [{}]", parallel_codes.join(", "))
    };
    
    format!(
        r#"Match this update to an existing assignment.

CONTEXT
Time: {} | Update: "{}" | Keywords: {:?}
{}

Assignments:
{}

TASK: Find which assignment this update refers to, or return null if no match.

OUTPUT FORMAT (JSON only, no commentary):
{{"assignment_id":"uuid","confidence":"high","reason":"..."}}

OR if no match:
{{"assignment_id":null,"confidence":"low","reason":"..."}}

Return ONLY valid JSON. No markdown, no explanations."#,
        current_time, changes, keywords, parallel_info, assignments_list
    )
}

/// Build a STRICT duplicate detection prompt
pub fn build_duplicate_detection_prompt(
    title: &str,
    description: &str,
    course_name: &str,
    parallel_codes: &[String],
    existing_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
) -> String {
    let assignments_list = existing_assignments.iter().enumerate().map(|(i, a)| {
        let parallel_str = if a.parallel_codes.is_empty() {
            "null".to_string()
        } else {
            format!("[{}]", a.parallel_codes.join(", "))
        };
        
        let course = a.course_id.and_then(|id| course_map.get(&id)).map(|s| s.as_str()).unwrap_or("Unknown");
        
        let desc_preview = if a.description.is_empty() { 
            "(no description)".to_string() 
        } else { 
            a.description.chars().take(100).collect::<String>()
        };
        
        format!("{}. ID: {} | Course: {} | Title: \"{}\" | Parallels: {} | Desc: \"{}\"", 
            i + 1, a.id, course, a.title, parallel_str, desc_preview)
    }).collect::<Vec<_>>().join("\n");
    
    let parallel_info = if parallel_codes.is_empty() {
        "Parallels: []".to_string()
    } else {
        format!("Parallels: [{}]", parallel_codes.join(", "))
    };
    
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
1. Sequential numbers = DIFFERENT (LKP 15 ≠ LKP 14 ≠ LKP 17)
2. Assignment types must match (quiz ≠ lab ≠ homework)
3. Topics must be similar
4. Parallel codes must overlap (assignment targeting [k1, k2] can match [k1] or [k1, k2])
5. When uncertain → NOT duplicate (safer to create new)

TRUE DUPLICATES (rare cases only):
- Exact match: "LKP 15" = "LKP 15"
- Semantic match: "Lab Report 3" = "Laboratory Report 3"
- Reannouncement: "Quiz tomorrow" posted twice
- Clarification: "Quiz 5 updated" vs "Quiz 5"

NOT DUPLICATES:
- Different numbers: "LKP 15" ≠ "LKP 14"
- Different types: "Quiz 5" ≠ "Lab 5"
- Different topics: "Data Structures" ≠ "Algorithms"
- No parallel overlap: [k1, k2] ≠ [k3, k4]

OUTPUT FORMAT (JSON only, no commentary or markdown):
{{
  "is_duplicate": true,
  "confidence": "high",
  "reason": "detailed explanation",
  "matched_assignment_id": "uuid-here"
}}

OR if not duplicate:
{{
  "is_duplicate": false,
  "confidence": "high",
  "reason": "detailed explanation",
  "matched_assignment_id": null
}}

Be STRICT. Default to false. Only mark as duplicate with HIGH confidence.
Return ONLY valid JSON. No markdown, no explanations."#,
        course_name,
        title,
        description,
        parallel_info,
        assignments_list
    )
}