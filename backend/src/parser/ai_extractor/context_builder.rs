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
    pub deadline_type: String,
    pub course_hints: Vec<CourseHint>,
}

/// Per-course context hints
#[derive(Debug, Clone)]
pub struct CourseHint {
    pub course_name: String,
    pub parallel_code: Option<String>,
    pub deadline_hint: Option<String>,
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
    
    Ok(MessageContext {
        parallel_code: ai_hints.parallel_code,
        parallel_confidence: ai_hints.parallel_confidence,
        parallel_source: ai_hints.parallel_source,
        deadline_hint,
        deadline_type: ai_hints.deadline_type,
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
    deadline_type: String,
    course_hints: Vec<AICourseHint>,
}

#[derive(Debug, Deserialize)]
struct AICourseHint {
    course_name: String,
    parallel_code: Option<String>,
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
   - Example: "PEMROG K2, KALKULUS K2, FISIKA K2" → parallel_code="k2" ✓
   - Example: "PEMROG K2, KALKULUS TUGAS" → parallel_code=null ✗ (Kalkulus not specified)
   - Example: "STRUKDAT K2, ORKOM KUIS" → parallel_code=null ✗ (ORKOM not specified)
   - If even ONE course lacks explicit parallel → null

2. parallel_confidence: 0.0-1.0
   - 1.0: ALL courses explicitly mention same parallel
   - 0.85-0.9: Single course with explicit parallel or strong history
   - 0.0: Multiple courses where not all have explicit parallels

3. parallel_source: "explicit"|"sender_history"|"unknown"

4. deadline_type: "explicit"|"next_meeting"|"relative"|"unknown"
   - explicit: contains YYYY-MM-DD or specific date
   - next_meeting: "sebelum pertemuan", "before class", "before next meeting"
   - relative: "besok", "lusa", "minggu depan"
   - unknown: no deadline mentioned

5. course_hints: Array of courses - CRITICAL FIELD
   - Extract EVERY course from message
   - For EACH course, determine parallel INDEPENDENTLY:
     
     Step 1: Check if parallel explicitly mentioned WITH that specific course
       "STRUKDAT K2 TUGAS" → Strukdat gets k2
       "ORKOM KUIS" → ORKOM gets null (NO parallel mentioned)
     
     Step 2: If not explicit, check sender history for THAT SPECIFIC course
       If "Struktur Data: k1 (3x)" in history → use k1
       If no history → null
     
     Step 3: NEVER copy parallel from another course
       Even if "STRUKDAT K2, ORKOM KUIS" are in same message
       → Strukdat=k2, ORKOM=null (NOT k2!)
   
   Examples:
   
   1. "STRUKDAT K2 TUGAS, ORKOM KUIS"
      → {{"parallel_code":null,"course_hints":[
           {{"course_name":"Struktur Data","parallel_code":"k2"}},
           {{"course_name":"Organisasi dan Arsitektur Komputer","parallel_code":null}}
         ]}}
      Reason: Only STRUKDAT has explicit K2, ORKOM has NOTHING
   
   2. "PEMROG K1, KALKULUS K1, FISIKA K1"
      → {{"parallel_code":"k1","course_hints":[
           {{"course_name":"Pemrograman","parallel_code":"k1"}},
           {{"course_name":"Kalkulus","parallel_code":"k1"}},
           {{"course_name":"Fisika","parallel_code":"k1"}}
         ]}}
      Reason: ALL three explicitly K1
   
   3. "PEMROG TUGAS" + history: "Pemrograman: k2 (5x)"
      → {{"parallel_code":"k2","course_hints":[
           {{"course_name":"Pemrograman","parallel_code":"k2"}}
         ]}}
      Reason: Single course, use history

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
    
    println!("🕐 Current time (GMT+7): {}", now.format("%Y-%m-%d %H:%M:%S"));
    println!("📅 Today's date: {}", today);
    println!("🔍 Deadline type: {}", hints.deadline_type);
    
    for ai_course_hint in &hints.course_hints {
        let deadline_hint = match hints.deadline_type.as_str() {
            "next_meeting" => {
                println!("🎯 Calculating next meeting for: {}", ai_course_hint.course_name);
                
                // Check if we have a valid parallel code
                let has_valid_parallel = ai_course_hint.parallel_code
                    .as_ref()
                    .map(|p| p != "all" && p != "null" && !p.is_empty())
                    .unwrap_or(false);
                
                if !has_valid_parallel {
                    println!("  ⚠️ No valid parallel code - will need clarification");
                    println!("  ⏭️ Skipping deadline hint (parallel needed for schedule lookup)");
                    None
                } else {
                    let parallel = ai_course_hint.parallel_code.as_ref().unwrap();
                    
                    if let Some((meeting_date, meeting_time)) = schedule_oracle
                        .get_next_meeting_with_time(&ai_course_hint.course_name, parallel, today)
                    {
                        let hint = format!("{} {}", meeting_date, meeting_time);
                        println!("  ✅ Found next meeting: {} (parallel: {})", hint, parallel);
                        Some(hint)
                    } else {
                        println!("  ⚠️ No schedule found for {} with parallel {}", 
                            ai_course_hint.course_name, parallel);
                        println!("  ⏭️ Will need clarification for deadline");
                        None
                    }
                }
            },
            "relative" => {
                println!("📆 Calculating relative deadline for: {}", ai_course_hint.course_name);
                let hint = format!("{} 23:59", today + Duration::days(1));
                println!("  ℹ️ Relative hint (tomorrow EOD): {}", hint);
                Some(hint)
            },
            "explicit" => {
                println!("📅 Explicit deadline in message - main AI will parse it");
                None
            },
            _ => {
                println!("  ⏭️ Skipping hint calculation (type: {})", hints.deadline_type);
                None
            }
        };
        
        course_hints.push(CourseHint {
            course_name: ai_course_hint.course_name.clone(),
            parallel_code: ai_course_hint.parallel_code.clone(),
            deadline_hint,
        });
    }
    
    println!("✅ Generated {} course hints\n", course_hints.len());
    course_hints
}