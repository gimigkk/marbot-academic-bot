// backend/src/parser/ai_extractor/context_builder.rs

use chrono::{DateTime, FixedOffset, NaiveDateTime, Utc};
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use once_cell::sync::Lazy;

use super::schedule_oracle::ScheduleOracle;
use super::parsing::{GeminiResponse, GroqResponse, extract_ai_text, extract_groq_text};
use super::{GEMINI_MODELS, GROQ_REASONING_MODELS, GROQ_TEXT_MODELS};
use crate::tui::JobLogger;

// ===== CONSTANTS & STATICS =====

static PARALLEL_CODE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b([kprs][1-4])\b").unwrap()
});

const MAX_HISTORY_PATTERNS: usize = 3;

// ===== PUBLIC TYPES =====

#[derive(Debug, Clone)]
pub struct QuotedAssignmentInfo {
    pub assignment_id: uuid::Uuid,
    pub course_name: String,
    pub title: String,
    pub parallel_codes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MessageContext {
    //pub parallel_codes: Vec<String>,   remove all global parallel references
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

// ===== PRIVATE TYPES =====

/// Sender's historical assignment patterns with temporal weighting
#[derive(Debug, Default)]
struct SenderHistory {
    parallel_patterns: Vec<ParallelPattern>,
}

/// Individual pattern with relevance scoring
#[derive(Debug, Clone)]
struct ParallelPattern {
    course_name: String,
    parallel_codes: Vec<String>,
    count: i32,
    last_used: NaiveDateTime,
    relevance_score: f32,
}

// ===== PUBLIC API =====

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

// BUILD CONTENT LOGIC
pub async fn build_context(
    message: &str,
    sender_id: &str,
    pool: &PgPool,
    schedule_oracle: &ScheduleOracle,
    quoted_message: Option<&str>,
    quoted_message_id: Option<&str>,
    logger: &JobLogger,
) -> Result<MessageContext, String> {
    
    // TIER 1: Extract parallels from message directly (HIGHEST PRIORITY)
    let text_extracted_parallels = extract_parallel_codes_from_text(message);
    
    // TIER 2: Look up quoted assignment by message_id (HIGH PRIORITY)
    let quoted_assignment = if let Some(msg_id) = quoted_message_id {
        lookup_assignment_by_message_id(pool, msg_id).await.ok()
    } else {
        None
    };
    
    // TIER 3: Sender history (FALLBACK ONLY - conditional loading)
    let sender_history = if should_load_sender_history(
        &text_extracted_parallels,
        &quoted_assignment
    ) {
        get_sender_history(pool, sender_id, &text_extracted_parallels).await
            .unwrap_or_default()
    } else {
        logger.log("│ ⏭️ Skipping sender history (explicit context available)");
        SenderHistory::default()
    };
    
    // Get courses list for AI
    let courses_list = get_courses_list(pool).await
        .unwrap_or_else(|_| "No courses available".to_string());
    
    // Byte safe truncater, Emojis was breaking the system
    fn truncate_utf8(s: &str, max_chars: usize) -> String {
        let truncated: String = s.chars().take(max_chars).collect();
        if truncated.len() < s.len() {
            format!("{}...", truncated)
        } else {
            truncated
        }
    }

    let quoted_summary = quoted_message.map(|q| truncate_utf8(q, 200));
    
    // Call lightweight AI for context resolution
    let ai_hints = call_context_resolver_ai(
        message, 
        &sender_history, 
        &courses_list,
        quoted_summary.as_deref(),
        &text_extracted_parallels,
        quoted_assignment.as_ref(),
        logger,
    ).await?;
    
    // Calculate per-course schedule hints
    let course_hints = calculate_course_hints(
        &ai_hints,
        schedule_oracle,
        logger,
    );
    
    // Generate deadline hint (single assignment only)
    let deadline_hint = generate_deadline_hint(&course_hints);
    
    // Determine global deadline type
    let global_deadline_type = determine_global_deadline_type(&course_hints);
    
    // Extract quoted assignment ID
    let quoted_assignment_id = quoted_assignment.as_ref().map(|a| a.assignment_id);
    
    Ok(MessageContext {
        //parallel_codes: ai_hints.parallel_codes,
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

// ===== CONTEXT DECISION LOGIC =====

/// Determine if sender history should be loaded (avoid context pollution)
fn should_load_sender_history(
    text_extracted_parallels: &[String],
    quoted_assignment: &Option<QuotedAssignmentInfo>,
) -> bool {
    // Skip history if we have explicit context
    text_extracted_parallels.is_empty() && quoted_assignment.is_none()
}

/// Generate single deadline hint (only for unambiguous single-assignment case)
fn generate_deadline_hint(course_hints: &[CourseHint]) -> Option<String> {
    if course_hints.len() == 1 {
        let hint = &course_hints[0];
        if hint.parallel_schedules.len() == 1 {
            return hint.parallel_schedules[0].next_meeting.clone();
        }
    }
    None
}

/// Determine global deadline type across all courses
fn determine_global_deadline_type(course_hints: &[CourseHint]) -> String {
    if course_hints.is_empty() {
        return "unknown".to_string();
    }
    
    if course_hints.len() == 1 {
        return course_hints[0].deadline_type.clone();
    }
    
    // Check if all courses have same deadline type
    let types: std::collections::HashSet<_> = course_hints
        .iter()
        .map(|h| h.deadline_type.as_str())
        .collect();
    
    if types.len() == 1 {
        course_hints[0].deadline_type.clone()
    } else {
        "mixed".to_string()
    }
}

// ===== DATABASE QUERIES =====

/// Look up assignment by message ID (reliable database reference)
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

/// Get formatted list of all available courses
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

/// Get sender's historical patterns with HYBRID scoring (frequency × recency × context)
async fn get_sender_history(
    pool: &PgPool, 
    sender_id: &str,
    text_extracted_parallels: &[String],
) -> Result<SenderHistory, sqlx::Error> {
    
    // Query with temporal and frequency data
    let records = sqlx::query!(
        r#"
        SELECT 
            c.name as course_name, 
            a.parallel_codes, 
            COUNT(*) as count,
            MAX(a.created_at) as last_used,
            -- Temporal decay: weight by recency (cast to double precision)
            AVG(
                CASE 
                    WHEN a.created_at > NOW() - INTERVAL '7 days' THEN 1.0
                    WHEN a.created_at > NOW() - INTERVAL '14 days' THEN 0.8
                    WHEN a.created_at > NOW() - INTERVAL '30 days' THEN 0.5
                    ELSE 0.2
                END
            )::double precision as "recency_weight!"
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.sender_id = $1 
          AND a.parallel_codes IS NOT NULL
          AND a.created_at > NOW() - INTERVAL '60 days'
        GROUP BY c.name, a.parallel_codes
        "#,
        sender_id
    )
    .fetch_all(pool)
    .await?;
    
   
    let mut patterns: Vec<ParallelPattern> = records
        .into_iter()
        .filter_map(|record| {
            record.parallel_codes.map(|parallel_codes| {
                let count = record.count.unwrap_or(0) as i32;
                let recency_weight = record.recency_weight as f32;
             
                let last_used = record.last_used.unwrap().naive_utc();
              
                let base_score = (count as f32) * recency_weight;
                
               
                let context_boost = calculate_context_boost(
                    &parallel_codes, 
                    text_extracted_parallels
                );
                
                let relevance_score = base_score * context_boost;
                
                ParallelPattern {
                    course_name: record.course_name,
                    parallel_codes,
                    count,
                    last_used,
                    relevance_score,
                }
            })
        })
        .collect();
    
    // Sort by relevance score (descending)
    patterns.sort_by(|a, b| {
        b.relevance_score.partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    
    // Take top N patterns only (strict limit to prevent context pollution)
    patterns.truncate(MAX_HISTORY_PATTERNS);
    
    Ok(SenderHistory {
        parallel_patterns: patterns,
    })
}

/// Calculate context boost multiplier based on parallel code overlap
fn calculate_context_boost(
    pattern_parallels: &[String],
    message_parallels: &[String],
) -> f32 {
    if message_parallels.is_empty() {
        return 1.0;
    }
    
    // Check for overlap between pattern and message parallels
    let has_overlap = pattern_parallels.iter()
        .any(|pc| message_parallels.contains(pc));
    
    if has_overlap {
        3.0
    } else {
        1.0 
    }
}

// ===== AI INTERACTION =====

/// AI hints response structure
#[derive(Debug, Deserialize)]
struct AIHints {
    //parallel_codes: Vec<String>, remove the global
    parallel_confidence: f32,
    parallel_source: String,
    course_hints: Vec<AICourseHint>,
}

/// Per-course hint from AI
#[derive(Debug, Deserialize)]
struct AICourseHint {
    course_name: String,
    parallel_codes: Vec<String>,
    deadline_type: String,
}

/// Call lightweight AI for context resolution
async fn call_context_resolver_ai(
    message: &str,
    sender_history: &SenderHistory,
    courses_list: &str,
    quoted_context: Option<&str>,
    text_extracted_parallels: &[String],
    quoted_assignment: Option<&QuotedAssignmentInfo>,
    logger: &JobLogger,
) -> Result<AIHints, String> {
    
    let history_text = format_history_for_prompt(sender_history);
    let quoted_section = build_quoted_section(quoted_assignment, quoted_context);
    let parallel_hint = build_parallel_hint(text_extracted_parallels);
    
    let prompt = build_context_resolver_prompt(
        message,
        &history_text,
        &quoted_section,
        &parallel_hint,
        courses_list,
    );
    
    // TIER 1: Try Gemini models first 
    match try_gemini_context(&prompt, logger).await {
        Ok(hints) => return Ok(hints),
        Err(e) if e == "rate limit" => {
            logger.log("│ \x1b[33m⚠️ CONTEXT\t: Gemini rate limited - Falling back to Groq...\x1b[0m");
        }
        Err(e) => {
            logger.log(&format!("│ \x1b[33m⚠️ CONTEXT\t: Gemini failed - {}\x1b[0m", e));
            logger.log("│ \x1b[36m🔄 CONTEXT\x1b[0m\t: Falling back to Groq...");
        }
    }
    
    // TIER 2: Groq reasoning models
    match try_groq_reasoning_context(&prompt, logger).await {
        Ok(hints) => return Ok(hints),
        Err(e) => {
            logger.log(&format!("│ \x1b[33m⚠️ CONTEXT\t: Groq Reasoning failed - {}\x1b[0m", e));
        }
    }
    
    // TIER 3: Groq standard text models (final fallback)
    match try_groq_standard_context(&prompt, logger).await {
        Ok(hints) => return Ok(hints),
        Err(e) => {
            logger.log(&format!("│ \x1b[31m❌ CONTEXT\t: All models failed - {}\x1b[0m", e));
        }
    }
    
    Err("All context resolver models failed".to_string())
}

// ===== GEMINI CONTEXT RESOLUTION =====

async fn try_gemini_context(prompt: &str, logger: &JobLogger) -> Result<AIHints, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set".to_string())?;
    
    for (idx, model) in GEMINI_MODELS.iter().enumerate() {
        let index = idx + 1;
        
        // Log trying line with box formatting
        print_trying_line(model, index, GEMINI_MODELS.len(), logger);
        
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );
        
        let request_body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.1,
                "maxOutputTokens": 2048,
                "responseMimeType": "application/json",
                "responseSchema": {
                    "type": "object",
                    "properties": {
                        "parallel_confidence": {
                            "type": "number",
                            "description": "Confidence level between 0.0 and 1.0"
                        },
                        "parallel_source": {
                            "type": "string",
                            "enum": ["explicit", "quoted_assignment", "sender_history", "unknown"],
                            "description": "Source of parallel code information"
                        },
                        "course_hints": {
                            "type": "array",
                            "description": "List of course-specific hints",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "course_name": {
                                        "type": "string",
                                        "description": "Full course name"
                                    },
                                    "parallel_codes": {
                                        "type": "array",
                                        "description": "Parallel class codes for this course",
                                        "items": {
                                            "type": "string"
                                        }
                                    },
                                    "deadline_type": {
                                        "type": "string",
                                        "enum": ["explicit", "next_meeting", "relative", "unknown"],
                                        "description": "Type of deadline mentioned"
                                    }
                                },
                                "required": ["course_name", "parallel_codes", "deadline_type"]
                            }
                        }
                    },
                    "required": ["parallel_confidence", "parallel_source", "course_hints"]
                }
            }
        });
        
        let client = reqwest::Client::new();
        let response = match client
            .post(&url)
            .header("X-Goog-Api-Key", &api_key)
            .json(&request_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                continue;
            }
        };
        
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            continue;
        }
        
        if response.status().is_success() {
            
            // Try to parse response
            let gemini_response: GeminiResponse = match response.json().await {
                Ok(r) => r,
                Err(e) => {
                    clear_trying_line(logger);
                    logger.log(&format!("│ \x1b[31m❌ PARSE FAILED\x1b[0m\t: {} - JSON deserialize error: {}", model, e));
                    if index < GEMINI_MODELS.len() { continue; } 
                    else { return Err(format!("JSON parse error: {}", e)); }
                }
            };
            
            // Try to extract text
            let ai_text = match extract_ai_text(&gemini_response) {
                Ok(text) => text,
                Err(e) => {
                    clear_trying_line(logger);
                    logger.log(&format!("│ \x1b[31m❌ EXTRACT FAILED\x1b[0m\t: {} - {}", model, e));
                    if index < GEMINI_MODELS.len() { continue; } 
                    else { return Err(e); }
                }
            };
            
            // Try to parse AI hints
            let hints = match parse_ai_hints(&ai_text) {
                Ok(h) => h,
                Err(e) => {
                    clear_trying_line(logger);
                    logger.log(&format!("│ \x1b[31m❌ HINTS PARSE FAILED\x1b[0m\t: {} - {}", model, e));
                    if index < GEMINI_MODELS.len() { continue; } 
                    else { return Err(e); }
                }
            };
            
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Gemini {}/{})", model, index, GEMINI_MODELS.len()));
            
            return Ok(hints);
        }
        
        // Log the error 
        if response.status() == reqwest::StatusCode::BAD_REQUEST {
            let error_text = response.text().await.unwrap_or_default();
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - 400 Bad Request", model));
            logger.log(&format!("│   Error: {}", error_text.chars().take(100).collect::<String>()));
            continue;
        }
        
        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, response.status()));
    }
    
    Err("All Gemini models failed".to_string())
}

// ===== GROQ REASONING CONTEXT RESOLUTION =====

async fn try_groq_reasoning_context(prompt: &str, logger: &JobLogger) -> Result<AIHints, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    for (idx, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_REASONING_MODELS.len(), logger);
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.1,
            "max_completion_tokens": 2048,
            "response_format": { "type": "json_object" }  
        });
        
        let client = reqwest::Client::new();
        let response = match client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\t: {} - Network error\x1b[0m", model));
                continue;
            }
        };
        
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            continue;
        }
        
        if response.status().is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ ✅ SUCCESS\t: {} (Groq Reasoning {}/{})", model, index, GROQ_REASONING_MODELS.len()));
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Parse error: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            return parse_ai_hints(&ai_text);
        }
        
        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\t: {} - HTTP {}\x1b[0m", model, response.status()));
    }

    Err("All Groq reasoning models failed".to_string())
}

// ===== GROQ STANDARD CONTEXT RESOLUTION =====

async fn try_groq_standard_context(prompt: &str, logger: &JobLogger) -> Result<AIHints, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;
    
    for (idx, model) in GROQ_TEXT_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_TEXT_MODELS.len(), logger);
        
        let url = "https://api.groq.com/openai/v1/chat/completions";
        
        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.1,
            "max_tokens": 1000,
            "response_format": { "type": "json_object" }  
        });
        
        let client = reqwest::Client::new();
        let response = match client.post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\t: {} - Network error\x1b[0m", model));
                if index == GROQ_TEXT_MODELS.len() - 1 {
                    return Err(format!("Request failed: {}", e));
                }
                continue;
            }
        };
        
        let status = response.status();
        
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            let error_text = response.text().await.unwrap_or_default();
            
            if let Some(retry_after) = extract_retry_after(&error_text) {
                logger.log(&format!("│ ⏳ RATE LIMIT\t: {} - retry in {}", model, retry_after));
            } else {
                logger.log(&format!("│ ⏳ RATE LIMIT\t: {}", model));
            }
            
            if index < GROQ_TEXT_MODELS.len() - 1 {
                continue;
            } else {
                return Err("All Groq standard models rate limited".to_string());
            }
        }
        
        if status.is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ ✅ SUCCESS\t: {} (Groq Standard {}/{})", model, index, GROQ_TEXT_MODELS.len()));
            
            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Parse error: {}", e))?;
            
            let ai_text = extract_groq_text(&groq_response)?;
            return parse_ai_hints(&ai_text);
        }
        
        if status == reqwest::StatusCode::BAD_REQUEST {
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[31m❌ FAILED\t: {} - 400 Bad Request\x1b[0m", model));
            if index < GROQ_TEXT_MODELS.len() - 1 {
                continue;
            }
        }
        
        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\t: {} - HTTP {}\x1b[0m", model, status));
    }
    
    Err("All Groq standard models failed".to_string())
}

// ===== TRYING LINE HELPERS =====

fn print_trying_line(model: &str, index: usize, total: usize, logger: &JobLogger) {
    use std::io::Write;
    print!("\r│ 🔄 TRYING : {} ({}/{})                    ", model, index, total);
    let _ = std::io::stdout().flush();
    
    // Dashboard: send trying update
    logger.log_trying(model, index, total);
}

fn clear_trying_line(logger: &JobLogger) {
    // Console: clear with \r
    use std::io::Write;
    print!("\r                                                                  \r");
    let _ = std::io::stdout().flush();
    
    // Dashboard: clear trying state
    logger.log_trying_clear();
}

/// Format sender history for prompt with relevance scoring
fn format_history_for_prompt(history: &SenderHistory) -> String {
    if history.parallel_patterns.is_empty() {
        return "None".to_string();
    }
    
    history.parallel_patterns
        .iter()
        .enumerate()
        .map(|(i, pattern)| {
            let age = Utc::now()
                .signed_duration_since(
                    DateTime::<Utc>::from_naive_utc_and_offset(pattern.last_used, Utc)
                )
                .num_days();
            
            format!(
                "{}. {}: [{}] (used {}x, {} days ago, relevance: {:.1})",
                i + 1,
                pattern.course_name,
                pattern.parallel_codes.join(", "),
                pattern.count,
                age,
                pattern.relevance_score
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ")
}

/// Build quoted assignment section for prompt
fn build_quoted_section(
    quoted_assignment: Option<&QuotedAssignmentInfo>,
    quoted_context: Option<&str>,
) -> String {
    if let Some(assignment) = quoted_assignment {
        format!(
            r#"

CRITICAL: User is replying to this assignment from database:
  Course: {}
  Title: {}
  Parallels: [{}]
  
  Action: Determine if user is UPDATING this assignment or announcing a NEW one.
  - If adding deadline/details to this assignment → UPDATE
  - If announcing different work → NEW"#,
            assignment.course_name,
            assignment.title,
            assignment.parallel_codes.join(", ")
        )
    } else if let Some(ctx) = quoted_context {
        format!(
            "\n\nUser is replying to: \"{}\"\nCheck if this is an update or new info.",
            ctx.chars().take(100).collect::<String>()
        )
    } else {
        String::new()
    }
}

/// Build parallel extraction hint for prompt
fn build_parallel_hint(text_extracted_parallels: &[String]) -> String {
    if !text_extracted_parallels.is_empty() {
        format!(
            "\n\nEXTRACTED PARALLEL CODES FROM MESSAGE: [{}]",
            text_extracted_parallels.join(", ")
        )
    } else {
        String::new()
    }
}

/// Build context resolver prompt with clear priority hierarchy
fn build_context_resolver_prompt(
    message: &str,
    history_text: &str,
    quoted_section: &str,
    parallel_hint: &str,
    courses_list: &str,
) -> String {
    format!(
        r#"Extract structured course information from an academic message.

MESSAGE: "{}"

CONTEXT PRIORITY HIERARCHY:
═══════════════════════════════════════════════════════════════════
1. EXTRACTED PARALLEL CODES           → HIGHEST PRIORITY - Explicitly mentioned in MESSAGE
2. QUOTED ASSIGNMENT (if present)     → Use for course/title context only
3. SENDER HISTORY (top 3 by relevance) → FALLBACK ONLY - Use when ambiguous
═══════════════════════════════════════════════════════════════════
{}{}

SENDER HISTORY (sorted by: frequency × recency × context-match):
  {}

AVAILABLE COURSES:
{}

═══════════════════════════════════════════════════════════════════
CRITICAL INSTRUCTION: PARALLEL CODE EXTRACTION FOR UPDATES
═══════════════════════════════════════════════════════════════════

⚠️ SPECIAL CASE: UPDATE MESSAGES
If the MESSAGE contains update keywords like:
  - "paralel untuk K2 P2"
  - "diubah menjadi k1, k2"
  - "untuk parallel K3"

→ YOU MUST extract parallels from the UPDATE MESSAGE, NOT from quoted assignment
→ The quoted assignment shows OLD parallels, MESSAGE shows NEW parallels
→ Example: 
   - Quoted shows "Parallels: [all]"
   - Message says "paralel untuk K2 P2"
   - CORRECT answer: ["k2", "p2"]  ← From MESSAGE
   - WRONG answer: ["all"]  ← From quoted assignment

═══════════════════════════════════════════════════════════════════
STANDARD PRIORITY FOR NEW ASSIGNMENTS
═══════════════════════════════════════════════════════════════════

IF EXTRACTED PARALLEL CODES exist in MESSAGE:
  → Use these codes (explicitly mentioned in message)
  → Example: Message says "GRAFKOM K2" → Return ["k2"]
  → Example: Message says "untuk k1, k2" → Return ["k1", "k2"]

IF QUOTED ASSIGNMENT exists AND no explicit codes in MESSAGE:
  → Use quoted assignment's parallel codes
  → This is database-verified information (100% reliable)

IF SENDER HISTORY exists AND no explicit codes:
  → Use Pattern #1 (highest relevance score) as fallback
  → History is PRE-FILTERED: only top 3 most relevant patterns shown

═══════════════════════════════════════════════════════════════════
TASK DEFINITION
═══════════════════════════════════════════════════════════════════

Identify courses and classify deadline information as structured JSON.

IMPORTANT: This message may reference ONE OR MORE assignments. Each has:
- Its own course
- Its own set of parallel codes  
- Its own deadline type

Extract EACH DISTINCT ASSIGNMENT separately.

ASSIGNMENT IDENTIFICATION:
- If QUOTED ASSIGNMENT exists → user likely updating that ONE assignment
- Otherwise, check if message describes multiple assignments or one
- Each assignment entry should have ONE course

COURSE IDENTIFICATION (per assignment):
- Match against AVAILABLE COURSES list only
- Use full course name, never alias (check [aka: ...])
- Assignment/project titles are NOT courses
- If QUOTED ASSIGNMENT present → use that course name EXACTLY

PARALLEL CLASS CODES (per assignment) - CRITICAL:
- Valid codes: k1-k4, p1-p4, r1-r4, or "all"
- Format: Array, can contain multiple codes
- THESE ARE PER-ASSIGNMENT, NOT PER-COURSE

Recognition patterns:
  - Course abbreviations: "GRAFKOM K2" → ["k2"]
  - Explicit lists: "untuk k1, k2" → ["k1", "k2"]
  - Explicit lists: "paralel untuk K2 P2" → ["k2", "p2"]
  - "untuk K2 dan P2" → ["k2", "p2"]
  - Keywords "semua kelas"/"all classes" → ["all"]

DEADLINE TYPE CLASSIFICATION (per assignment):
- "explicit" → Absolute dates (5 Januari, Jumat 10 Jan, tanggal 15)
- "next_meeting" → Class references (sebelum pertemuan, saat kuliah, during class)
- "relative" → Temporal references (besok, lusa, minggu depan, tomorrow)
- "unknown" → Course mentioned without deadline info

GLOBAL PARALLEL CODES:
- Return parallels common to ALL courses
- If courses have different parallels → empty array
- Single course scenario → empty array (not global)

═══════════════════════════════════════════════════════════════════
OUTPUT FORMAT
═══════════════════════════════════════════════════════════════════

Return JSON only:
{{
  "parallel_confidence": float,  // 0.0-1.0
  "parallel_source": "explicit" | "quoted_assignment" | "sender_history" | "unknown",
  "course_hints": [
    {{
      "course_name": string,
      "parallel_codes": [string],  // PER-ASSIGNMENT parallels - extract from MESSAGE first
      "deadline_type": "explicit" | "next_meeting" | "relative" | "unknown"
    }}
  ]
}}

FINAL REMINDER: 
- For UPDATE messages: Extract parallels from MESSAGE text, not from quoted assignment
- Use EXTRACTED codes from MESSAGE as HIGHEST PRIORITY
- Quoted assignment is for context only (course name, title)
- Example phrases to watch: "paralel untuk K2 P2", "untuk k1, k2", "diubah ke K3""#,
        message,
        quoted_section,
        parallel_hint,
        history_text,
        courses_list
    )
}


fn extract_retry_after(error_text: &str) -> Option<String> {
    if let Some(start) = error_text.find("try again in ") {
        let rest = &error_text[start + 13..];
        if let Some(end) = rest.find('.') {
            let time = &rest[..end];
            return Some(format_duration(time));
        } else if let Some(end) = rest.find('s') {
            let time = &rest[..end];
            return Some(format_duration(time));
        }
    }
    None
}

fn format_duration(duration: &str) -> String {
    if duration.contains('m') {
        let parts: Vec<&str> = duration.split('m').collect();
        if let Some(mins) = parts.first() {
            if let Ok(m) = mins.parse::<i32>() {
                return format!("~{}min", m + 1);
            }
        }
    }
    duration.to_string()
}

/// Parse AI hints from JSON response
fn parse_ai_hints(json_text: &str) -> Result<AIHints, String> {
    serde_json::from_str(json_text)
        .map_err(|e| format!("Failed to parse AI hints: {}", e))
}

// ===== SCHEDULE CALCULATION =====

/// Calculate per-course hints with per-parallel schedule information
fn calculate_course_hints(
    hints: &AIHints,
    schedule_oracle: &ScheduleOracle,
    logger: &JobLogger,
) -> Vec<CourseHint> {
    let mut course_hints = Vec::new();
    
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let today = now.date_naive();
    
    for ai_course_hint in &hints.course_hints {
        logger.log("│");
        logger.log(&format!("│ 🎯 Processing\t: {}", ai_course_hint.course_name));
        logger.log(&format!("│    Parallels\t: {:?}", ai_course_hint.parallel_codes));
        logger.log(&format!("│    Dead. Type\t: {}", ai_course_hint.deadline_type));
        
        let parallel_schedules = match ai_course_hint.deadline_type.as_str() {
            "next_meeting" | "relative" => {
                calculate_parallel_schedules(
                    &ai_course_hint.course_name,
                    &ai_course_hint.parallel_codes,
                    &ai_course_hint.deadline_type,
                    schedule_oracle,
                    today,
                    logger,
                )
            },
            "explicit" => {
                logger.log("│    📅 Result\t: Explicit date (main AI will parse)");
                vec![]
            },
            _ => {
                logger.log("│    ❓ Result\t: Unknown type");
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

/// Calculate schedule information for each parallel code
fn calculate_parallel_schedules(
    course_name: &str,
    parallel_codes: &[String],
    deadline_type: &str,
    schedule_oracle: &ScheduleOracle,
    today: chrono::NaiveDate,
    logger: &JobLogger,
) -> Vec<ParallelSchedule> {
    if parallel_codes.is_empty() {
        logger.log("│    ⏭️ Result\t: Skipped (needs parallel for schedule)");
        return vec![];
    }
    
    if parallel_codes.contains(&"all".to_string()) {
        logger.log("│    ⏭️ Result\t: Skipped ('all' cannot determine specific schedule)");
        return vec![];
    }
    
    let hint_type = if deadline_type == "next_meeting" {
        "Next meeting"
    } else {
        "Schedule reference"
    };
    
    let mut schedules = Vec::new();
    
    for parallel in parallel_codes {
        if let Some((meeting_date, meeting_time)) = schedule_oracle
            .get_next_meeting_with_time(course_name, parallel, today)
        {
            let next_meeting = format!("{} {}", meeting_date, meeting_time);
            logger.log(&format!("│    ✅ {}: {} at {}", 
                parallel.to_uppercase(), hint_type, next_meeting));
            
            schedules.push(ParallelSchedule {
                parallel_code: parallel.clone(),
                next_meeting: Some(next_meeting),
            });
        } else {
            logger.log(&format!("│    ⏭️  {}: No schedule found", parallel.to_uppercase()));
            
            schedules.push(ParallelSchedule {
                parallel_code: parallel.clone(),
                next_meeting: None,
            });
        }
    }
    
    schedules
}