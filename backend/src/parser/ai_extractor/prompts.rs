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
- category="informal": No academic context (social chat, memes, greetings)
- category="academic_related": Has academic context but fails THREE REQUIREMENTS
- reason: Required for academic_related (explain which requirement fails), omit for informal

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
        
        let age_days = now.signed_duration_since(a.created_at).num_days();
        
        format!("{}. {} | \"{}\" | Parallels: {} | Created: {} days ago | ID: {}", 
            i + 1, course_name, a.title, parallel_str, age_days, a.id)
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
1. Rekayasa Perangkat Lunak | "Tugas RPL" | Parallels: [k1,k2] | 1 day ago
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

2. RECENCY: Prioritize assignments created within last 7 days
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
  "reasoning": "Step-by-step explanation of your decision"
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
                "none".to_string()
            } else {
                a.parallel_codes.join(", ")
            };
            
            let course = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|s| s.as_str())
                .unwrap_or("Unknown");
            
            format!("{}. Course: {} | Title: \"{}\" | Parallels: {} | ID: {}", 
                i + 1, course, a.title, parallel_str, a.id)
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    let parallel_info = if parallel_codes.is_empty() {
        "none".to_string()
    } else {
        parallel_codes.join(", ")
    };
    
    format!(
        r#"Is this a duplicate (re-announcement) of an existing assignment?

NEW ASSIGNMENT:
Course: {}
Title: "{}"
Parallels: {}

EXISTING CANDIDATES:
{}

═══════════════════════════════════════════════════════════════════
LEARN FROM EXAMPLES
═══════════════════════════════════════════════════════════════════

Example 1: DUPLICATE (same generic title)
New: Course: RPL | Title: "Tugas RPL" | Parallels: k1,k2
Existing: Course: RPL | Title: "Tugas RPL" | Parallels: k1
Reasoning:
- Same course ✓
- Same generic title ✓
- Parallels overlap (k1,k2 includes k1) ✓
- No distinguishing identifier
→ Duplicate: YES (re-announcement of same work)

Example 2: NOT DUPLICATE (different numbers)
New: Course: Pemrograman | Title: "LKP 15" | Parallels: k1
Existing: Course: Pemrograman | Title: "LKP 14" | Parallels: k1
Reasoning:
- Same course ✓
- Different sequential numbers (15 vs 14) ✗
- These are different lab assignments
→ Duplicate: NO

Example 3: DUPLICATE (semantic match)
New: Course: Physics | Title: "Laboratory Report 3" | Parallels: none
Existing: Course: Physics | Title: "Laporan Lab 3" | Parallels: k1
Reasoning:
- Same course ✓
- Same work, different language (Lab Report = Laporan Lab) ✓
- Same number (3) ✓
→ Duplicate: YES

Example 4: NOT DUPLICATE (different types)
New: Course: Grafkom | Title: "Quiz 2" | Parallels: k1
Existing: Course: Grafkom | Title: "Lab 2" | Parallels: k1
Reasoning:
- Same course ✓
- Different assignment types (Quiz ≠ Lab) ✗
- Even though same number, different work
→ Duplicate: NO

Example 5: NOT DUPLICATE (no parallel overlap)
New: Course: Strukdat | Title: "Tugas Besar" | Parallels: k3
Existing: Course: Strukdat | Title: "Tugas Besar" | Parallels: k1,k2
Reasoning:
- Same course ✓
- Same title ✓
- No parallel overlap (k3 ≠ k1,k2) ✗
→ Duplicate: NO (different sections)

═══════════════════════════════════════════════════════════════════
REASONING APPROACH
═══════════════════════════════════════════════════════════════════

Think step-by-step:

1. COURSE: Must match exactly
   - Different courses → NOT duplicate

2. PARALLELS: Must overlap
   - [k1,k2] overlaps [k1] → can be duplicate
   - [k1] vs [k3] → NOT duplicate (different sections)
   - Empty parallels = unspecified = overlaps anything

3. IDENTIFIERS: Check for numbers/names
   - "LKP 15" vs "LKP 14" → NOT duplicate (sequential)
   - "Quiz 3" vs "Quiz 3" → might be duplicate
   - "Tugas RPL" vs "Tugas RPL" → likely duplicate (generic)

4. ASSIGNMENT TYPE: Must be same type
   - Quiz, Lab, Homework, Exam, Project
   - Different types → NOT duplicate

5. CONFIDENCE:
   - HIGH: Clear duplicate (same course, generic title, overlapping parallels)
   - MEDIUM: Similar but uncertain
   - LOW: Probably different

═══════════════════════════════════════════════════════════════════
OUTPUT FORMAT
═══════════════════════════════════════════════════════════════════

Return JSON:

{{
  "is_duplicate": boolean,
  "confidence": "high"|"medium"|"low",
  "reasoning": "Your step-by-step analysis",
  "matched_assignment_id": "uuid"|null
}}

Rules:
- Only mark as duplicate with HIGH confidence
- When uncertain → is_duplicate: false (safer to create new)
- Provide clear reasoning

Think through each step, then provide your answer."#,
        course_name,
        title,
        parallel_info,
        assignments_list
    )
}