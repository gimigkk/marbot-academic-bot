// backend/src/parser/ai_extractor/context_builder.rs

use chrono::{FixedOffset, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use super::schedule_oracle::ScheduleOracle;
use super::parsing::{extract_groq_text, GroqResponse};
use super::GROQ_TEXT_MODELS;

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
) -> Result<MessageContext, String> {
    
    let sender_history = get_sender_history(pool, sender_id).await
        .unwrap_or_default();
    
    let courses_list = get_courses_list(pool).await
        .unwrap_or_else(|_| "No courses available".to_string());
    
    let quoted_summary = if let Some(quoted) = quoted_message {
        extract_quoted_context(quoted, pool).await
            .ok()
    } else {
        None
    };
    
    let ai_hints = call_context_resolver_ai(
        message, 
        &sender_history, 
        &courses_list,
        quoted_summary.as_deref(),
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
    
    Ok(MessageContext {
        parallel_codes: ai_hints.parallel_codes,
        parallel_confidence: ai_hints.parallel_confidence,
        parallel_source: ai_hints.parallel_source,
        deadline_hint,
        deadline_type: global_deadline_type,
        course_hints,
        courses_list,
        quoted_message_summary: quoted_summary,
    })
}

// ===== QUOTED MESSAGE CONTEXT =====

async fn extract_quoted_context(
    quoted_text: &str,
    _pool: &PgPool,
) -> Result<String, String> {
    let truncated = if quoted_text.len() > 200 {
        format!("{}...", &quoted_text[..200])
    } else {
        quoted_text.to_string()
    };
    
    Ok(truncated)
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
    
    let quoted_section = quoted_context
        .map(|ctx| format!("\n\nQUOTED MESSAGE CONTEXT:\n{}\n(User is replying to/referencing this message)", ctx))
        .unwrap_or_default();
    
    let prompt = format!(
        r#"Analyze this academic message and extract structured course information.

MESSAGE: "{}"
SENDER HISTORY: {}{}

AVAILABLE COURSES:
{}

TASK: Identify courses mentioned and classify deadline information.

COURSE IDENTIFICATION:
• Match against AVAILABLE COURSES list (check both full names and aliases in [aka: ...])
• Always use the FULL course name, not the alias
• Assignment titles and project names are NOT courses
• If QUOTED MESSAGE CONTEXT is present, use it to identify which assignment is being referenced
• Return empty array if no valid courses identified

PARALLEL CLASSES (per course):
• Valid values: k1 - k4, p1 - p4, r1 - r4, all
• Return as ARRAY (can be multiple): ["k1", "k2"] or ["all"]
• Priority: explicit mention > quoted context > sender history > empty array
• Each course independent (don't assume shared parallel)
• Examples:
  - "Tugas PBO untuk k1 dan k2" → ["k1", "k2"]
  - "Semua kelas" → ["all"]
  - "Kelas k1, k2, k3" → ["k1", "k2", "k3"]
  - No mention → []

DEADLINE TYPE (per course):
• "explicit": ABSOLUTE date references (calendar dates, specific date-month combinations)
  Examples: "5 Januari", "Jumat 10 Januari", "10 January 2026", "tanggal 15"
  
• "next_meeting": References class session timing
  Examples: "sebelum pertemuan", "before class", "di awal kelas", "saat kuliah", "before next session"
  
• "relative": RELATIVE temporal references (relative to current date/time)
  Examples: "besok", "lusa", "minggu depan", "nanti", "hari ini"
  Note: STILL relative even with specific times ("besok jam 10" = relative + time)
  
• "unknown": Course mentioned without any deadline information

Key distinction:
- Absolute date (5 Jan, Friday 10th) → explicit
- Relative term (tomorrow, next week) → relative (even with time: "tomorrow 10am")
- Class-based reference (before class) → next_meeting

GLOBAL PARALLEL:
• Return array of parallels that apply to ALL courses
• If courses have different parallels, return empty array
• Examples:
  - All courses mention k1 → ["k1"]
  - Course A has k1, Course B has k2 → []
  - All courses mention "all" → ["all"]

USING QUOTED CONTEXT:
• If message says "diundur" / "berubah" / "updated" and quotes a previous assignment, extract info from quoted context
• Treat quoted assignment info as the reference point for updates

Return JSON:
{{
  "parallel_codes": [string],
  "parallel_confidence": float,
  "parallel_source": "explicit" | "quoted_context" | "sender_history" | "unknown",
  "course_hints": [
    {{
      "course_name": string,
      "parallel_codes": [string],
      "deadline_type": string
    }}
  ]
}}"#,
        message,
        history_text,
        quoted_section,
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