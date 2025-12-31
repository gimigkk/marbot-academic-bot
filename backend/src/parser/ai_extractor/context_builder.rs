// backend/src/parser/ai_extractor/context_builder.rs

use chrono::{Duration, FixedOffset, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use super::schedule_oracle::ScheduleOracle;
use super::parsing::{extract_groq_text, GroqResponse};

/// Minimal context needed for main AI prompt
#[derive(Debug, Clone)]
pub struct MessageContext {
    pub parallel_code: Option<String>,
    pub parallel_confidence: f32,
    pub parallel_source: String,
    pub deadline_hint: Option<String>,
    pub deadline_type: String, // Keep for backward compatibility
    pub course_hints: Vec<CourseHint>,
}

/// Per-course context hints
#[derive(Debug, Clone)]
pub struct CourseHint {
    pub course_name: String,
    pub parallel_code: Option<String>,
    pub deadline_hint: Option<String>,
    pub deadline_type: String, // NEW: per-course deadline type
}

/// Build context by querying DB + lightweight AI
pub async fn build_context(
    message: &str,
    sender_id: &str,
    pool: &PgPool,
    schedule_oracle: &ScheduleOracle,
) -> Result<MessageContext, String> {
    
    // Step 1: Query sender history (fast, 0.01s)
    let sender_history = get_sender_history(pool, sender_id).await
        .unwrap_or_default();
    
    // Step 2: Quick AI call to resolve ambiguities (0.8s)
    let ai_hints = call_context_resolver_ai(message, &sender_history).await?;
    
    // Step 3: Calculate deadline hints for each course (instant)
    let course_hints = calculate_course_hints(
        &ai_hints,
        schedule_oracle,
    );
    
    // Step 4: Determine global deadline hint (for backward compatibility)
    let deadline_hint = if course_hints.len() == 1 {
        course_hints.first().and_then(|h| h.deadline_hint.clone())
    } else {
        None
    };
    
    // Global deadline_type is now just for backward compatibility
    // Use the first course's type, or "unknown" if multiple different types
    let global_deadline_type = if course_hints.len() == 1 {
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
    })
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
    deadline_type: String, // Global fallback
    course_hints: Vec<AICourseHint>,
}

#[derive(Debug, Deserialize)]
struct AICourseHint {
    course_name: String,
    parallel_code: Option<String>,
    deadline_type: String, // NEW: per-course deadline type
}

async fn call_context_resolver_ai(
    message: &str,
    sender_history: &SenderHistory,
) -> Result<AIHints, String> {
    
    let history_text = if sender_history.parallel_patterns.is_empty() {
        "No history".to_string()
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
        r#"Quick analysis of this message.

MESSAGE: "{}"

SENDER HISTORY: {}

ABSOLUTE RULES (NEVER VIOLATE):
1. NEVER assume a parallel applies to multiple courses unless EXPLICITLY stated
2. Each course MUST have its parallel explicitly mentioned OR be in sender history
3. "PEMROG K2, GKV KUIS" → ONLY Pemrog gets K2, GKV gets null (NOT K2!)
4. "STRUKDAT K2, ORKOM KUIS" → ONLY Strukdat gets K2, ORKOM gets null (NOT K2!)
5. Do NOT infer parallels from nearby courses - treat each independently

TASK: Answer these questions in JSON:

1. parallel_code: Global parallel (k1/k2/k3/p1/p2/p3/r1/r2/r3/null)
   - ONLY set if EVERY SINGLE course explicitly mentions the SAME parallel
   - If even ONE course lacks explicit parallel → null

2. parallel_confidence: 0.0-1.0

3. parallel_source: "explicit"|"sender_history"|"unknown"

4. deadline_type: Global fallback (use if all courses have same type)
   - "explicit"|"next_meeting"|"relative"|"unknown"

5. course_hints: Array of courses with PER-COURSE deadline types
   
   For EACH course, determine:
   
   a) parallel_code: Same rules as before (explicit > history > null)
   
   b) deadline_type: **IMPORTANT - ANALYZE PER COURSE**
      - explicit: Course mentions YYYY-MM-DD or specific date
        Examples: "STRUKDAT TUGAS deadline 5 Januari"
      
      - next_meeting: Course mentions "sebelum pertemuan", "before class", "before next meeting"
        Examples: "STRUKDAT sebelum pertemuan berikutnya"
      
      - relative: Course mentions "besok", "lusa", "minggu depan", "tomorrow"
        Examples: "ORKOM KUIS deadline Lusa"
      
      - unknown: No deadline mentioned for this specific course
   
   CRITICAL: Each course gets its OWN deadline_type based on what's said about THAT course
   
   Examples:
   
   1. "STRUKDAT K2 TUGAS sebelum pertemuan, ORKOM KUIS deadline Lusa"
      → course_hints: [
           {{"course_name":"Struktur Data","parallel_code":"k2","deadline_type":"next_meeting"}},
           {{"course_name":"Organisasi dan Arsitektur Komputer","parallel_code":null,"deadline_type":"relative"}}
         ]
      Reason: STRUKDAT has "sebelum pertemuan" → next_meeting
              ORKOM has "deadline Lusa" → relative
   
   2. "PEMROG K1 TUGAS besok, KALKULUS K1 TUGAS besok"
      → course_hints: [
           {{"course_name":"Pemrograman","parallel_code":"k1","deadline_type":"relative"}},
           {{"course_name":"Kalkulus","parallel_code":"k1","deadline_type":"relative"}}
         ]
      Reason: Both mention "besok" → relative
   
   3. "STRUKDAT TUGAS 15, ORKOM QUIZ 3 sebelum pertemuan"
      → course_hints: [
           {{"course_name":"Struktur Data","parallel_code":null,"deadline_type":"unknown"}},
           {{"course_name":"Organisasi dan Arsitektur Komputer","parallel_code":null,"deadline_type":"next_meeting"}}
         ]
      Reason: STRUKDAT has no deadline mention → unknown
              ORKOM mentions "sebelum pertemuan" → next_meeting

OUTPUT: JSON only, no markdown."#,
        message,
        history_text
    );
    
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    let models = &[
        "llama-3.3-70b-versatile",
        "llama-3.1-8b-instant",
    ];
    
    for model in models {
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
        "temperature": 0.2,
        "max_tokens": 800,
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
        // Use PER-COURSE deadline type instead of global
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
            deadline_type: ai_course_hint.deadline_type.clone(), // Store per-course type
        });
    }
    
    //println!("│\n│ ✅ Generated {} course hints\n│", course_hints.len());
    course_hints
}