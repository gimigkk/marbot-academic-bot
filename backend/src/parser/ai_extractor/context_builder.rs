// backend/src/parser/ai_extractor/context_builder.rs

use chrono::{Duration, FixedOffset, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use super::schedule_oracle::ScheduleOracle;
use super::parsing::{extract_groq_text, GroqResponse};
use super::GROQ_TEXT_MODELS;

/// Minimal context needed for main AI prompt
#[derive(Debug, Clone)]
pub struct MessageContext {
    pub parallel_code: Option<String>,
    pub parallel_confidence: f32,
    pub parallel_source: String,
    pub deadline_hint: Option<String>,
    pub deadline_type: String,
    pub course_hints: Vec<CourseHint>,
    pub courses_list: String,  // ✅ NEW: Full course list with aliases
}

/// Per-course context hints
#[derive(Debug, Clone)]
pub struct CourseHint {
    pub course_name: String,
    pub parallel_code: Option<String>,
    pub deadline_hint: Option<String>,
    pub deadline_type: String,
}

/// Build context by querying DB + lightweight AI
pub async fn build_context(
    message: &str,
    sender_id: &str,
    pool: &PgPool,
    schedule_oracle: &ScheduleOracle,
) -> Result<MessageContext, String> {
    
    let sender_history = get_sender_history(pool, sender_id).await
        .unwrap_or_default();
    
    // ✅ NEW: Get course list with aliases
    let courses_list = get_courses_list(pool).await
        .unwrap_or_else(|_| "No courses available".to_string());
    
    let ai_hints = call_context_resolver_ai(message, &sender_history, &courses_list).await?;
    
    let course_hints = calculate_course_hints(
        &ai_hints,
        schedule_oracle,
    );
    
    let deadline_hint = if course_hints.len() == 1 {
        course_hints.first().and_then(|h| h.deadline_hint.clone())
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
        parallel_code: ai_hints.parallel_code,
        parallel_confidence: ai_hints.parallel_confidence,
        parallel_source: ai_hints.parallel_source,
        deadline_hint,
        deadline_type: global_deadline_type,
        course_hints,
        courses_list,  // ✅ NEW: Include in context
    })
}

// ===== COURSE LIST =====

/// Get formatted course list with aliases
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
    parallel_patterns: Vec<(String, String, i32)>,
}

async fn get_sender_history(pool: &PgPool, sender_id: &str) -> Result<SenderHistory, sqlx::Error> {
    let records = sqlx::query!(
        r#"
        SELECT c.name as course_name, a.parallel_code, COUNT(*) as count
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.sender_id = $1 AND a.parallel_code IS NOT NULL
        GROUP BY c.name, a.parallel_code
        ORDER BY count DESC
        LIMIT 10
        "#,
        sender_id
    )
    .fetch_all(pool)
    .await?;
    
    let mut history = SenderHistory::default();
    
    for record in records {
        if let Some(parallel) = record.parallel_code {
            history.parallel_patterns.push((
                record.course_name,
                parallel,
                record.count.unwrap_or(0) as i32,
            ));
        }
    }
    
    Ok(history)
}

// ===== LIGHTWEIGHT AI CALL =====

#[derive(Debug, Deserialize)]
struct AIHints {
    parallel_code: Option<String>,
    parallel_confidence: f32,
    parallel_source: String,
    course_hints: Vec<AICourseHint>,
}

#[derive(Debug, Deserialize)]
struct AICourseHint {
    course_name: String,
    parallel_code: Option<String>,
    deadline_type: String,
}

async fn call_context_resolver_ai(
    message: &str,
    sender_history: &SenderHistory,
    courses_list: &str,  // ✅ NEW parameter
) -> Result<AIHints, String> {
    
    let history_text = if sender_history.parallel_patterns.is_empty() {
        "None".to_string()
    } else {
        sender_history.parallel_patterns
            .iter()
            .map(|(course, parallel, count)| {
                format!("{}: {} ({}x)", course, parallel, count)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    
    let prompt = format!(
        r#"Analyze this academic message and extract structured course information.

MESSAGE: "{}"
SENDER HISTORY: {}

AVAILABLE COURSES:
{}

TASK: Identify courses mentioned and classify deadline information.

COURSE IDENTIFICATION:
• Match against AVAILABLE COURSES list (check both full names and aliases in [aka: ...])
• Always use the FULL course name, not the alias
• Assignment titles and project names are NOT courses
• Return empty array if no valid courses identified

PARALLEL CLASS (per course):
• Valid values: k1, k2, k3, p1, p2, p3, r1, r2, r3, or null
• Priority: explicit mention > sender history > null
• Each course independent (don't assume shared parallel)

DEADLINE TYPE (per course):
• "explicit": Specific date (2026-01-15, "5 Januari", "15 Desember")
• "next_meeting": References next class ("sebelum pertemuan", "before class")
• "relative": Relative time ("besok", "tomorrow", "minggu depan")
• "unknown": Course mentioned without deadline

GLOBAL PARALLEL:
• Set only if ALL courses share identical parallel
• Otherwise null

Return JSON:
{{
  "parallel_code": string | null,
  "parallel_confidence": float,
  "parallel_source": "explicit" | "sender_history" | "unknown",
  "course_hints": [
    {{
      "course_name": string,
      "parallel_code": string | null,
      "deadline_type": string
    }}
  ]
}}"#,
        message,
        history_text,
        courses_list  // ✅ Include course list in prompt
    );
    
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    // ✅ Use configured text models from mod.rs
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
        "temperature": 0.1,  // ✅ Lower temperature for consistency
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

// ===== DEADLINE CALCULATION (PER-COURSE) =====

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
        println!("│    Parallel: {:?}", ai_course_hint.parallel_code);
        println!("│    Deadline Type: {}", ai_course_hint.deadline_type);
        
        let deadline_hint = match ai_course_hint.deadline_type.as_str() {
            "next_meeting" => {
                let has_valid_parallel = ai_course_hint.parallel_code
                    .as_ref()
                    .map(|p| p != "all" && p != "null" && !p.is_empty())
                    .unwrap_or(false);
                
                if !has_valid_parallel {
                    println!("│    ⏭️  Result: Skipped (needs parallel for schedule)");
                    None
                } else {
                    let parallel = ai_course_hint.parallel_code.as_ref().unwrap();
                    
                    if let Some((meeting_date, meeting_time)) = schedule_oracle
                        .get_next_meeting_with_time(&ai_course_hint.course_name, parallel, today)
                    {
                        let hint = format!("{} {}", meeting_date, meeting_time);
                        println!("│    ✅ Result: Next meeting at {}", hint);
                        Some(hint)
                    } else {
                        println!("│    ⏭️  Result: No schedule found");
                        None
                    }
                }
            },
            "relative" => {
                let hint = format!("{} 23:59", today + Duration::days(1));
                println!("│    ✅ Result: Tomorrow EOD ({})", hint);
                Some(hint)
            },
            "explicit" => {
                println!("│    📅 Result: Explicit date (main AI will parse)");
                None
            },
            _ => {
                println!("│    ❓ Result: Unknown type (no hint generated)");
                None
            }
        };
        
        course_hints.push(CourseHint {
            course_name: ai_course_hint.course_name.clone(),
            parallel_code: ai_course_hint.parallel_code.clone(),
            deadline_hint,
            deadline_type: ai_course_hint.deadline_type.clone(),
        });
    }
    
    course_hints
}