// backend/src/parser/ai_extractor/context_builder.rs

use chrono::{FixedOffset, Utc};
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use once_cell::sync::Lazy;

use super::schedule_oracle::ScheduleOracle;
use super::parsing::{extract_groq_text, GroqResponse};
use super::GROQ_TEXT_MODELS;

// Compile regex once at startup for performance
static PARALLEL_CODE_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Match k1-k4, p1-p4, r1-r4 (case-insensitive, word boundaries)
    Regex::new(r"(?i)\b([kprs][1-4])\b").unwrap()
});

/// Extract ALL parallel codes from text (handles emojis, case-insensitive)
pub fn extract_parallel_codes_from_text(text: &str) -> Vec<String> {
    let mut codes = Vec::new();
    
    for cap in PARALLEL_CODE_REGEX.captures_iter(text) {
        if let Some(code_match) = cap.get(1) {
            let code = code_match.as_str().to_lowercase();
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
    }
    
    codes
}

/// Information about a quoted assignment from database
#[derive(Debug, Clone)]
pub struct QuotedAssignmentInfo {
    pub assignment_id: uuid::Uuid,
    pub course_name: String,
    pub title: String,
    pub parallel_codes: Vec<String>,
}

/// Minimal context needed for main AI prompt
#[derive(Debug, Clone)]
pub struct MessageContext {
    pub parallel_codes: Vec<String>,
    pub parallel_confidence: f32,
    pub parallel_source: String,
    pub deadline_hint: Option<String>,
    pub deadline_type: String,
    pub course_hints: Vec<CourseHint>,
    pub courses_list: String,
    pub quoted_message_summary: Option<String>,
    pub quoted_assignment_id: Option<uuid::Uuid>,
}

/// Per-course context hints with per-parallel schedule info
#[derive(Debug, Clone)]
pub struct CourseHint {
    pub course_name: String,
    pub parallel_codes: Vec<String>,
    pub deadline_type: String,
    pub parallel_schedules: Vec<ParallelSchedule>,
}

/// Individual parallel schedule information
#[derive(Debug, Clone)]
pub struct ParallelSchedule {
    pub parallel_code: String,
    pub next_meeting: Option<String>,  // Format: "YYYY-MM-DD HH:MM"
}

/// Build context by querying DB + lightweight AI
pub async fn build_context(
    message: &str,
    sender_id: &str,
    pool: &PgPool,
    schedule_oracle: &ScheduleOracle,
    quoted_message: Option<&str>,
    quoted_message_id: Option<&str>,
) -> Result<MessageContext, String> {
    
    let sender_history = get_sender_history(pool, sender_id).await
        .unwrap_or_default();
    
    let courses_list = get_courses_list(pool).await
        .unwrap_or_else(|_| "No courses available".to_string());
    
    // Look up quoted assignment by message_id (reliable!)
    let quoted_assignment = if let Some(msg_id) = quoted_message_id {
        lookup_assignment_by_message_id(pool, msg_id).await.ok()
    } else {
        None
    };
    
    let quoted_summary = quoted_message.map(|q| {
        if q.len() > 200 {
            format!("{}...", &q[..200])
        } else {
            q.to_string()
        }
    });
    
    // Extract parallel codes from message text directly (GRAFKOM K2 case)
    let text_extracted_parallels = extract_parallel_codes_from_text(message);
    
    let ai_hints = call_context_resolver_ai(
        message, 
        &sender_history, 
        &courses_list,
        quoted_summary.as_deref(),
        &text_extracted_parallels,
        quoted_assignment.as_ref(),
    ).await?;
    
    let course_hints = calculate_course_hints(
        &ai_hints,
        schedule_oracle,
    );
    
    let deadline_hint = if course_hints.len() == 1 {
        let hint = &course_hints[0];
        if hint.parallel_schedules.len() == 1 {
            hint.parallel_schedules[0].next_meeting.clone()
        } else {
            None
        }
    } else {
        None
    };
    
    let global_deadline_type = if course_hints.is_empty() {
        "unknown".to_string()
    } else if course_hints.len() == 1 {
        course_hints[0].deadline_type.clone()
    } else {
        let types: std::collections::HashSet<_> = course_hints
            .iter()
            .map(|h| h.deadline_type.as_str())
            .collect();
        if types.len() == 1 {
            course_hints[0].deadline_type.clone()
        } else {
            "mixed".to_string()
        }
    };
    
    let quoted_assignment_id = quoted_assignment.as_ref().map(|a| a.assignment_id);
    
    Ok(MessageContext {
        parallel_codes: ai_hints.parallel_codes,
        parallel_confidence: ai_hints.parallel_confidence,
        parallel_source: ai_hints.parallel_source,
        deadline_hint,
        deadline_type: global_deadline_type,
        course_hints,
        courses_list,
        quoted_message_summary: quoted_summary,
        quoted_assignment_id,
    })
}

// ===== QUOTED ASSIGNMENT LOOKUP =====

async fn lookup_assignment_by_message_id(
    pool: &PgPool,
    message_id: &str,
) -> Result<QuotedAssignmentInfo, sqlx::Error> {
    let record = sqlx::query!(
        r#"
        SELECT 
            a.id,
            a.title,
            a.parallel_codes,
            c.name as course_name
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE $1 = ANY(a.message_ids)
        "#,
        message_id
    )
    .fetch_one(pool)
    .await?;
    
    Ok(QuotedAssignmentInfo {
        assignment_id: record.id,
        course_name: record.course_name,
        title: record.title,
        parallel_codes: record.parallel_codes.unwrap_or_default(),
    })
}

// ===== COURSE LIST =====

async fn get_courses_list(pool: &PgPool) -> Result<String, sqlx::Error> {
    #[derive(Debug)]
    struct CourseRow {
        name: String,
        aliases: Option<Vec<String>>,
    }
    
    let courses = sqlx::query_as!(
        CourseRow,
        r#"
        SELECT name, aliases
        FROM courses
        ORDER BY name
        "#
    )
    .fetch_all(pool)
    .await?;
    
    let formatted = courses
        .iter()
        .map(|c| {
            if let Some(ref aliases) = c.aliases {
                if !aliases.is_empty() {
                    format!("{} [aka: {}]", c.name, aliases.join(", "))
                } else {
                    c.name.clone()
                }
            } else {
                c.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    Ok(formatted)
}

// ===== SENDER HISTORY =====

#[derive(Debug, Default)]
struct SenderHistory {
    parallel_patterns: Vec<(String, Vec<String>, i32)>,
}

async fn get_sender_history(pool: &PgPool, sender_id: &str) -> Result<SenderHistory, sqlx::Error> {
    let records = sqlx::query!(
        r#"
        SELECT c.name as course_name, a.parallel_codes, COUNT(*) as count
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.sender_id = $1 AND a.parallel_codes IS NOT NULL
        GROUP BY c.name, a.parallel_codes
        ORDER BY count DESC
        LIMIT 10
        "#,
        sender_id
    )
    .fetch_all(pool)
    .await?;
    
    let mut history = SenderHistory::default();
    
    for record in records {
        if let Some(parallel_codes) = record.parallel_codes {
            history.parallel_patterns.push((
                record.course_name,
                parallel_codes,
                record.count.unwrap_or(0) as i32,
            ));
        }
    }
    
    Ok(history)
}

// ===== LIGHTWEIGHT AI CALL =====

#[derive(Debug, Deserialize)]
struct AIHints {
    parallel_codes: Vec<String>,
    parallel_confidence: f32,
    parallel_source: String,
    course_hints: Vec<AICourseHint>,
}

#[derive(Debug, Deserialize)]
struct AICourseHint {
    course_name: String,
    parallel_codes: Vec<String>,
    deadline_type: String,
}

async fn call_context_resolver_ai(
    message: &str,
    sender_history: &SenderHistory,
    courses_list: &str,
    quoted_context: Option<&str>,
    text_extracted_parallels: &[String],
    quoted_assignment: Option<&QuotedAssignmentInfo>,
) -> Result<AIHints, String> {
    
    let history_text = if sender_history.parallel_patterns.is_empty() {
        "None".to_string()
    } else {
        sender_history.parallel_patterns
            .iter()
            .map(|(course, parallels, count)| {
                format!("{}: [{}] ({}x)", course, parallels.join(", "), count)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    
    // Build quoted section with DB info (reliable!)
    let quoted_section = if let Some(assignment) = quoted_assignment {
        format!(
            r#"

QUOTED ASSIGNMENT (from database):
  Course: {}
  Title: {}
  Current Parallels: [{}]
  → User is updating/referencing this assignment
  → YOU MUST extract ALL these parallel codes for schedule lookup"#,
            assignment.course_name,
            assignment.title,
            assignment.parallel_codes.join(", ")
        )
    } else if let Some(ctx) = quoted_context {
        format!("\n\nQUOTED MESSAGE CONTEXT:\n{}\n(User is replying to/referencing this message)", ctx)
    } else {
        String::new()
    };
    
    let parallel_hint = if !text_extracted_parallels.is_empty() {
        format!("\n\nEXTRACTED PARALLEL CODES FROM MESSAGE: [{}]", text_extracted_parallels.join(", "))
    } else {
        String::new()
    };
    
    let prompt = format!(
        r#"Extract structured course information from an academic message.

MESSAGE: "{}"
SENDER HISTORY: {}{}{}

AVAILABLE COURSES:
{}

=== CRITICAL INSTRUCTION ===
If QUOTED ASSIGNMENT data is provided above, YOU MUST extract ALL its parallel codes.
This is NOT optional. Schedule lookup requires complete parallel information.
Example: If quoted shows [k1, k2, r3], extract ALL THREE, not just one.

=== TASK DEFINITION ===
Identify courses and classify deadline information as structured JSON.

=== ENTITY EXTRACTION GUIDELINES ===

IMPORTANT: This message may reference ONE OR MORE assignments. Each assignment has:
- Its own course
- Its own set of parallel codes  
- Its own deadline type

Your job is to identify EACH DISTINCT ASSIGNMENT and extract their information SEPARATELY.

1. ASSIGNMENT IDENTIFICATION
   • If QUOTED ASSIGNMENT exists, user is likely updating/referencing that ONE assignment
   • Otherwise, check if message describes multiple assignments or just one
   • Each assignment entry should have ONE course

2. COURSE IDENTIFICATION (per assignment)
   • Match against AVAILABLE COURSES list only
   • Use full course name, never alias (check [aka: ...] for aliases)
   • Assignment/project titles are NOT courses
   • If QUOTED ASSIGNMENT present, use that course name EXACTLY

3. PARALLEL CLASS CODES (per assignment) - CRITICAL
   • Definition: Valid codes are k1-k4, p1-p4, r1-r4, or "all"
   • Format: Return as array, can contain multiple codes
   • THESE ARE PER-ASSIGNMENT, NOT PER-COURSE
   
   • MANDATORY PRIORITY ORDER:
     1. **QUOTED ASSIGNMENT parallels** → USE ALL OF THEM (highest priority)
        - If quoted assignment has multiple codes, extract EVERY SINGLE ONE
        - Do NOT drop any codes from quoted assignment
        - These are from database, 100% reliable
        - Example: Quoted has [k1, k2, k3] → YOU MUST RETURN [k1, k2, k3]
        - Example: Quoted has [r1, r2] → YOU MUST RETURN [r1, r2]
        - Example: Quoted has [k2, p1, p4] → YOU MUST RETURN [k2, p1, p4]
     
     2. **EXTRACTED PARALLEL CODES** (from message regex)
        - If message explicitly mentions parallels, use those
     
     3. **SENDER HISTORY** (user's past pattern for this course)
        - Fallback if no quoted assignment or explicit mention
     
     4. **Empty array** if no information available
   
   • Recognition patterns:
     - Course abbreviations: "GRAFKOM K2" → ["k2"], "METCUAN R1" → ["r1"]
     - Explicit lists: "untuk k1, k2" → ["k1", "k2"]
     - Keywords "semua kelas"/"all classes" → ["all"]
   
   • CRITICAL: When QUOTED ASSIGNMENT exists, extract its FULL parallel list
     DO NOT extract only the first one or a subset. Extract ALL codes.

4. DEADLINE TYPE CLASSIFICATION (per assignment)
   • "explicit": Absolute calendar dates
     - Contains: specific date-month, day of week + date, "tanggal [number]"
     - Examples: "5 Januari", "Jumat 10 Januari", "tanggal 15"
   
   • "next_meeting": Class session references WITHOUT relative time terms
     - Contains: "sebelum pertemuan", "before class", "di awal kelas", "saat kuliah"
     - "ketika praktikum", "waktu kelas", "during class", "pertemuan berikutnya"
     - "di class", "saat pertemuan", "waktu lab"
     - Must NOT contain relative terms (besok, minggu depan, etc.)
   
   • "relative": Relative temporal references
     - Contains: "besok", "lusa", "minggu depan", "nanti", "hari ini", "tomorrow"
     - Note: "besok sebelum kelas" is RELATIVE (besok = relative term)
   
   • "unknown": Course mentioned without deadline info

4. GLOBAL PARALLEL CODES
   • Return parallels common to ALL courses
   • If courses have different parallels → empty array
   • Single course scenario → empty array (not global)

=== OUTPUT FORMAT ===
Return JSON only:
{{
  "parallel_codes": [string],  // Global parallels or []
  "parallel_confidence": float,  // 0.0-1.0
  "parallel_source": "quoted_assignment" | "explicit" | "sender_history" | "unknown",
  "course_hints": [
    {{
      "course_name": string,  // Full name from AVAILABLE COURSES
      "parallel_codes": [string],  // THIS ASSIGNMENT's parallels (not course-wide!)
      "deadline_type": "explicit" | "next_meeting" | "relative" | "unknown"
    }}
  ]
}}

FINAL REMINDER: 
- Parallel codes are PER-ASSIGNMENT, not per-course
- If QUOTED ASSIGNMENT exists, its parallel codes are MANDATORY
- Extract ALL parallel codes from quoted assignment, not a subset
- This is required for schedule lookup to work correctly for each parallel class"#,
        message,
        history_text,
        quoted_section,
        parallel_hint,
        courses_list
    );
    
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    for model in GROQ_TEXT_MODELS {
        match call_groq_api(&api_key, model, &prompt).await {
            Ok(json_text) => {
                return parse_ai_hints(&json_text);
            }
            Err(e) => {
                eprintln!("Context AI failed with {}: {}", model, e);
                continue;
            }
        }
    }
    
    Err("All context resolver models failed".to_string())
}

async fn call_groq_api(api_key: &str, model: &str, prompt: &str) -> Result<String, String> {
    let url = "https://api.groq.com/openai/v1/chat/completions";
    
    let request_body = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.1,
        "max_tokens": 1000,
        "response_format": {"type": "json_object"}
    });
    
    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    let status = response.status();
    
    if !status.is_success() {
        let error_text = response.text().await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("API error: {} - {}", status, error_text));
    }
    
    let groq_response: GroqResponse = response.json().await
        .map_err(|e| format!("Parse error: {}", e))?;
    
    extract_groq_text(&groq_response)
}

fn parse_ai_hints(json_text: &str) -> Result<AIHints, String> {
    serde_json::from_str(json_text)
        .map_err(|e| format!("Failed to parse AI hints: {}", e))
}

// ===== DEADLINE CALCULATION WITH PER-PARALLEL SCHEDULES =====

fn calculate_course_hints(
    hints: &AIHints,
    schedule_oracle: &ScheduleOracle,
) -> Vec<CourseHint> {
    let mut course_hints = Vec::new();
    
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let today = now.date_naive();
    
    for ai_course_hint in &hints.course_hints {
        println!("│");
        println!("│ 🎯 Processing: {}", ai_course_hint.course_name);
        println!("│    Parallels: {:?}", ai_course_hint.parallel_codes);
        println!("│    Deadline Type: {}", ai_course_hint.deadline_type);
        
        let parallel_schedules = match ai_course_hint.deadline_type.as_str() {
            "next_meeting" | "relative" => {
                // Both types benefit from schedule hints
                // "next_meeting" = deadline is literally the next meeting time
                // "relative" = deadline is relative (besok, minggu depan) - provide next meeting as reference
                
                if ai_course_hint.parallel_codes.is_empty() {
                    println!("│    ⏭️  Result: Skipped (needs parallel for schedule)");
                    vec![]
                } else if ai_course_hint.parallel_codes.contains(&"all".to_string()) {
                    println!("│    ⏭️  Result: Skipped ('all' cannot determine specific schedule)");
                    vec![]
                } else {
                    // Get immediate next meeting for EACH parallel
                    let mut schedules = Vec::new();
                    
                    let hint_type = if ai_course_hint.deadline_type == "next_meeting" {
                        "Next meeting"
                    } else {
                        "Schedule reference"
                    };
                    
                    for parallel in &ai_course_hint.parallel_codes {
                        if let Some((meeting_date, meeting_time)) = schedule_oracle
                            .get_next_meeting_with_time(&ai_course_hint.course_name, parallel, today)
                        {
                            let next_meeting = format!("{} {}", meeting_date, meeting_time);
                            println!("│    ✅ {}: {} at {}", 
                                parallel.to_uppercase(), hint_type, next_meeting);
                            
                            schedules.push(ParallelSchedule {
                                parallel_code: parallel.clone(),
                                next_meeting: Some(next_meeting),
                            });
                        } else {
                            println!("│    ⏭️  {}: No schedule found", parallel.to_uppercase());
                            
                            schedules.push(ParallelSchedule {
                                parallel_code: parallel.clone(),
                                next_meeting: None,
                            });
                        }
                    }
                    
                    schedules
                }
            },
            "explicit" => {
                println!("│    📅 Result: Explicit date (main AI will parse)");
                vec![]
            },
            _ => {
                println!("│    ❓ Result: Unknown type");
                vec![]
            }
        };
        
        course_hints.push(CourseHint {
            course_name: ai_course_hint.course_name.clone(),
            parallel_codes: ai_course_hint.parallel_codes.clone(),
            deadline_type: ai_course_hint.deadline_type.clone(),
            parallel_schedules,
        });
    }
    
    course_hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_code_extraction() {
        // Basic cases
        assert_eq!(extract_parallel_codes_from_text("K1"), vec!["k1"]);
        assert_eq!(extract_parallel_codes_from_text("p2"), vec!["p2"]);
        assert_eq!(extract_parallel_codes_from_text("R3"), vec!["r3"]);
        
        // With emojis
        assert_eq!(
            extract_parallel_codes_from_text("🚨 GRAFKOM K2 🚨"),
            vec!["k2"]
        );
        
        // Course abbreviations
        assert_eq!(extract_parallel_codes_from_text("METCUAN K3"), vec!["k3"]);
        assert_eq!(extract_parallel_codes_from_text("Pemrograman P1"), vec!["p1"]);
        assert_eq!(extract_parallel_codes_from_text("Statistika R2"), vec!["r2"]);
        
        // Multiple codes
        assert_eq!(
            extract_parallel_codes_from_text("K1 dan K2"),
            vec!["k1", "k2"]
        );
        
        assert_eq!(
            extract_parallel_codes_from_text("R1, R2, K3"),
            vec!["r1", "r2", "k3"]
        );
        
        // No code
        assert_eq!(extract_parallel_codes_from_text("No code here").len(), 0);
    }
}