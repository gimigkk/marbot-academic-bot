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
    
    let assignments_to_show = assignments.iter().take(10);
    let count = assignments.len().min(10);
    
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
                course_name, a.title, deadline, parallel, truncate_for_log(&a.description, 40)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    if assignments.len() > 10 {
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

        // Display overall confidence and source
        hints.push_str(&format!(
            "Detection Confidence: {:.0}% (source: {})\n\n",
            ctx.parallel_confidence * 100.0,
            ctx.parallel_source
        ));
        
        // Quoted message context
        if let Some(ref quoted) = ctx.quoted_message_summary {
            hints.push_str("QUOTED MESSAGE REFERENCE:\n");
            hints.push_str(&format!("  {}\n", quoted));
            hints.push_str("  User is replying to/updating this assignment\n\n");
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
        r#"You are a bilingual (Indonesian/English) academic assistant that extracts structured assignment information from WhatsApp messages. Make sure to fill the fields in Indonesian.

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
- CLASS SESSIONS/ATTENDENCE: "praktikum besok", "hadir ke lab", "lecture tomorrow", "lab session Friday"
- ANNOUNCEMENTS: "topik minggu ini", "we'll discuss X", "next week's topic"
- READING/VIEWING: "baca chapter 5", "watch this video", "prepare reading" (without submission)
- RESOURCES: "here are the slides", "check the forum", "see course website"

CRITICAL: "Praktikum besok" = NO assignment | "Tugas dikumpulkan ketika praktikum" = Assignment"

═══════════════════════════════════════════════════════════════════
REQUIRED INFORMATION
═══════════════════════════════════════════════════════════════════

To classify as assignment, you MUST identify:
1. **Course name** (mandatory - from message or context)
2. **Specific identifier** (number, name, or distinguishing feature)

If missing course name or only generic keywords ("tugas", "assignment") without context:
→ UNRECOGNIZED (academic_related) with reason stating what's missing

Principle: Would a student know WHICH assignment for WHICH course?
- NO → UNRECOGNIZED
- YES → Classify normally

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

Check if message is assignment-related by asking: "Is this about academic work students need to complete?"

RECOGNIZE AS ASSIGNMENT if message mentions:
- Work to submit: "tugas", "assignment", "dikumpulkan", "submit", "deadline"
- Academic assessments: "quiz", "ujian", "exam", "test", "kuis"  
- Academic deliverables: "laporan", "report", "project", "LKP", "presentasi"
- Work with deadlines: mentions course + any time reference

REJECT AS NON-ASSIGNMENT (class schedules/announcements):
- Pure attendance: "praktikum besok" (no mention of work to submit)
- Class times: "kelas hari Rabu jam 10"
- Meeting announcements: "zoom meeting tomorrow"
- Course content: "baca chapter 5" WITHOUT submission requirement

Key principle: If unclear whether work must be submitted, treat as ASSIGNMENT.
It's better to create an assignment that can be deleted than miss a real one.

When message just says "TUGAS" - this IS assignment-related. The AI will ask for clarification to get missing details.

═══════════════════════════════════════════════════════════════════
PRIORITY 2: CLASSIFICATION LOGIC
═══════════════════════════════════════════════════════════════════

STEP A: Check for QUOTED MESSAGE context (HIGHEST PRIORITY)

When user replies to a quoted assignment message:
→ Look for: time/date mentions, change indicators, or clarifications
→ Reference = QUOTED message (extract course + title)
→ Changes = REPLY message (extract new info)
→ Don't hallucinate fields - only set what user actually provides

UPDATE examples:
- "deadline hari ini" → set new_deadline only
- "diundur besok" → set new_deadline only
- "jam 14:00" → set new_deadline with that time

NOT updates (NEW assignments):
- Reply announces different course/topic
- Reply says "ada lagi" (another assignment)

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

UPDATE_ASSIGNMENT signals (HIGH PRIORITY - check FIRST):
1. EXPLICIT REFERENCE PATTERNS:
   - "tugas yang [course]" → references existing assignment
   - "assignment yang [course]" → references existing assignment  
   - "tugas [course] yang" → references existing assignment
   - "[course] yang deadline" → references existing assignment
   - "yang [course] deadline" → references existing assignment

2. TEMPORAL REFERENCE WITH COURSE:
   - "tugas kemarin" → yesterday's assignment
   - "[course] kemarin" → yesterday's course assignment
   - "yang tadi" → earlier mentioned assignment
   
3. DEADLINE CHANGES (CLEAR UPDATE SIGNALS):
   - "deadline itu besok" → changing existing deadline
   - "diundur besok" → postponing deadline
   - "dimajuin besok" → moving deadline forward
   - "deadline [course] besok" → updating course deadline
   
4. EXPLICIT CHANGE LANGUAGE:
   - "berubah", "ganti", "revisi", "update", "correction"
   - "diganti", "diubah", "diperbaiki"

NEW_ASSIGNMENT signals (only if NO update signals):
- "ada tugas baru" → explicit new announcement
- Clear announcement with full details (course + description + deadline)
- Sequential numbering not in DB (LKP 15 when only LKP 14 exists)
- "ada lagi" when replying → NEW, not update

DECISION TREE:
1. Check for "yang [course]" or "[course] yang" pattern → UPDATE
2. Check for "deadline itu/yang" with course reference → UPDATE  
3. Check for temporal reference ("kemarin", "tadi") → UPDATE
4. Check for explicit change words → UPDATE
5. Only if NONE of above → consider NEW

Examples:
- "tugas yang rpl deadline itu besok yaa" → UPDATE (has "yang rpl" + "deadline itu")
- "tugas rpl deadline besok" → Could be NEW (no reference word "yang")
- "ada tugas baru rpl" → NEW (explicit "baru")
- "LKP 15 deadline berubah" → UPDATE (explicit change)

CRITICAL: When message has:
- Reference word ("yang", "itu", "tadi", "kemarin") + 
- Course name +  
- Deadline mention
→ This is almost ALWAYS an UPDATE, not a new assignment

═══════════════════════════════════════════════════════════════════
PRIORITY 3: EXTRACTION RULES
═══════════════════════════════════════════════════════════════════

TITLE EXTRACTION (SPECIFIC, 2-40 chars):
BAD: "Tugas", "Assignment", "Praktikum" (too generic)
GOOD: "LKP 15", "Quiz 3", "Tugas Berpasangan", "Coding on Paper"

Rules: Use identifiers (numbers/names) → descriptive type → never just "Tugas"

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

4. Keep titles CONCISE (2 words minimum, 40 characters maximum)
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
═══════════════════════════════════════════════════════════════════
CRITICAL "all" RULE (HIGHEST PRIORITY):
   If "all"/"semua"/"everyone"/"semuanya" appears ANYWHERE:
   → IMMEDIATELY return ["all"] and STOP processing other parallel codes
   → IGNORE all other parallel mentions (k1, k2, etc.)
   → ["all", "k2"] is ALWAYS WRONG 
   → ["all"] is ALWAYS RIGHT 
═══════════════════════════════════════════════════════════════════

Valid codes: k1-k4, p1-p4, r1-r4, all

DECISION TREE:
1. Does message contain "all"/"semua"/"everyone"? 
   → YES: Return ["all"] immediately, skip step 2
   → NO: Proceed to step 2
2. Extract specific parallel codes (k1, k2, etc.)

Examples:
- "semua parallel, k2" → ["all"] (step 1: "semua" found, stop)
- "untuk all parallel" → ["all"] (step 1: "all" found, stop)
- "k1 dan k2" → ["k1","k2"] (step 1: no "all", step 2: extract codes)
- "GRAFKOM K2" → ["k2"] (step 1: no "all", step 2: extract from title)
- "All students including k1" → ["all"] (step 1: "all" found, stop)
- No mention + no context → []

- Return as ARRAY (assignments can target multiple parallels)
- Extract from message or use context hint if not explicitly mentioned
- Look in course abbreviation section (e.g., "GRAFKOM K2" → ["k2"])

DESCRIPTION FIELD (MANDATORY):
- NEVER leave empty or null
- Generate meaningful description from message content
- Include submission details if mentioned
- If minimal info: "[Course] assignment - [brief context]"
- If has enough info: "[helpful context outside of the title]"

═══════════════════════════════════════════════════════════════════
OUTPUT FORMATS
═══════════════════════════════════════════════════════════════════

UNRECOGNIZED:
{{
  "type": "unrecognized",
  "category": "informal" | "academic_related",
  "reason": string | null
}}

Rules:
- category="informal": No academic context whatsoever
- category="academic_related": Has academic context but fails REQUIRED INFORMATION THRESHOLD
- reason: MANDATORY for academic_related (state which required field is missing), null for informal. CONSICE IN ONE SENTENCE.

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
  "new_title": More informative title OR null,
  "new_description": More description title OR null,
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
13. UNRECOGNIZED CATEGORIES:
    - informal: Casual chat with no academic context
    - academic_related: Mentions courses/classes but no assignment deliverable

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
    
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    
    let assignments_list = assignments.iter().enumerate().map(|(i, a)| {
        let parallel_str = if a.parallel_codes.is_empty() {
            "N/A".to_string()
        } else {
            format!("[{}]", a.parallel_codes.join(", "))
        };
        
        let course_name = a.course_id
            .and_then(|id| course_map.get(&id))
            .map(|s| s.as_str())
            .unwrap_or("Unknown");
        
        let age_hours = now.signed_duration_since(a.created_at).num_hours();

        format!("{}. {} | \"{}\" | Parallels: {} | Created: {} hours ago | ID: {}", 
            i + 1, course_name, a.title, parallel_str, age_hours, a.id)
    }).collect::<Vec<_>>().join("\n");
    
    let parallel_info = if parallel_codes.is_empty() {
        "".to_string()
    } else {
        format!("\nParallel codes mentioned: [{}]", parallel_codes.join(", "))
    };
    
    format!(
        r#"Match this update message to an existing assignment.

UPDATE MESSAGE:
"{}"

Keywords extracted: {:?}{}

CANDIDATE ASSIGNMENTS:
{}

═══════════════════════════════════════════════════════════════════
EXAMPLES OF GOOD MATCHING
═══════════════════════════════════════════════════════════════════

Example 1: Generic title match
Update: "tugas yang rpl deadline besok"
Keywords: ["rpl"]
Candidates:
1. Rekayasa Perangkat Lunak | "Tugas RPL" | Parallels: [k1,k2] | 5 hours ago
→ Match: #1 (only recent RPL assignment, generic title matches)

Example 2: Specific identifier match
Update: "LKP 14 diundur"
Keywords: ["lkp", "14"]
Candidates:
1. Pemrograman | "LKP 14" | Parallels: [k1] | 2 days ago
2. Pemrograman | "LKP 15" | Parallels: [k1] | 0 days ago
→ Match: #1 (exact number match)

Example 3: Semantic match
Update: "quiz kalkulus yang kemarin dipindah"
Keywords: ["quiz", "kalkulus"]
Candidates:
1. Kalkulus | "Quiz 3" | Parallels: [k2] | 1 day ago
2. Kalkulus | "Problem Set 5" | Parallels: [k2] | 3 days ago
→ Match: #1 (quiz type matches, recent)

Example 4: No match
Update: "tugas grafkom deadline besok"
Keywords: ["grafkom"]
Candidates:
1. Pemrograman | "LKP 14" | Parallels: [k1] | 2 days ago
2. Kalkulus | "Quiz 3" | Parallels: [k2] | 1 day ago
→ Match: NONE (no Grafkom assignments)

═══════════════════════════════════════════════════════════════════
MATCHING REASONING APPROACH
═══════════════════════════════════════════════════════════════════

Think step-by-step:

1. COURSE MATCH: Which candidates match the course keywords?
   - Look for course name mentions in keywords
   - Consider course abbreviations (RPL = Rekayasa Perangkat Lunak)

2. RECENCY: Prioritize assignments created within last 48 hours (2 days)
   - Updates typically reference recent announcements

3. TITLE SIMILARITY: Does the title match the update context?
   - Generic titles ("Tugas RPL") often reference the only recent assignment
   - Specific identifiers (numbers) must match exactly
   - Semantic similarity (quiz vs quiz, lab vs lab)

4. PARALLEL OVERLAP: If parallels mentioned, do they overlap?
   - Empty parallels = match anything
   - [k1, k2] matches [k1] or [k2] or [k1, k2]
   - [k1] does NOT match [k3]

5. CONFIDENCE: How certain are you?
   - HIGH: Clear match (course + title + recent + parallels match)
   - MEDIUM: Probable match (course matches, similar title)
   - LOW: Uncertain (multiple candidates or weak signals)

═══════════════════════════════════════════════════════════════════
OUTPUT FORMAT
═══════════════════════════════════════════════════════════════════

Return JSON with reasoning:

{{
  "assignment_id": "uuid"|null,
  "confidence": "high"|"medium"|"low",
  "reason": "single line reason on why you think so"
}}

Rules:
- Only return ID with "high" confidence
- If "medium" or "low" → return null (safer to not match)
- Include your reasoning for transparency

Think through the steps above, then provide your answer."#,
        changes,
        keywords,
        parallel_info,
        assignments_list
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
    
    let assignments_list = existing_assignments.iter().enumerate()
        .map(|(i, a)| {
            let parallel_str = if a.parallel_codes.is_empty() {
                "all".to_string()
            } else {
                a.parallel_codes.join(", ")
            };
            
            let course = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown");
            
            let desc_text = a.description.as_str();
            
            format!(
                "Assignment #{}:\n  Course: {}\n  Title: \"{}\"\n  Parallels: {}\n  Description: {}\n  ID: {}\n", 
                i + 1, course, a.title, parallel_str, desc_text, a.id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    let parallel_info = if parallel_codes.is_empty() {
        "all".to_string()
    } else {
        parallel_codes.join(", ")
    };
    
    format!(
        r#"You are analyzing whether a new assignment announcement is a DUPLICATE of an existing assignment in the database.

═══════════════════════════════════════════════════════════════════
PROBLEM DEFINITION
═══════════════════════════════════════════════════════════════════

A DUPLICATE means: The new message is re-announcing THE SAME ASSIGNMENT that already exists in the database. This happens when:
- A professor sends a reminder about an existing assignment
- An assignment is reposted with updated information (deadline change, clarification, etc.)
- The same assignment is announced to additional parallel classes

A NON-DUPLICATE means: The new message announces a DIFFERENT ASSIGNMENT that should be stored separately. This happens when:
- It's a new sequential assignment (Quiz 2 after Quiz 1, Lab 5 after Lab 4)
- It's a different type of assignment for the same course (Quiz vs Lab, Project vs Homework)
- It's the same title but for non-overlapping class sections (different parallel codes)

═══════════════════════════════════════════════════════════════════
NEW ASSIGNMENT TO ANALYZE
═══════════════════════════════════════════════════════════════════

Course: {}
Title: "{}"
Parallel Codes: {}
Description: {}

═══════════════════════════════════════════════════════════════════
EXISTING ASSIGNMENTS IN DATABASE
═══════════════════════════════════════════════════════════════════

{}

═══════════════════════════════════════════════════════════════════
REASONING PRINCIPLES
═══════════════════════════════════════════════════════════════════

1. SEMANTIC IDENTITY
   Understand what the assignment actually IS, not just surface text matching:
   - "Tugas Pemrograman" and "Programming Assignment" could be the same thing
   - "Lab Report 3" and "Laporan Lab 3" refer to the same deliverable
   - Look beyond literal string matching to understand the actual work being requested

2. SEQUENTIAL CONTEXT
   Assignments often come in numbered sequences:
   - "Quiz 1", "Quiz 2", "Quiz 3" are DIFFERENT assignments in a series
   - "LKP 14" and "LKP 15" are DIFFERENT lab assignments
   - "Week 3 Assignment" and "Week 4 Assignment" are DIFFERENT
   - However: "Quiz Minggu 3" and "Week 3 Quiz" could be the SAME assignment in different languages

3. ASSIGNMENT TYPE TAXONOMY
   Different types of academic work are distinct:
   - Quiz ≠ Lab ≠ Homework ≠ Exam ≠ Project ≠ Essay
   - "Tugas" (general homework) vs "Kuis" (quiz) vs "Praktikum" (lab) are different types
   - Even if they have the same number or timing, different types = different assignments

4. PARALLEL CLASS LOGIC
   Parallel codes represent different sections/classes:
   - Assignments are relevant to specific parallel classes
   - If parallel codes overlap (k1 in both), it could be the same assignment
   - If parallels are "all" or empty, it applies to everyone
   - If there's NO overlap (k1,k2 vs k3), they're for different sections = not duplicate
   - Exception: A re-announcement might ADD parallel codes to an existing assignment

5. DESCRIPTION AS SEMANTIC CONTEXT
   The description provides crucial context about what the work actually entails:
   - Use it to understand the actual requirements and deliverables
   - Compare the substance of the work, not just keywords
   - If descriptions discuss fundamentally different topics/requirements → different assignments
   - If descriptions are semantically similar but differently worded → could be duplicate
   - Missing descriptions shouldn't prevent duplicate detection if other signals are clear

6. CONFIDENCE CALIBRATION
   Be precise about your certainty:
   - HIGH confidence: Clear duplicate with strong signals (same course, same work identity, parallel overlap)
   - MEDIUM confidence: Likely duplicate but some ambiguity exists
   - LOW confidence: Significant uncertainty, multiple interpretations possible
   
   Default to "not duplicate" when confidence is not HIGH. Creating a duplicate entry is safer than missing a new assignment.

═══════════════════════════════════════════════════════════════════
REASONING EXAMPLES
═══════════════════════════════════════════════════════════════════

Example 1: Generic Title with Overlap
New: [RPL] "Tugas RPL" | k1,k2
Old: [RPL] "Tugas RPL" | k1

Analysis:
- Same course ✓
- Parallel overlap (k1 appears in both) ✓
- Generic identical title with no distinguishing features
- No sequential numbers or type differences
- Likely a re-announcement extending to k2

Conclusion: DUPLICATE (high confidence)

---

Example 2: Sequential Numbering
New: [Pemrograman] "LKP 15"
Old: [Pemrograman] "LKP 14"

Analysis:
- Same course ✓
- Different sequential numbers (14 vs 15)
- "LKP" = Lab assignments in a series
- Each number represents a distinct lab exercise

Conclusion: NOT DUPLICATE (high confidence)

---

Example 3: Semantic Equivalence Across Languages
New: [Physics] "Laboratory Report 3" | all
Old: [Physics] "Laporan Lab 3" | k1

Analysis:
- Same course ✓
- Parallel overlap (all includes k1) ✓
- "Laboratory Report" = "Laporan Lab" (English/Indonesian)
- Same number (3)
- Semantically identical work

Conclusion: DUPLICATE (high confidence)

---

Example 4: No Parallel Overlap
New: [Struktur Data] "Tugas Besar" | k3
Old: [Struktur Data] "Tugas Besar" | k1,k2

Analysis:
- Same course ✓
- Same title ✓
- BUT: No parallel overlap (k3 vs k1,k2)
- Different class sections = different assignment instances
- These are separate assignments for separate classes

Conclusion: NOT DUPLICATE (high confidence)

---

Example 5: Different Assignment Types
New: [Grafkom] "Quiz 2" | k1
Old: [Grafkom] "Lab 2" | k1

Analysis:
- Same course ✓
- Same number (2) ✓
- Different types: Quiz vs Lab
- Quiz and Lab are fundamentally different assessment types
- Same timing doesn't make them the same assignment

Conclusion: NOT DUPLICATE (high confidence)

---

Example 6: Description Provides Clarity
New: [Database] "Project" | all | desc: "Design and implement a library management system with CRUD operations"
Old: [Database] "Project" | k1 | desc: "Create an e-commerce database with transaction handling"

Analysis:
- Same course ✓
- Same generic title ("Project") ✓
- Parallel overlap (all includes k1) ✓
- BUT: Descriptions reveal completely different scopes and requirements
- Library system vs E-commerce system = different projects

Conclusion: NOT DUPLICATE (high confidence)

═══════════════════════════════════════════════════════════════════
YOUR TASK
═══════════════════════════════════════════════════════════════════

Analyze the new assignment against existing assignments using the principles above.

Think step-by-step:
1. What course is this for? Does it match any existing assignments?
2. What parallel codes apply? Is there overlap with candidates?
3. What is the semantic identity of this assignment? (What work is actually being requested?)
4. Are there sequential indicators (numbers, dates) that distinguish it?
5. What type of assignment is this? (quiz, lab, homework, etc.)
6. What do the descriptions tell us about the actual work required?
7. Based on all factors, is this the SAME assignment or a DIFFERENT one?

Respond with valid JSON:
{{
  "is_duplicate": boolean,
  "confidence": "high" | "medium" | "low",
  "reasoning": "Explain your analysis. ONLY IN A SINGLE SENTENCE.",
  "matched_assignment_id": "uuid of matched assignment or null"
}}

Critical rules:
- Only set is_duplicate: true if confidence is "high"
- When in doubt, mark as NOT duplicate (safer to create new entry)
- Consider ALL principles, not just surface matching"#,
        course_name,
        title,
        parallel_info,
        description,
        assignments_list
    )
}