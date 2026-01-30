use crate::models::{Assignment, AssignmentWithCourse};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{Utc, FixedOffset, Duration}; 
use super::context_builder::{MessageContext};

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
        let mut hints = String::from("\n\n# CONTEXT HINTS FROM MESSAGE ANALYSIS\n\n");

        hints.push_str(&format!(
            "**Confidence:** {:.0}% | **Source:** {}\n\n",
            ctx.parallel_confidence * 100.0,
            ctx.parallel_source
        ));
        
        if let Some(ref quoted) = ctx.quoted_message_summary {
            hints.push_str("## QUOTED MESSAGE\n");
            hints.push_str(&format!("  {}\n", quoted));
            hints.push_str("  → User is likely updating/referencing this assignment\n\n");
        }
        
        if !ctx.course_hints.is_empty() {
            hints.push_str("## DETECTED COURSES\n");
            for course_hint in &ctx.course_hints {
                hints.push_str(&format!("- **{}**\n", course_hint.course_name));
                
                if !course_hint.parallel_codes.is_empty() {
                    hints.push_str(&format!("  - Parallels: [{}]\n", course_hint.parallel_codes.join(", ")));
                }
                
                if !course_hint.parallel_schedules.is_empty() {
                    hints.push_str("  - Next meetings:\n");
                    for ps in &course_hint.parallel_schedules {
                        if let Some(ref meeting) = ps.next_meeting {
                            hints.push_str(&format!("    - {} → {}\n", ps.parallel_code.to_uppercase(), meeting));
                        }
                    }
                }
            }
            hints.push('\n');
        }
        
        if let Some(ref deadline) = ctx.deadline_hint {
            hints.push_str(&format!("**Suggested deadline:** {}\n\n", deadline));
        }
        
        hints.push_str("## USAGE RULES\n");
        hints.push_str("- Hints are SUGGESTIONS based on patterns\n");
        hints.push_str("- QUOTED MESSAGE present → likely an update to that assignment\n");
        hints.push_str("- 'ketika praktikum'/'during class' → use next meeting time\n");
        hints.push_str("- Explicit times ('besok jam 13') → use that time with schedule date if available\n");
        hints.push_str("- Different meeting times per parallel → split into separate assignments\n");
        hints
    } else {
        String::new()
    };
    
    format!(
        r#"
# CRITICAL RESPONSE REQUIREMENTS
1. You MUST output COMPLETE, VALID JSON
2. NEVER truncate your response mid-JSON
3. If approaching token limit, SIMPLIFY but COMPLETE the JSON
4. Close ALL brackets, braces, and quotes
5. Test: Your response should parse as valid JSON

You are a bilingual (Indonesian first/English) academic assistant that extracts structured assignment information from WhatsApp messages. Fill fields in Indonesian.

# CORE TASK: MESSAGE CLASSIFICATION

Classify the message into ONE category using this decision tree:

1. UNRECOGNIZED → Not assignment-related or missing required information
2. MULTIPLE_ASSIGNMENTS → Contains 2+ distinct assignments
3. ASSIGNMENT_INFO → Single new assignment announcement  
4. ASSIGNMENT_UPDATE → Modifying existing assignment

**Current time:** {} (GMT+7)
**Today:** {}
**Message:** "{}"

**Available courses:**
{}

**Recent assignments (max 10):**
{}{}

**Temporal references:**
- Besok/Tomorrow → {} 23:59
- Lusa/Day after tomorrow → {} 23:59
- Minggu depan/Next week → {} 23:59

---

# DECISION TREE: CLASSIFY THE MESSAGE

## STEP 1: ASSIGNMENT VALIDATION

An assignment requires ALL THREE:
1. Students must CREATE something (not just attend/read)
2. Concrete DELIVERABLE exists (something to submit)
3. EVALUATION expected (will be checked/graded)

Decision question: "Will instructor check if students submitted something?"
→ YES = Assignment | NO = Not assignment

**ASSIGNMENTS:** Lab reports, homework, essays, quizzes, exams, projects, coding tasks, problem sets
**Pattern:** "submit X", "kumpulkan X", "deadline X", "turn in X", "tugas dikumpulkan ketika praktikum"

**NOT ASSIGNMENTS:** Class sessions, attendance, announcements, reading (without submission), resources
**Pattern:** "praktikum besok", "lecture tomorrow", "hadir ke lab", "baca chapter 5"

**CRITICAL:** "praktikum besok" = class session | "tugas dikumpulkan ketika praktikum" = assignment

- IF message not assignment-related → UNRECOGNIZED (category: informal)
- IF assignment-related BUT missing course name OR identifier → UNRECOGNIZED (category: academic_related, reason: "Missing [course/identifier]")

## STEP 2: COUNT ASSIGNMENTS

Check for multiple distinct assignments:
- Numbered lists: "1. Pemrog LKP 14... 2. Kalkulus Quiz..."
- Multiple courses: "Pemrog dan Fisika ada tugas"
- Explicit count: "ada 2 tugas", "3 assignments"

Apply THREE REQUIREMENTS to EACH item - only count actual assignments.

**SPECIAL CASE - Different deadlines per parallel:**
IF announcement targets multiple parallels AND deadline is "ketika praktikum"/"during class":
- Check context hints for meeting times
- IF different meeting times → SPLIT into separate assignments
- Group parallels with same deadline together

Example:
```
Message: "P1, P2, P3 submit ketika praktikum"
Context: P1→Thu 10:00, P2→Thu 13:00, P3→Tue 13:00
Result: TWO assignments
  [{{"parallel_codes": ["p3"], "deadline": "2026-01-07 13:00", ...}},
   {{"parallel_codes": ["p1", "p2"], "deadline": "2026-01-09 10:00", ...}}]
```

- IF 2+ assignments → MULTIPLE_ASSIGNMENTS
- ELSE continue to STEP 3

## STEP 3: NEW vs UPDATE DETECTION

**CONTEXT-BASED DECISION:**

IF quoted message present AND (quoted is assignment):
  Check reply intent:
  
  **UPDATE signals:**
  - Correction language: "typo", "salah", "sorry", "seharusnya", "correction"
  - Clarification: "maksudnya", "more specifically", "lebih tepatnya"
  - Change verbs: "diundur", "berubah", "extended", "diperpanjang"
  - Date/time additions without "ada tugas baru"
  - Course reassignment: "pindahin ke", "seharusnya [course]", "diubah ke [course]"
  
  **NEW signals:**
  - Explicit: "ada tugas lagi", "tugas baru", "another assignment"
  - Different course than quoted
  - Different requirements than quoted
  
  - IF has UPDATE signals → ASSIGNMENT_UPDATE
  - IF has NEW signals → ASSIGNMENT_INFO
  - IF pure acknowledgment → UNRECOGNIZED (informal)

IF no quoted message:
  Check linguistic markers:
  
  **REFERENCE markers (UPDATE):**
  - Demonstratives: "tugas yang [course]", "[course] yang", "itu [course]"
  - Temporal: "kemarin", "tadi", "earlier"
  - Definite article: "deadline itu", "tugas yang"
  - Change verbs: "berubah", "diubah", "diganti", "diundur"
  
  **ANNOUNCEMENT markers (NEW):**
  - Full details without reference words
  - Sequential progression: "LKP 15" when DB has "LKP 14"
  - Existence verbs: "ada tugas baru"
  
  - IF reference markers + change info → ASSIGNMENT_UPDATE
  - IF announcement markers → ASSIGNMENT_INFO
  - IF ambiguous → default to ASSIGNMENT_INFO (safer)

**VALIDATION for UPDATE:**
- Course must be identifiable
- Must have distinguishing feature (number/type/temporal reference)
- Change information must be present
- NEVER match across different courses

---

# EXTRACTION RULES

## Platform references:
- "di class"/"on class" → usually refers to class.ipb.ac.id LMS, not physical classroom
- "upload ke class" → submission to online platform
- Context: "class" as location often means the learning management system, while "kelas" means a physical classroom at a lecture.

## TITLE (2-3 words minimum, max 40 chars, MUST BE SPECIFIC)

Single-word titles are prohibited. Every title must have a distinguishing element.

**Priority order:**
1. Use identifier: "LKP 15", "Quiz 3", "Problem Set 5"
2. Use descriptive type: "Tugas Berpasangan", "Laporan Praktikum"
3. Add topic if mentioned: "Quiz Chapter 5", "Tugas Modul 2"
4. Add course as minimum context: "Quiz Grafkom", "Tugas RPL"

**PROHIBITED:** Do not output single-word titles
- NOT "Quiz" → USE "Quiz [Course/Topic]"
- NOT "Tugas" → USE "Tugas [Type/Topic/Course]"
- NOT "Kuis" → USE "Kuis [Course/Topic]"

**Examples:**
- Message: "quiz grafkom besok" → "Quiz Grafkom" (not "Quiz")
- Message: "tugas chapter 5" → "Tugas Chapter 5" (not "Tugas")
- Message: "kuis kalkulus" → "Kuis Kalkulus" (not "Kuis")

## DEADLINE (format: YYYY-MM-DD HH:MM)

**Priority order:**

1. **WHEN-DURING patterns** ("ketika praktikum", "saat kelas", "during class"):
   → Use EXACT schedule time from context hints
   Example: "dikumpulkan ketika praktikum" + Hint "K1: 2026-01-12 08:00" → "2026-01-12 08:00"

2. **EXPLICIT TIME with relative date** ("besok jam 13", "Jumat pukul 14:00"):
   → IF context hint date within 7 days of relative date: Use hint DATE with message TIME
   → ELSE: Use calculated relative date with message TIME
   Example: "besok jam 13:00" + Hint "2026-01-12 08:00" → "2026-01-12 13:00"

3. **DATE ONLY** ("besok", "Friday", "minggu depan"):
   → Calculate date, use 23:59
   Example: "deadline besok" → "2026-01-06 23:59"

4. **NO deadline info:**
   → Use null (do NOT guess)

## PARALLEL CODES (array: ["k1", "k2", ...])

Valid codes: k1-k4, p1-p4, r1-r4, all

**Decision tree:**
1. Does message contain "all"/"semua"/"everyone"? → Return ["all"], STOP
2. Extract specific codes from message text
3. IF no codes in message: Check context hints
4. IF still none: Return []

**Pattern recognition:**
- "K2 P2" → ["k2", "p2"]
- "untuk k1" → ["k1"]
- "semua" → ["all"]
- Course abbreviation: "GRAFKOM K2" → ["k2"]

## DESCRIPTION (MANDATORY, never null)

- Extract from message content
- Include submission details if mentioned
- If minimal info: "[Course] assignment - [brief context]"
- If good info: Concise summary of requirements

---

# OUTPUT SCHEMAS

Return valid JSON only. No markdown, no explanations.

**UNRECOGNIZED:**
```json
{{"type": "unrecognized", "category": "informal|academic_related", "reason": string|null}}
```
- category="informal" → no academic context
- category="academic_related" → academic but missing course/identifier
- reason=null for informal, required string for academic_related

**MULTIPLE_ASSIGNMENTS:**
```json
{{"type": "multiple_assignments", "assignments": [
  {{"course_name": string, "title": string, "deadline": string|null, "description": string, "parallel_codes": array}},
  ...
]}}
```

**ASSIGNMENT_INFO:**
```json
{{"type": "assignment_info", "course_name": string, "title": string, "deadline": string|null, "description": string, "parallel_codes": array}}
```

**ASSIGNMENT_UPDATE:**
```json
{{"type": "assignment_update", "reference_keywords": array, "changes": string, "new_deadline": string|null, "new_title": string|null, "new_description": string|null, "new_course_name": string|null, "parallel_codes": array}}
```
- reference_keywords: [course, identifier] from quoted/referenced assignment
- changes: what user said in reply/update message
- new_*: Only set fields that user explicitly changed
- new_course_name: Only if user is explicitly changing the course (e.g. "pindahin ke RPL", "seharusnya masuk Grafkom")
- parallel_codes: Only if user is changing target parallels

---

# REASONING CHECKLIST

Before outputting, verify:
1. Applied THREE REQUIREMENTS test for assignment validation
2. Checked for quoted message context first
3. Used context hints appropriately (suggestions, not commands)
4. For "ketika praktikum" → used exact schedule time
5. For "besok jam X" → used schedule date with message time if available
6. Split assignments when parallels have different meeting times
7. Title is specific (has identifier or descriptive type)
8. Parallel codes: checked "all"/"semua" first
9. Description is never null/empty
10. When uncertain NEW vs UPDATE → chose NEW

**Common errors to avoid:**
- Generic titles: "Tugas", "Assignment"
- Invented deadlines when none mentioned
- Matching updates across different courses
- Forgetting to split different parallel meeting times
- Extracting context hints instead of message content for parallel codes"#,
        current_datetime,
        current_date,
        text,
        available_courses,
        assignments_context,
        context_hints,
        tomorrow_str,
        lusa_str,
        next_week_str
    )
}

pub fn build_update_prompt(
    update_message: &str,
    target_assignment: &AssignmentWithCourse,
    current_datetime: &str,
    current_date: &str,
    context: Option<&MessageContext>,
) -> String {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    
    let tomorrow_str = (now + Duration::days(1)).format("%Y-%m-%d").to_string();
    let lusa_str = (now + Duration::days(2)).format("%Y-%m-%d").to_string();
    let next_week_str = (now + Duration::days(7)).format("%Y-%m-%d").to_string();
    
    let context_hints = if let Some(ctx) = context {
        let mut hints = String::from("\n\n# CONTEXT HINTS FROM MESSAGE ANALYSIS\n\n");

        hints.push_str(&format!(
            "**Confidence:** {:.0}% | **Source:** {}\n\n",
            ctx.parallel_confidence * 100.0,
            ctx.parallel_source
        ));
        
        if let Some(ref quoted) = ctx.quoted_message_summary {
            hints.push_str("## QUOTED MESSAGE\n");
            hints.push_str(&format!("  {}\n", quoted));
            hints.push_str("  → User is updating this assignment\n\n");
        }
        
        if !ctx.course_hints.is_empty() {
            hints.push_str("## DETECTED COURSES\n");
            for course_hint in &ctx.course_hints {
                hints.push_str(&format!("- **{}**\n", course_hint.course_name));
                
                if !course_hint.parallel_codes.is_empty() {
                    hints.push_str(&format!("  - Parallels: [{}]\n", course_hint.parallel_codes.join(", ")));
                }
                
                if !course_hint.parallel_schedules.is_empty() {
                    hints.push_str("  - Next meetings:\n");
                    for ps in &course_hint.parallel_schedules {
                        if let Some(ref meeting) = ps.next_meeting {
                            hints.push_str(&format!("    - {} → {}\n", ps.parallel_code.to_uppercase(), meeting));
                        }
                    }
                }
            }
            hints.push('\n');
        }
        
        if let Some(ref deadline) = ctx.deadline_hint {
            hints.push_str(&format!("**Suggested deadline:** {}\n\n", deadline));
        }
        
        hints.push_str("## USAGE RULES\n");
        hints.push_str("- Hints are SUGGESTIONS based on patterns\n");
        hints.push_str("- 'ketika praktikum'/'during class' → use next meeting time\n");
        hints.push_str("- Explicit times ('besok jam 13') → use that time with schedule date if available\n");
        hints.push_str("- Different meeting times per parallel → use EARLIEST time, keep ALL parallels together\n");
        hints
    } else {
        String::new()
    };
    
    let parallel_display = if target_assignment.parallel_codes.is_empty() {
        "N/A".to_string()
    } else {
        format!("[{}]", target_assignment.parallel_codes.join(", "))
    };
    
    let deadline_display = target_assignment.deadline
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "N/A".to_string());
    
    let description_display = target_assignment.description
        .as_deref()
        .unwrap_or("N/A");
    
    format!(
        r#"
# CRITICAL RESPONSE REQUIREMENTS
1. You MUST output COMPLETE, VALID JSON
2. NEVER truncate your response mid-JSON
3. If approaching token limit, SIMPLIFY but COMPLETE the JSON
4. Close ALL brackets, braces, and quotes
5. Test: Your response should parse as valid JSON

You are a bilingual (Indonesian first/English) academic assistant updating an existing assignment. Fill fields in Indonesian.

# CORE TASK: UPDATE EXISTING ASSIGNMENT

**Current time:** {} (GMT+7)
**Today:** {}
**Update message:** "{}"

**ASSIGNMENT BEING UPDATED:**
**Course:** {}
**Title:** {}
**Current Deadline:** {}
**Parallels:** {}
**Description:** {}

{}

**Temporal references:**
- Besok/Tomorrow → {} 23:59
- Lusa/Day after tomorrow → {} 23:59
- Minggu depan/Next week → {} 23:59

---

# EXTRACTION RULES

## TITLE (2-3 words minimum, max 40 chars, MUST BE SPECIFIC)

Single-word titles are prohibited. Every title must have a distinguishing element.

**Priority order:**
1. Use identifier: "LKP 15", "Quiz 3", "Problem Set 5"
2. Use descriptive type: "Tugas Berpasangan", "Laporan Praktikum"
3. Add topic if mentioned: "Quiz Chapter 5", "Tugas Modul 2"
4. Add course as minimum context: "Quiz Grafkom", "Tugas RPL"

**PROHIBITED:** Do not output single-word titles
- NOT "Quiz" → USE "Quiz [Course/Topic]"
- NOT "Tugas" → USE "Tugas [Type/Topic/Course]"
- NOT "Kuis" → USE "Kuis [Course/Topic]"

Only set `new_title` if user explicitly changes the title.

## DEADLINE (format: YYYY-MM-DD HH:MM)

**Priority order:**

1. **WHEN-DURING patterns** ("ketika praktikum", "saat kelas", "during class", "pertemuan berikutnya"):
   → Use EXACT schedule time from context hints
   → If multiple parallels with different times: Use EARLIEST time, keep ALL parallels
   Example: "deadline pertemuan berikutnya" + K4: 08:00, P4: 10:00 → Use "2026-02-04 08:00"

2. **EXPLICIT TIME with relative date** ("besok jam 13", "Jumat pukul 14:00"):
   → IF context hint date within 7 days of relative date: Use hint DATE with message TIME
   → ELSE: Use calculated relative date with message TIME
   Example: "besok jam 13:00" + Hint "2026-01-12 08:00" → "2026-01-12 13:00"

3. **DATE ONLY** ("besok", "Friday", "minggu depan"):
   → Calculate date, use 23:59
   Example: "deadline besok" → "2026-01-31 23:59"

4. **NO deadline info:**
   → Use null (do NOT guess)

## PARALLEL CODES (array: ["k1", "k2", ...])

Valid codes: k1-k4, p1-p4, r1-r4, all

**CRITICAL: Extract from UPDATE MESSAGE, NOT from existing assignment**

**Decision tree:**
1. Does message contain "all"/"semua"/"everyone"? → Return ["all"], STOP
2. Extract specific codes from UPDATE MESSAGE text
3. IF no codes in UPDATE MESSAGE: Check context hints
4. IF still none: Return [] (empty = no change)

**Pattern recognition:**
- "untuk K2 P2" → ["k2", "p2"]
- "paralel K1" → ["k1"]
- "semua kelas" → ["all"]
- "diubah ke k3" → ["k3"]

**IMPORTANT:** Empty array means "no change to parallels", NOT "use existing parallels"

## DESCRIPTION (optional for updates)

- Extract from message content if user provides new description
- If no new description mentioned: return null
- Never copy existing description

## COURSE CHANGE (rare)

Only set `new_course_name` if user EXPLICITLY moves assignment to different course:
- "pindahin ke RPL"
- "seharusnya masuk Grafkom"
- "diubah ke Kalkulus"

Otherwise: return null

---

# OUTPUT SCHEMA

Return valid JSON only. No markdown, no explanations.

```json
{{
  "type": "assignment_update",
  "reference_keywords": ["{}"],
  "changes": "brief summary in Indonesian of what user changed",
  "new_deadline": "YYYY-MM-DD HH:MM" | null,
  "new_title": string | null,
  "new_description": string | null,
  "new_course_name": string | null,
  "parallel_codes": [] | ["k1", "k2", ...]
}}
```

**Fields:**
- `reference_keywords`: Always use [course name] of target assignment
- `changes`: What user said in their update message (keep it brief)
- `new_*`: Only set fields that user explicitly changed
- `parallel_codes`: Codes from UPDATE MESSAGE, empty array if no change mentioned

---

# REASONING CHECKLIST

Before outputting, verify:
1. For "ketika praktikum"/"pertemuan berikutnya" → used exact schedule time
2. For "besok jam X" → used schedule date with message time if available
3. If multiple parallels with different meeting times → used EARLIEST time
4. Parallel codes extracted from UPDATE MESSAGE, not existing assignment
5. Title is specific (has identifier or descriptive type) if being changed
6. Description is only set if user provided new description
7. Course change only if explicitly mentioned
8. Empty parallel_codes array means "no change", not "use existing"

**Common errors to avoid:**
- Using existing assignment's parallels instead of message's parallels
- Splitting into multiple assignments (THIS IS AN UPDATE, NOT NEW ASSIGNMENTS)
- Inventing new info not mentioned in update message
- Setting fields to existing values (only set what changed)
- Generic titles like "Tugas", "Assignment" if changing title

**CRITICAL FOR UPDATES:**
- If update mentions "untuk K2 P2" → extract ["k2", "p2"] from MESSAGE
- If update says "deadline pertemuan berikutnya" with K4:08:00, P4:10:00 → use earliest (08:00)
- DO NOT create multiple assignments, this is ONE update
- Only fill fields the user wants to change
"#,
        current_datetime,
        current_date,
        update_message,
        target_assignment.course_name,
        target_assignment.title,
        deadline_display,
        parallel_display,
        description_display,
        context_hints,
        tomorrow_str,
        lusa_str,
        next_week_str,
        target_assignment.course_name
    )
}

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
        format!("\n**Parallel codes mentioned:** [{}]", parallel_codes.join(", "))
    };
    
    format!(
        r#"Match this update message to an existing assignment.

**UPDATE MESSAGE:**
"{}"

**Keywords extracted:** {:?}{}

**CANDIDATE ASSIGNMENTS:**
{}

---

# MATCHING REASONING APPROACH

Think step-by-step:

1. **COURSE MATCH:** Which candidates match the course keywords?
   - Look for course name mentions in keywords
   - Consider course abbreviations (RPL = Rekayasa Perangkat Lunak)

2. **RECENCY:** Prioritize assignments created within last 48 hours (2 days)
   - Updates typically reference recent announcements

3. **TITLE SIMILARITY:** Does the title match the update context?
   - Generic titles ("Tugas RPL") often reference the only recent assignment
   - Specific identifiers (numbers) must match exactly
   - Semantic similarity (quiz vs quiz, lab vs lab)

4. **PARALLEL OVERLAP:** If parallels mentioned, do they overlap?
   - Empty parallels = match anything
   - [k1, k2] matches [k1] or [k2] or [k1, k2]
   - [k1] does NOT match [k3]

5. **CONFIDENCE:** How certain are you?
   - HIGH: Clear match (course + title + recent + parallels match)
   - MEDIUM: Probable match (course matches, similar title)
   - LOW: Uncertain (multiple candidates or weak signals)

---

# OUTPUT FORMAT

Return JSON with reasoning:

```json
{{
  "assignment_id": "uuid"|null,
  "confidence": "high"|"medium"|"low",
  "reason": "single line reason on why you think so"
}}
```

**Rules:**
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
                "**Assignment #{}:**\n  - Course: {}\n  - Title: \"{}\"\n  - Parallels: {}\n  - Description: {}\n  - ID: {}\n", 
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

# PROBLEM DEFINITION

**A DUPLICATE means:** The new message is re-announcing THE SAME ASSIGNMENT that already exists in the database. This happens when:
- A professor sends a reminder about an existing assignment
- An assignment is reposted with updated information (deadline change, clarification, etc.)
- The same assignment is announced to additional parallel classes

**A NON-DUPLICATE means:** The new message announces a DIFFERENT ASSIGNMENT that should be stored separately. This happens when:
- It's a new sequential assignment (Quiz 2 after Quiz 1, Lab 5 after Lab 4)
- It's a different type of assignment for the same course (Quiz vs Lab, Project vs Homework)
- It's the same title but for non-overlapping class sections (different parallel codes)

---

# NEW ASSIGNMENT TO ANALYZE

- **Course:** {}
- **Title:** "{}"
- **Parallel Codes:** {}
- **Description:** {}

---

# EXISTING ASSIGNMENTS IN DATABASE

{}

---

# REASONING PRINCIPLES

## 1. SEMANTIC IDENTITY
Understand what the assignment actually IS, not just surface text matching:
- "Tugas Pemrograman" and "Programming Assignment" could be the same thing
- "Lab Report 3" and "Laporan Lab 3" refer to the same deliverable
- Look beyond literal string matching to understand the actual work being requested

## 2. SEQUENTIAL CONTEXT
Assignments often come in numbered sequences:
- "Quiz 1", "Quiz 2", "Quiz 3" are DIFFERENT assignments in a series
- "LKP 14" and "LKP 15" are DIFFERENT lab assignments
- "Week 3 Assignment" and "Week 4 Assignment" are DIFFERENT
- However: "Quiz Minggu 3" and "Week 3 Quiz" could be the SAME assignment in different languages

## 3. ASSIGNMENT TYPE TAXONOMY
Different types of academic work are distinct:
- Quiz ≠ Lab ≠ Homework ≠ Exam ≠ Project ≠ Essay
- "Tugas" (general homework) vs "Kuis" (quiz) vs "Praktikum" (lab) are different types
- Even if they have the same number or timing, different types = different assignments

## 4. PARALLEL CLASS LOGIC
Parallel codes represent different sections/classes:
- Assignments are relevant to specific parallel classes
- If parallel codes overlap (k1 in both), it could be the same assignment
- If parallels are "all" or empty, it applies to everyone
- If there's NO overlap (k1,k2 vs k3), they're for different sections = not duplicate
- Exception: A re-announcement might ADD parallel codes to an existing assignment

## 5. DESCRIPTION AS SEMANTIC CONTEXT
The description provides crucial context about what the work actually entails:
- Use it to understand the actual requirements and deliverables
- Compare the substance of the work, not just keywords
- If descriptions discuss fundamentally different topics/requirements → different assignments
- If descriptions are semantically similar but differently worded → could be duplicate
- Missing descriptions shouldn't prevent duplicate detection if other signals are clear

## 6. CONFIDENCE CALIBRATION
Be precise about your certainty:
- HIGH confidence: Clear duplicate with strong signals (same course, same work identity, parallel overlap)
- MEDIUM confidence: Likely duplicate but some ambiguity exists
- LOW confidence: Significant uncertainty, multiple interpretations possible

Default to "not duplicate" when confidence is not HIGH. Creating a duplicate entry is safer than missing a new assignment.

---

# YOUR TASK

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
```json
{{
  "is_duplicate": boolean,
  "confidence": "high" | "medium" | "low",
  "reasoning": "Explain your analysis. ONLY IN A SINGLE SENTENCE.",
  "matched_assignment_id": "uuid of matched assignment or null"
}}
```

**Critical rules:**
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