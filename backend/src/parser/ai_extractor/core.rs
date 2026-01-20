use crate::models::{AIClassification, Assignment};
use uuid::Uuid;
use serde_json::json;
use std::collections::HashMap;
use sqlx::PgPool;

use super::schedule_oracle::ScheduleOracle;
use once_cell::sync::Lazy;

use super::prompts::*;
use super::parsing::*;
use super::{GROQ_REASONING_MODELS, GROQ_VISION_MODELS, GROQ_TEXT_MODELS, GEMINI_MODELS};
use super::context_builder::build_context;

// TUI integration
use crate::tui::JobLogger;

pub static SCHEDULE_ORACLE: Lazy<ScheduleOracle> = Lazy::new(|| {
    ScheduleOracle::load_from_file("schedule.json")
        .expect("Failed to load schedule.json")
});

// Retry configuration
const MAX_RETRIES: u32 = 4;

// ===== MAIN AI EXTRACTION FUNCTION =====

pub async fn extract_with_ai(
    text: &str,
    available_courses: &str,
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    image_base64: Option<&str>,
    sender_id: &str,
    pool: &PgPool,
    quoted_message: Option<&str>,  
    quoted_message_id: Option<&str>,
    logger: &JobLogger,
) -> Result<AIClassification, String> {
    let current_datetime = get_current_datetime();
    let current_date = get_current_date();

    logger.log("┌──[AI PROCESSING]──────────\x1b[90m────────────\x1b[2m────\x1b[0m");

    let message_display = text
        .replace('\n', "\\n")
        .chars()
        .take(80)
        .collect::<String>();

    let message_truncated = if text.len() > 60 {
        format!("\"{}...\"", message_display)
    } else {
        format!("\"{}\"", message_display)
    };

    logger.log(&format!("│ 📝 Message\t: {}", message_truncated));

    if let Some(quoted) = quoted_message {
        let quoted_display = quoted
            .replace('\n', "\\n")
            .chars()
            .take(80)
            .collect::<String>();

        let quoted_truncated = if quoted.len() > 60 {
            format!("\"{}...\"", quoted_display)
        } else {
            format!("\"{}\"", quoted_display)
        };

        logger.log(&format!("│ 💬 Quoted\t: {}", quoted_truncated));
    }

    // Build context
    let context = match build_context(
        text,
        sender_id,
        pool,
        &*SCHEDULE_ORACLE,
        quoted_message,
        quoted_message_id,
        logger,
    ).await {
        Ok(ctx) => {
            let courses_summary = if ctx.course_hints.is_empty() {
                "none".to_string()
            } else {
                ctx.course_hints
                    .iter()
                    .map(|ch| {
                        if ch.parallel_schedules.is_empty() {
                            let parallel = if ch.parallel_codes.is_empty() {
                                "?".to_string()
                            } else {
                                format!("[{}]", ch.parallel_codes.join(","))
                            };
                            format!("{}:{}", ch.course_name, parallel)
                        } else {
                            let schedules_str = ch.parallel_schedules
                                .iter()
                                .map(|ps| {
                                    if let Some(ref meeting) = ps.next_meeting {
                                        format!("{}:{}", ps.parallel_code, meeting)
                                    } else {
                                        format!("{}:?", ps.parallel_code)
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(",");
                            format!("{}:[{}]", ch.course_name, schedules_str)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            let parallel_summary = if ctx.course_hints.is_empty() {
                "none".to_string()
            } else {
                let courses_with_parallels: Vec<String> = ctx.course_hints
                    .iter()
                    .filter(|ch| !ch.parallel_codes.is_empty())
                    .map(|ch| {
                        let course_abbr = ch.course_name
                            .split_whitespace()
                            .take(2)
                            .collect::<Vec<_>>()
                            .join(" ");
                        format!("{}:[{}]", course_abbr, ch.parallel_codes.join(","))
                    })
                    .collect();

                if courses_with_parallels.is_empty() {
                    "none".to_string()
                } else {
                    courses_with_parallels.join(", ")
                }
            };

            logger.log("│");
            logger.log(&format!("│ \x1b[32m✅ Context\x1b[0m\t: Detected={} ({}), Schedules=[{}]",
                parallel_summary, ctx.parallel_source, courses_summary));

            Some(ctx)
        }
        Err(e) => {
            logger.log("│");
            logger.log(&format!("│ \x1b[33m⚠️  Context failed\x1b[0m\t: {}", e));
            None
        }
    };

    let prompt = build_classification_prompt(
        text,
        available_courses,
        active_assignments,
        course_map,
        &current_datetime,
        &current_date,
        context.as_ref()
    );

    if image_base64.is_some() {
        logger.log("│ 🖼️  Image\t: Attached (may be irrelevant meme)");
    }
    logger.log(&format!("│ 📊 Context\t: {} active assignments", active_assignments.len()));
    logger.log(&format!("│ 📅 Time\t: {}", current_datetime));
    logger.log("│");

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            retry_with_countdown(attempt, logger).await;
        }

        // If image present try vision first
        if let Some(img) = image_base64 {
            match try_groq_vision(&prompt, img, logger).await {
                Ok(classification) => {
                    match classification {
                        AIClassification::Unrecognized { reason, .. } => {
                            let reason_display = reason.as_deref().unwrap_or("No reason provided");
                            logger.log(&format!("│ \x1b[36mℹ️  Vision Result\x1b[0m\t: Unrecognized ({})", reason_display));
                            logger.log("│ \x1b[36m🔄 Retrying with Gemini text-only...\x1b[0m");
                            logger.log("│");

                            match try_gemini_models(&prompt, logger).await {
                                Ok(text_result) => {
                                    match text_result {
                                        AIClassification::Unrecognized { .. } => {
                                            logger.log("│ \x1b[33m⚠️  Gemini: Still unrecognized\x1b[0m");
                                            logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
                                            return Ok(AIClassification::Unrecognized { reason: None, category: crate::models::UnrecognizedCategory::Informal });
                                        }
                                        _ => {
                                            log_classification_success(&text_result, logger);
                                            logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
                                            return Ok(text_result);
                                        }
                                    }
                                }
                                Err(_) => {
                                    // continue to fallback
                                }
                            }
                        }
                        _ => {
                            log_classification_success(&classification, logger);
                            logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
                            return Ok(classification);
                        }
                    }
                }
                Err(_) => {
                    // gemini fallback
                    match try_gemini_models(&prompt, logger).await {
                        Ok(classification) => {
                            log_classification_success(&classification, logger);
                            logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
                            return Ok(classification);
                        }
                        Err(_) => {}
                    }
                }
            }
        } else {
            // No image: try gemini first
            match try_gemini_models(&prompt, logger).await {
                Ok(classification) => {
                    log_classification_success(&classification, logger);
                    logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
                    return Ok(classification);
                }
                Err(_) => {
                    logger.log("│ \x1b[36m🔄 Falling back to Groq...\x1b[0m");
                    logger.log("│");
                }
            }
        }

        // Groq reasoning fallback
        match try_groq_reasoning(&prompt, logger).await {
            Ok(classification) => {
                log_classification_success(&classification, logger);
                logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
                return Ok(classification);
            }
            Err(_) => {
                if attempt < MAX_RETRIES - 1 {
                    logger.log(&format!("│ \x1b[33m⚠️ All models failed - will retry ({}/{})\x1b[0m", attempt + 1, MAX_RETRIES - 1));
                }
            }
        }
    }

    logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
    Err("All models failed after retries".to_string())
}

// ===== RETRY HELPERS =====

async fn retry_with_countdown(attempt: u32, logger: &JobLogger) {
    let delay = 10 * attempt as u64;

    for remaining in (1..=delay).rev() {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        logger.log_countdown(attempt, remaining);
    }

    // Clear countdown line after completion
    logger.log_countdown_clear();
    logger.log("│");
}

// ===== TRYING-LINE HELPERS =====
// CLEANED UP: Always log to both console and dashboard

fn print_trying_line(model: &str, index: usize, total: usize, logger: &JobLogger) {
    // Console: overwrite with \r for animation
    use std::io::Write;
    print!("\r│ 🔄 TRYING : {} ({}/{})                    ", model, index, total);
    let _ = std::io::stdout().flush();
    
    // Dashboard: send trying update
    logger.log_trying(model, index, total);
}

fn clear_trying_line(logger: &JobLogger) {
    // Console: clear line with \r
    use std::io::Write;
    print!("\r                                                                  \r");
    let _ = std::io::stdout().flush();
    
    // Dashboard: clear trying state
    logger.log_trying_clear();
}

// ===== GEMINI MODELS =====

async fn try_gemini_models(prompt: &str, logger: &JobLogger) -> Result<AIClassification, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;

    let mut all_rate_limited = true;

    for (idx, model) in GEMINI_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GEMINI_MODELS.len(), logger);

        let request_body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": 4096,
                "responseMimeType": "application/json"
            }
        });

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );

        let client = reqwest::Client::new();
        let response = match client
            .post(&url)
            .header("X-Goog-Api-Key", &api_key)
            .json(&request_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                all_rate_limited = false;
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                if index < GEMINI_MODELS.len() { continue; } else { return Err(format!("Network error: {}", e)); }
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            if index < GEMINI_MODELS.len() { continue; } else { continue; }
        }

        if status.is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Gemini {}/{})", model, index, GEMINI_MODELS.len()));

            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;

            let ai_text = extract_ai_text(&gemini_response)?;
            let classification = parse_classification(ai_text)?;

            return Ok(classification);
        }

        all_rate_limited = false;
        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m : {} - HTTP {}", model, status));
        if index < GEMINI_MODELS.len() { continue; }
    }

    if all_rate_limited {
        logger.log("│ \x1b[33m⚠️ R-LIMIT on all gemini models\x1b[0m");
        return Err("rate limit".to_string());
    }

    Err("All Gemini models failed".to_string())
}

// ===== GROQ REASONING =====

async fn try_groq_reasoning(prompt: &str, logger: &JobLogger) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;

    let mut all_rate_limited = true;

    for (idx, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_REASONING_MODELS.len(), logger);

        let url = "https://api.groq.com/openai/v1/chat/completions";

        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.6,
            "top_p": 0.95,
            "max_completion_tokens": 8192,
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
                all_rate_limited = false;
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                if index < GROQ_REASONING_MODELS.len() { continue; } else {
                    logger.log("│ \x1b[36m🔄 Falling back to standard models...\x1b[0m");
                    logger.log("│");
                    return try_groq_standard_text(prompt, logger).await;
                }
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            if index < GROQ_REASONING_MODELS.len() { continue; } else { continue; }
        }

        if status.is_success() {
            all_rate_limited = false;
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Groq Reasoning {}/{})", model, index, GROQ_REASONING_MODELS.len()));

            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;

            let ai_text = extract_groq_text(&groq_response)?;
            let classification = parse_classification(&ai_text)?;

            if matches!(classification, AIClassification::Unrecognized { .. }) && !ai_text.contains("unrecognized") {
                if index < GROQ_REASONING_MODELS.len() - 1 { continue; }
            }

            return Ok(classification);
        }

        all_rate_limited = false;
        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m : {} - HTTP {}", model, status));
        if index < GROQ_REASONING_MODELS.len() { continue; }
    }

    if all_rate_limited {
        logger.log("│ \x1b[33m⚠️ R-LIMIT on all Groq reasoning models\x1b[0m");
        logger.log("│ \x1b[36m🔄 Falling back to standard models...\x1b[0m");
        logger.log("│");
        return try_groq_standard_text(prompt, logger).await;
    }

    logger.log("│ \x1b[36m🔄 Falling back to standard models...\x1b[0m");
    logger.log("│");
    try_groq_standard_text(prompt, logger).await
}

// ===== GROQ STANDARD TEXT =====

async fn try_groq_standard_text(prompt: &str, logger: &JobLogger) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;

    for (idx, model) in GROQ_TEXT_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_TEXT_MODELS.len(), logger);

        let url = "https://api.groq.com/openai/v1/chat/completions";

        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.2,
            "max_tokens": 4096,
            "top_p": 0.95,
            "frequency_penalty": 0.0,
            "presence_penalty": 0.0,
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
            Err(_) => {
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                if index < GROQ_TEXT_MODELS.len() { continue; } else { return Err("All Groq standard models failed".to_string()); }
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            if index < GROQ_TEXT_MODELS.len() { continue; } else { return Err("rate limit".to_string()); }
        }

        if status.is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Groq Standard {}/{})", model, index, GROQ_TEXT_MODELS.len()));

            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;

            let ai_text = extract_groq_text(&groq_response)?;
            let classification = parse_classification(&ai_text)?;

            if matches!(classification, AIClassification::Unrecognized { .. }) && !ai_text.contains("unrecognized") {
                if index < GROQ_TEXT_MODELS.len() - 1 { continue; }
            }

            return Ok(classification);
        }

        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, status));
        if index < GROQ_TEXT_MODELS.len() { continue; }
    }

    Err("All Groq standard models failed".to_string())
}

// ===== GROQ VISION =====

async fn try_groq_vision(prompt: &str, image_base64: &str, logger: &JobLogger) -> Result<AIClassification, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set in .env".to_string())?;

    for (idx, model) in GROQ_VISION_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_VISION_MODELS.len(), logger);

        let url = "https://api.groq.com/openai/v1/chat/completions";

        let request_body = json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/jpeg;base64,{}", image_base64)
                        }
                    }
                ]
            }],
            "temperature": 0.2,
            "max_tokens": 4096,
            "top_p": 0.95,
            "frequency_penalty": 0.0,
            "presence_penalty": 0.0,
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
            Err(_) => {
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                if index < GROQ_VISION_MODELS.len() { continue; } else { return Err("All Groq vision models failed".to_string()); }
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            if index < GROQ_VISION_MODELS.len() { continue; } else { return Err("rate limit".to_string()); }
        }

        if status.is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Vision {}/{})", model, index, GROQ_VISION_MODELS.len()));

            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;

            let ai_text = extract_groq_text(&groq_response)?;
            let classification = parse_classification(&ai_text)?;

            if matches!(classification, AIClassification::Unrecognized { .. }) && !ai_text.contains("unrecognized") {
                if index < GROQ_VISION_MODELS.len() - 1 { continue; }
            }

            return Ok(classification);
        }

        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, status));
        if index < GROQ_VISION_MODELS.len() { continue; }
    }

    Err("All Groq vision models failed".to_string())
}

// ===== DEDUPLICATION AI =====

pub async fn check_duplicate_assignment(
    title: &str,
    description: &str,
    course_name: &str,
    parallel_codes: &[String],
    existing_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    logger: &JobLogger,
) -> Result<Option<(Uuid, String)>, String> {
    let new_numbers = extract_numbers(title);
    let new_type = extract_assignment_type(title);

    fn parallels_overlap(new_codes: &[String], existing_codes: &[String]) -> bool {
        let clean_new = if new_codes.iter().any(|c| c.eq_ignore_ascii_case("all")) {
            vec!["all".to_string()]
        } else {
            new_codes.to_vec()
        };

        let clean_existing = if existing_codes.iter().any(|c| c.eq_ignore_ascii_case("all")) {
            vec!["all".to_string()]
        } else {
            existing_codes.to_vec()
        };

        if clean_new.iter().any(|c| c == "all") || clean_existing.iter().any(|c| c == "all") {
            return true;
        }

        if clean_new.is_empty() && clean_existing.is_empty() { return true; }
        if clean_new.is_empty() || clean_existing.is_empty() { return false; }

        clean_new.iter().any(|new_p|
            clean_existing.iter().any(|existing_p|
                new_p.eq_ignore_ascii_case(existing_p)
            )
        )
    }

    // STEP 1: Filter candidates
    let filtered: Vec<&Assignment> = existing_assignments
        .iter()
        .filter(|a| {
            let same_course = a.course_id
                .and_then(|id| course_map.get(&id))
                .map(|name| name.eq_ignore_ascii_case(course_name))
                .unwrap_or(false);

            if !same_course { return false; }

            if !parallels_overlap(parallel_codes, &a.parallel_codes) { return false; }

            let existing_numbers = extract_numbers(&a.title);
            if !new_numbers.is_empty() && !existing_numbers.is_empty() {
                if new_numbers != existing_numbers { return false; }
            }

            if let Some(ref new_t) = new_type {
                if let Some(existing_t) = extract_assignment_type(&a.title) {
                    if new_t != &existing_t { return false; }
                }
            }

            let similarity = calculate_word_overlap(title, &a.title);
            if similarity < 0.2 { return false; }

            true
        })
        .collect();

    // Log filtering results
    if filtered.is_empty() {
        logger.log(&format!("│ 🔍 Checked {} assignments\t: No candidates (course/parallel/type mismatch)", existing_assignments.len()));
        return Ok(None);
    }

    if filtered.len() > 3 {
        logger.log(&format!("│ 🔍 Checked {} assignments\t: Too many candidates ({}) - skipping AI check", existing_assignments.len(), filtered.len()));
        return Ok(None);
    }

    // Show what we're checking
    logger.log(&format!("│ \x1b[36m🔍 Checked {} assignments\x1b[0m\t: {} candidates for AI verification", existing_assignments.len(), filtered.len()));

    let filtered_owned: Vec<Assignment> = filtered.into_iter().cloned().collect();
    let prompt = build_duplicate_detection_prompt(
        title,
        description,
        course_name,
        parallel_codes,
        &filtered_owned,
        course_map,
    );

    // STEP 2: AI verification with retry
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            retry_with_countdown(attempt, logger).await;
        }

        match try_gemini_duplicate_check(&prompt, logger).await {
            Ok(result) => return Ok(result),
            Err(e) if e == "rate limit" => {
                logger.log("│ \x1b[36m🔄 Gemini rate limited\x1b[0m\t: Trying Groq...");
            }
            Err(_) => {}
        }

        match try_groq_duplicate_check(&prompt, logger).await {
            Ok(result) => return Ok(result),
            Err(_) => {
                if attempt < MAX_RETRIES - 1 {
                    logger.log(&format!("│ \x1b[33m⚠️ Attempt {}/{} failed\x1b[0m\t: Retrying duplicate check...", attempt + 1, MAX_RETRIES - 1));
                }
            }
        }
    }

    logger.log(&format!("│ \x1b[31m❌ All {} retry attempts failed\x1b[0m\t: Treating as new assignment", MAX_RETRIES));
    Err("All duplicate check attempts failed".to_string())
}

async fn try_gemini_duplicate_check(prompt: &str, logger: &JobLogger) -> Result<Option<(Uuid, String)>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set".to_string())?;

    let mut all_rate_limited = true;

    for (idx, model) in GEMINI_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GEMINI_MODELS.len(), logger);

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );

        let request_body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.0,
                "maxOutputTokens": 1024,
                "topP": 0.95,
                "responseMimeType": "application/json"
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
                all_rate_limited = false;
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t\t\t: {} - Network error", model));
                continue;
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            continue;
        }

        if status.is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t\t\t: {} (Gemini {}/{})", model, index, GEMINI_MODELS.len()));

            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Parse error: {}", e))?;

            let ai_text = extract_ai_text(&gemini_response)?;

            let result: DuplicateCheckResult = serde_json::from_str(&ai_text)
                .map_err(|e| {
                    logger.log(&format!("│ \x1b[31m❌ JSON Parse Failed: {} | Raw: {}\x1b[0m", 
                        e, ai_text.chars().take(100).collect::<String>()));
                    format!("JSON error: {}", e)
                })?;

            let reason = result.reason.clone();

            if result.is_duplicate && result.confidence == "high" {
                if let Some(ref id_str) = result.matched_assignment_id {
                    match Uuid::parse_str(id_str) {
                        Ok(uuid) => {
                            logger.log(&format!("│ \x1b[32m✅ Duplicate Match\x1b[0m\t\t: {} (confidence: high)", uuid));
                            return Ok(Some((uuid, reason)));
                        }
                        Err(_) => {
                            logger.log(&format!("│ \x1b[33m⚠️ Invalid UUID\x1b[0m\t\t: {}", id_str));
                            return Ok(None);
                        }
                    }
                } else {
                    logger.log("│ \x1b[33m⚠️ High confidence duplicate but no ID provided\x1b[0m");
                    return Ok(None);
                }
            } else if result.is_duplicate {
                logger.log(&format!("│ \x1b[33m⚠️ Low confidence ({})\x1b[0m\t\t: Treating as non-duplicate", result.confidence));
                return Ok(None);
            } else {
                logger.log("│ \x1b[32m✅ Not a duplicate\x1b[0m");
                return Ok(None);
            }
        }

        all_rate_limited = false;
        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, status));
    }

    if all_rate_limited { return Err("rate limit".to_string()); }

    Err("All Gemini models failed".to_string())
}

async fn try_groq_duplicate_check(prompt: &str, logger: &JobLogger) -> Result<Option<(Uuid, String)>, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;

    for (idx, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_REASONING_MODELS.len(), logger);

        let url = "https://api.groq.com/openai/v1/chat/completions";

        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.0,
            "max_completion_tokens": 1024,
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
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                continue;
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            continue;
        }

        if status.is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Groq {}/{})", model, index, GROQ_REASONING_MODELS.len()));

            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Parse error: {}", e))?;

            let ai_text = extract_groq_text(&groq_response)?;
            
            let result: DuplicateCheckResult = serde_json::from_str(&ai_text)
                .map_err(|e| {
                    logger.log(&format!("│ \x1b[31m❌ JSON Parse Failed: {} | Raw: {}\x1b[0m", 
                        e, ai_text.chars().take(100).collect::<String>()));
                    format!("JSON error: {}", e)
                })?;

            let reason = result.reason.clone();

            if result.is_duplicate && result.confidence == "high" {
                if let Some(ref id_str) = result.matched_assignment_id {
                    match Uuid::parse_str(id_str) {
                        Ok(uuid) => {
                            logger.log(&format!("│ \x1b[32m✅ Duplicate Match\x1b[0m\t: {} (confidence: high)", uuid));
                            return Ok(Some((uuid, reason)));
                        }
                        Err(_) => {
                            logger.log(&format!("│ \x1b[33m⚠️ Invalid UUID\x1b[0m\t: {}", id_str));
                            return Ok(None);
                        }
                    }
                } else {
                    logger.log("│ \x1b[33m⚠️ High confidence duplicate but no ID provided\x1b[0m");
                    return Ok(None);
                }
            } else if result.is_duplicate {
                logger.log(&format!("│ \x1b[33m⚠️ Low confidence ({})\x1b[0m\t: Treating as non-duplicate", result.confidence));
                return Ok(None);
            } else {
                logger.log("│ \x1b[32m✅ Not a duplicate\x1b[0m");
                return Ok(None);
            }
        }

        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, status));
    }

    Err("All Groq models failed".to_string())
}


// ===== MATCHING =====

pub async fn match_update_to_assignment(
    changes: &str,
    keywords: &[String],
    active_assignments: &[Assignment],
    course_map: &HashMap<Uuid, String>,
    parallel_codes: &[String],
    logger: &JobLogger,
) -> Result<Option<(Uuid, String)>, String> {
    let prompt = build_matching_prompt(changes, keywords, active_assignments, course_map, parallel_codes);

    logger.log("┌──[AI MATCHING]────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
    logger.log(&format!("│ 🔍 Keywords\t: {:?}", keywords));

    if !parallel_codes.is_empty() {
        logger.log(&format!("│ 🧩 Parallels\t: [{}]", parallel_codes.join(", ")));
    }
    logger.log("│");

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            retry_with_countdown(attempt, logger).await;
        }

        match try_gemini_matching(&prompt, logger).await {
            Ok(result) => {
                logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
                return Ok(result);
            }
            Err(e) if e == "rate limit" => {
                logger.log("│ \x1b[36m🔄 Falling back to Groq for matching...\x1b[0m");
            }
            Err(_) => {}
        }

        match try_groq_matching(&prompt, logger).await {
            Ok(result) => {
                logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
                return Ok(result);
            }
            Err(_) => {
                if attempt < MAX_RETRIES - 1 {
                    logger.log(&format!("│ \x1b[33m⚠️ Matching failed - will retry ({}/{})\x1b[0m", attempt + 1, MAX_RETRIES - 1));
                }
            }
        }
    }

    logger.log(&format!("│ \x1b[31m❌ CRITICAL: Matching failed after {} retries\x1b[0m", MAX_RETRIES));
    logger.log("└──────────────────────────\x1b[90m────────────\x1b[2m────\x1b[0m");
    Err("All matching attempts failed".to_string())
}

async fn try_gemini_matching(prompt: &str, logger: &JobLogger) -> Result<Option<(Uuid, String)>, String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set in .env".to_string())?;

    let mut all_rate_limited = true;

    for (idx, model) in GEMINI_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GEMINI_MODELS.len(), logger);

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );

        let request_body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": 4096,
                "responseMimeType": "application/json"
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
                all_rate_limited = false;
                clear_trying_line(logger);
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                continue;
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            continue;
        }

        if status.is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Gemini {}/{})", model, index, GEMINI_MODELS.len()));

            let gemini_response: GeminiResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;

            let ai_text = extract_ai_text(&gemini_response)?;
            let result = parse_match_result(ai_text, logger)?;

            return Ok(result);
        }

        all_rate_limited = false;
        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, status));
    }

    if all_rate_limited { return Err("rate limit".to_string()); }

    Err("All Gemini models failed for matching".to_string())
}

async fn try_groq_matching(prompt: &str, logger: &JobLogger) -> Result<Option<(Uuid, String)>, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set".to_string())?;

    for (idx, model) in GROQ_REASONING_MODELS.iter().enumerate() {
        let index = idx + 1;
        print_trying_line(model, index, GROQ_REASONING_MODELS.len(), logger);

        let url = "https://api.groq.com/openai/v1/chat/completions";

        let request_body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.2,
            "max_completion_tokens": 4096,
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
                logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - Network error", model));
                continue;
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            clear_trying_line(logger);
            continue;
        }

        if status.is_success() {
            clear_trying_line(logger);
            logger.log(&format!("│ \x1b[32m✅ SUCCESS\x1b[0m\t: {} (Groq {}/{})", model, index, GROQ_REASONING_MODELS.len()));

            let groq_response: GroqResponse = response.json().await
                .map_err(|e| format!("Failed to deserialize: {}", e))?;

            let ai_text = extract_groq_text(&groq_response)?;
            let result = parse_match_result(&ai_text, logger)?;

            return Ok(result);
        }

        clear_trying_line(logger);
        logger.log(&format!("│ \x1b[31m❌ FAILED\x1b[0m\t: {} - HTTP {}", model, status));
    }

    Err("All Groq models failed for matching".to_string())
}

// ===== LOGGING HELPER =====

fn log_classification_success(classification: &AIClassification, logger: &JobLogger) {
    logger.log("│");
    match classification {
        AIClassification::MultipleAssignments { assignments, .. } => {
            logger.log(&format!("│ \x1b[32m✅ Result\x1b[0m\t: {} assignments detected", assignments.len()));
            for (i, a) in assignments.iter().enumerate() {
                let parallels = if a.parallel_codes.is_empty() {
                    "N/A".to_string()
                } else {
                    format!("[{}]", a.parallel_codes.join(", "))
                };
                logger.log(&format!("│    {}. {} - {} (parallels: {})", i + 1, a.course_name, a.title, parallels));
            }
        }
        AIClassification::AssignmentInfo { course_name, title, parallel_codes, .. } => {
            let course_display = course_name.as_deref().unwrap_or("Unknown");
            let parallels = if parallel_codes.is_empty() {
                "N/A".to_string()
            } else {
                format!("[{}]", parallel_codes.join(", "))
            };
            logger.log(&format!("│ \x1b[32m✅ Result\x1b[0m\t: Single assignment ({} - {}, parallels: {})", course_display, title, parallels));
        }
        AIClassification::AssignmentUpdate { reference_keywords, parallel_codes, .. } => {
            let parallels = if parallel_codes.is_empty() {
                "N/A".to_string()
            } else {
                format!("[{}]", parallel_codes.join(", "))
            };
            logger.log(&format!("│ \x1b[32m✅ Result\x1b[0m\t: Update detected (keywords: {:?}, parallels: {})", reference_keywords, parallels));
        }
        AIClassification::Unrecognized { reason, category } => {
            use crate::models::UnrecognizedCategory;
            match category {
                UnrecognizedCategory::Informal => {
                    logger.log("│ \x1b[36mℹ️ Result\x1b[0m\t: Informal chat (no academic context)");
                }
                UnrecognizedCategory::AcademicRelated => {
                    let reason_display = reason.as_deref().unwrap_or("No reason provided");
                    logger.log(&format!("│ \x1b[36mℹ️ Result\x1b[0m\t: Academic-related but not assignment ({})", reason_display));
                }
            }
        }
    }
}