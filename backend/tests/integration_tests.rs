use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::Semaphore;

use marbot::models::AIClassification;
use marbot::parser::ai_extractor::extract_with_ai;
use marbot::database::crud;

// ANSI color codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const GRAY: &str = "\x1b[90m";

#[derive(Debug, Deserialize, Clone)]
struct TestCase {
    name: String,
    message: String,
    expected_type: String,
    #[serde(default)]
    quoted_message: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct TestResult {
    name: String,
    passed: bool,
    expected: String,
    actual: String,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_preview: Option<String>,
    retry_count: u32,
}

#[derive(Debug, Serialize)]
struct TestSummary {
    total: usize,
    passed: usize,
    failed: usize,
    success_rate: f64,
    total_retries: u32,
    by_category: HashMap<String, CategoryStats>,
    failures: Vec<FailureDetail>,
}

#[derive(Debug, Serialize)]
struct CategoryStats {
    total: usize,
    passed: usize,
    failed: usize,
    success_rate: f64,
}

#[derive(Debug, Serialize)]
struct FailureDetail {
    name: String,
    category: String,
    expected: String,
    actual: String,
    message_preview: String,
    error: Option<String>,
}

#[tokio::test]
async fn run_all_test_cases() {
    // Load environment
    if std::path::Path::new(".env.test").exists() {
        dotenv::from_filename(".env.test").ok();
    } else {
        dotenv::dotenv().ok();
    }
    
    print_header("ENVIRONMENT VALIDATION");
    
    let gemini_key = std::env::var("GEMINI_API_KEY").ok();
    let groq_key = std::env::var("GROQ_API_KEY").ok();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    
    println!("  {}GEMINI_API_KEY{}  : {}", CYAN, RESET, format_key_status(&gemini_key));
    println!("  {}GROQ_API_KEY{}    : {}", CYAN, RESET, format_key_status(&groq_key));
    println!("  {}DATABASE_URL{}    : {}✅ Set{}", CYAN, RESET, GREEN, RESET);
    println!();
    
    if gemini_key.is_none() && groq_key.is_none() {
        println!("{}❌ FATAL: At least one API key must be set{}", RED, RESET);
        panic!("❌ At least one API key (GEMINI_API_KEY or GROQ_API_KEY) must be set");
    }
    
    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to database");
    
    // Verify courses exist (from production migration)
    verify_database_setup(&pool).await;
    
    // Load test cases
    let test_data = fs::read_to_string("tests/test_cases.json")
        .expect("Failed to read test_cases.json");
    
    let test_cases: Vec<TestCase> = serde_json::from_str(&test_data)
        .expect("Failed to parse test_cases.json");
    
    let limit = std::env::var("TEST_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(test_cases.len());
    
    print_header(&format!("RUNNING {} TEST CASES", limit));
    
    // Configuration
    let concurrency = std::env::var("TEST_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);
    
    let delay_secs = std::env::var("TEST_DELAY_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(6);
    
    let max_retries = std::env::var("TEST_MAX_RETRIES")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(5);
    
    let retry_delay = std::env::var("TEST_RETRY_DELAY_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    
    println!("  {}⚙️  Configuration:{}", BOLD, RESET);
    println!("     • Concurrency: {}{}{}", YELLOW, concurrency, RESET);
    println!("     • Delay between tests: {}{}s{}", YELLOW, delay_secs, RESET);
    println!("     • Max retries on rate limit: {}{}{}", YELLOW, max_retries, RESET);
    println!("     • Retry delay: {}{}s{}", YELLOW, retry_delay, RESET);
    println!();
    
    let estimated_min = (limit as u64 * delay_secs) / 60;
    let estimated_max = estimated_min + 2;
    println!("  {}⏱️  Estimated time: ~{}-{} minutes{}", CYAN, estimated_min, estimated_max, RESET);
    println!();
    
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let pool = Arc::new(pool);
    
    let mut handles = vec![];
    
    println!("{}╭{}──────────────────────────────────────────────────────────────────╮{}", 
        GRAY, RESET, RESET);
    println!("{}│{}              🧪 COMPREHENSIVE TEST EXECUTION                     {}│{}", 
        GRAY, RESET, GRAY, RESET);
    println!("{}╰{}──────────────────────────────────────────────────────────────────╯{}\n", 
        GRAY, RESET, RESET);
    
    for (i, test_case) in test_cases.iter().take(limit).enumerate() {
        let sem = semaphore.clone();
        let pool = pool.clone();
        let test_case = test_case.clone();
        let test_num = i + 1;
        let total = limit;
        
        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            
            let progress = format!("[{}/{}]", test_num, total);
            let category_tag = test_case.category.as_ref()
                .map(|c| format!(" {}{}{}", DIM, c, RESET))
                .unwrap_or_default();
            
            println!("{}┌─{}─ {}{:>8}{} {} {}", 
                GRAY, RESET, BLUE, progress, RESET, test_case.name, category_tag);
            
            let start = std::time::Instant::now();
            let result = run_single_test_with_retry(&pool, &test_case, max_retries, retry_delay).await;
            let duration = start.elapsed().as_millis() as u64;
            
            let mut test_result = result;
            test_result.duration_ms = duration;
            
            if test_result.passed {
                let retry_info = if test_result.retry_count > 0 {
                    format!(" {}(retried {} times){}", DIM, test_result.retry_count, RESET)
                } else {
                    String::new()
                };
                println!("{}└─{}─ {}✅ PASS{} {}({}ms){}{}", 
                    GRAY, RESET, GREEN, RESET, DIM, duration, RESET, retry_info);
            } else {
                println!("{}└─{}─ {}❌ FAIL{}", GRAY, RESET, RED, RESET);
                println!("   {}Expected:{} {}{}{}", 
                    YELLOW, RESET, CYAN, test_result.expected, RESET);
                println!("   {}Got:{}      {}{}{}", 
                    YELLOW, RESET, MAGENTA, test_result.actual, RESET);
                
                if let Some(ref err) = test_result.error {
                    let short_err = if err.len() > 100 {
                        format!("{}...", &err[..100])
                    } else {
                        err.clone()
                    };
                    println!("   {}Error:{} {}{}{}", YELLOW, RESET, RED, short_err, RESET);
                }
                
                let preview = test_case.message.chars().take(80).collect::<String>();
                println!("   {}Message:{} {}{}{}", 
                    YELLOW, RESET, DIM, preview, RESET);
            }
            println!();
            
            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
            
            test_result
        });
        
        handles.push(handle);
    }
    
    // Collect results
    let mut results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }
    
    results.sort_by(|a, b| a.name.cmp(&b.name));
    
    let summary = generate_summary(&results);
    
    // Save results
    let results_json = serde_json::to_string_pretty(&results)
        .expect("Failed to serialize results");
    fs::write("test-results.json", results_json)
        .expect("Failed to write test-results.json");
    
    let summary_json = serde_json::to_string_pretty(&summary)
        .expect("Failed to serialize summary");
    fs::write("test-summary.json", summary_json)
        .expect("Failed to write test-summary.json");
    
    print_summary(&summary);
    
    if summary.failed > 0 {
        panic!("{}{} test case(s) failed{}", RED, summary.failed, RESET);
    }
}

async fn verify_database_setup(pool: &PgPool) {
    println!("{}🔍 Verifying database setup (production migration)...{}", CYAN, RESET);
    
    let course_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM courses")
        .fetch_one(pool)
        .await
        .expect("Failed to count courses");
    
    if course_count == 0 {
        println!("{}❌ FATAL: No courses found in database!{}", RED, RESET);
        println!("{}   Your production migration should have inserted courses.{}", RED, RESET);
        panic!("Database not properly initialized - run migrations first!");
    }
    
    println!("  {}✅ Found {} courses (seeded by production migration){}", GREEN, course_count, RESET);
    
    // Show which courses exist for debugging
    let courses: Vec<(String, Option<Vec<String>>)> = sqlx::query_as(
        "SELECT name, aliases FROM courses ORDER BY name"
    )
    .fetch_all(pool)
    .await
    .expect("Failed to fetch courses");
    
    println!("  {}📚 Available courses:{}", DIM, RESET);
    for (name, aliases) in courses {
        let alias_str = aliases
            .map(|a| format!(" (aliases: {})", a.join(", ")))
            .unwrap_or_default();
        println!("     • {}{}{}", DIM, name, alias_str);
    }
    println!("{}  {}", RESET, DIM);
    println!();
}

async fn run_single_test_with_retry(
    pool: &PgPool, 
    test_case: &TestCase,
    max_retries: u32,
    retry_delay: u64,
) -> TestResult {
    let mut retry_count = 0;
    
    loop {
        let result = run_single_test(pool, test_case).await;
        
        // Check if it's a rate limit error
        let is_rate_limit = result.error.as_ref()
            .map(|e| {
                e.contains("rate limit") || 
                e.contains("429") || 
                e.contains("R-LIMIT") ||
                e.contains("rate_limit") ||
                e.contains("All models failed") ||
                e.contains("All Gemini models rate limited") ||
                e.contains("All Groq") && e.contains("rate limited")
            })
            .unwrap_or(false);
        
        if !is_rate_limit || retry_count >= max_retries {
            let mut final_result = result;
            final_result.retry_count = retry_count;
            return final_result;
        }
        
        // Rate limited - wait and retry
        retry_count += 1;
        println!("   {}⏳ Rate limit detected, retrying in {}s... (attempt {}/{}){}", 
            YELLOW, retry_delay, retry_count, max_retries, RESET);
        tokio::time::sleep(tokio::time::Duration::from_secs(retry_delay)).await;
    }
}

fn print_header(title: &str) {
    println!("\n{}╔══════════════════════════════════════════════════════════════════╗{}", 
        CYAN, RESET);
    println!("{}║{} {:^64} {}║{}", 
        CYAN, RESET, title, CYAN, RESET);
    println!("{}╚══════════════════════════════════════════════════════════════════╝{}\n", 
        CYAN, RESET);
}

#[allow(non_snake_case)]
fn format_key_status(key: &Option<String>) -> String {
    match key {
        Some(k) => {
            let preview: String = k.chars().take(8).collect();
            format!("{}✅ Set{} {}({}...){}", GREEN, RESET, DIM, preview, RESET)
        }
        None => format!("{}⚠️  Not set{}", YELLOW, RESET),
    }
}

fn generate_summary(results: &[TestResult]) -> TestSummary {
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total - passed;
    let success_rate = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    
    let total_retries: u32 = results.iter().map(|r| r.retry_count).sum();
    
    let mut by_category: HashMap<String, CategoryStats> = HashMap::new();
    for result in results {
        let cat = result.category.clone().unwrap_or_else(|| "uncategorized".to_string());
        let stats = by_category.entry(cat).or_insert(CategoryStats {
            total: 0,
            passed: 0,
            failed: 0,
            success_rate: 0.0,
        });
        
        stats.total += 1;
        if result.passed {
            stats.passed += 1;
        } else {
            stats.failed += 1;
        }
    }
    
    for stats in by_category.values_mut() {
        stats.success_rate = if stats.total > 0 {
            (stats.passed as f64 / stats.total as f64) * 100.0
        } else {
            0.0
        };
    }
    
    let failures: Vec<FailureDetail> = results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| {
            FailureDetail {
                name: r.name.clone(),
                category: r.category.clone().unwrap_or_else(|| "uncategorized".to_string()),
                expected: r.expected.clone(),
                actual: r.actual.clone(),
                message_preview: r.message_preview.clone().unwrap_or_default(),
                error: r.error.clone(),
            }
        })
        .collect();
    
    TestSummary {
        total,
        passed,
        failed,
        success_rate,
        total_retries,
        by_category,
        failures,
    }
}

fn print_summary(summary: &TestSummary) {
    let (status_emoji, status_color) = if summary.failed == 0 {
        ("✅", GREEN)
    } else if summary.failed < 5 {
        ("⚠️", YELLOW)
    } else {
        ("❌", RED)
    };
    
    println!("\n{}╔══════════════════════════════════════════════════════════════════╗{}", 
        CYAN, RESET);
    println!("{}║{} {:^64} {}║{}", 
        CYAN, RESET, "📊 TEST SUMMARY", CYAN, RESET);
    println!("{}╠══════════════════════════════════════════════════════════════════╣{}", 
        CYAN, RESET);
    
    println!("{}║{} {}{} Overall Status:{} {:<44} {}║{}", 
        CYAN, RESET, BOLD, status_color, RESET, 
        format!("{} {}/{} tests passed", 
            status_emoji, summary.passed, summary.total), 
        CYAN, RESET);
    println!("{}║{} {}📈 Success Rate:{} {:<45} {}║{}", 
        CYAN, RESET, BOLD, RESET, 
        format!("{:.1}%", summary.success_rate), 
        CYAN, RESET);
    println!("{}║{} {}✅ Passed:{} {:<52} {}║{}", 
        CYAN, RESET, BOLD, RESET, summary.passed, CYAN, RESET);
    println!("{}║{} {}❌ Failed:{} {:<52} {}║{}", 
        CYAN, RESET, BOLD, RESET, summary.failed, CYAN, RESET);
    
    if summary.total_retries > 0 {
        println!("{}║{} {}🔄 Total Retries:{} {:<46} {}║{}", 
            CYAN, RESET, BOLD, RESET, summary.total_retries, CYAN, RESET);
    }
    
    println!("{}╚══════════════════════════════════════════════════════════════════╝{}", 
        CYAN, RESET);
    
    if !summary.by_category.is_empty() {
        println!("\n{}╔══════════════════════════════════════════════════════════════════╗{}", 
            CYAN, RESET);
        println!("{}║{} {:^64} {}║{}", 
            CYAN, RESET, "📂 RESULTS BY CATEGORY", CYAN, RESET);
        println!("{}╠══════════════════════════════════════════════════════════════════╣{}", 
            CYAN, RESET);
        
        let mut categories: Vec<_> = summary.by_category.iter().collect();
        categories.sort_by_key(|(name, _)| *name);
        
        for (category, stats) in categories {
            let cat_color = if stats.failed == 0 { GREEN } else { YELLOW };
            let rate_str = format!("{:.0}%", stats.success_rate);
            
            println!("{}║{} {}{:<30}{} {:>3}/{:<3} {}{:>6}{} {}║{}", 
                CYAN, RESET,
                cat_color, category, RESET,
                stats.passed, stats.total,
                DIM, rate_str, RESET,
                CYAN, RESET);
        }
        println!("{}╚══════════════════════════════════════════════════════════════════╝{}", 
            CYAN, RESET);
    }
    
    if !summary.failures.is_empty() {
        println!("\n{}╔══════════════════════════════════════════════════════════════════╗{}", 
            RED, RESET);
        println!("{}║{} {:^64} {}║{}", 
            RED, RESET, 
            format!("❌ FAILED TESTS ({} failures)", summary.failures.len()), 
            RED, RESET);
        println!("{}╠══════════════════════════════════════════════════════════════════╣{}", 
            RED, RESET);
        
        for (i, failure) in summary.failures.iter().enumerate() {
            println!("{}║{}                                                                {}║{}", 
                RED, RESET, RED, RESET);
            println!("{}║{} {}{}. {}{} {}[{}]{}", 
                RED, RESET, BOLD, i + 1, failure.name, RESET, 
                DIM, failure.category, RESET);
            
            println!("{}║{}    {}Expected:{} {}{}{}", 
                RED, RESET, YELLOW, RESET, CYAN, failure.expected, RESET);
            println!("{}║{}    {}Got:{}      {}{}{}", 
                RED, RESET, YELLOW, RESET, MAGENTA, failure.actual, RESET);
            
            if let Some(ref err) = failure.error {
                let short_err = if err.len() > 50 {
                    format!("{}...", &err[..50])
                } else {
                    err.clone()
                };
                println!("{}║{}    {}Error:{} {}{}{}", 
                    RED, RESET, YELLOW, RESET, DIM, short_err, RESET);
            }
            
            let preview = if failure.message_preview.len() > 50 {
                format!("{}...", &failure.message_preview.chars().take(50).collect::<String>())
            } else {
                failure.message_preview.clone()
            };
            println!("{}║{}    {}Message:{} {}{}{}", 
                RED, RESET, YELLOW, RESET, DIM, preview, RESET);
        }
        
        println!("{}║{}                                                                {}║{}", 
            RED, RESET, RED, RESET);
        println!("{}╚══════════════════════════════════════════════════════════════════╝{}", 
            RED, RESET);
    } else {
        println!("\n{}╔══════════════════════════════════════════════════════════════════╗{}", 
            GREEN, RESET);
        println!("{}║{} {:^64} {}║{}", 
            GREEN, RESET, "🎉 PERFECT SCORE!", GREEN, RESET);
        println!("{}║{} {:^64} {}║{}", 
            GREEN, RESET, "All tests passed successfully!", GREEN, RESET);
        if summary.total_retries > 0 {
            println!("{}║{} {:^64} {}║{}", 
                GREEN, RESET, 
                format!("(with {} automatic retries on rate limits)", summary.total_retries), 
                GREEN, RESET);
        }
        println!("{}╚══════════════════════════════════════════════════════════════════╝{}", 
            GREEN, RESET);
    }
    
    println!();
}

// 🔥 NEW HELPER FUNCTION: Save classifications to database
async fn save_classification_to_db(
    pool: &PgPool,
    classification: AIClassification,
    message_body: &str,
) {
    use marbot::models::NewAssignment;
    
    match classification {
        AIClassification::AssignmentInfo { course_name, title, deadline, description, parallel_codes, .. } => {
            if let Some(ref cname) = course_name {
                if let Ok(Some(course)) = crud::get_course_by_name(pool, cname).await {
                    let deadline_parsed = deadline.as_ref()
                        .and_then(|d| crud::parse_deadline(d).ok());
                    
                    let new_assignment = NewAssignment {
                        course_id: Some(course.id),
                        title: title.clone(),
                        description: description.unwrap_or_else(|| "Test assignment".to_string()),
                        deadline: deadline_parsed,
                        parallel_codes: parallel_codes.clone(),
                        sender_id: Some("test_user_github_actions".to_string()),
                        message_id: format!("test_msg_{}", uuid::Uuid::new_v4()),
                        relating_messages: vec![message_body.to_string()],
                    };
                    
                    let _ = crud::create_assignment(pool, new_assignment).await;
                }
            }
        }
        
        AIClassification::MultipleAssignments { assignments, .. } => {
            for assignment in assignments {
                if let Ok(Some(course)) = crud::get_course_by_name(pool, &assignment.course_name).await {
                    let deadline_parsed = assignment.deadline.as_ref()
                        .and_then(|d| crud::parse_deadline(d).ok());
                    
                    let new_assignment = NewAssignment {
                        course_id: Some(course.id),
                        title: assignment.title.clone(),
                        description: assignment.description.unwrap_or_else(|| "Test assignment".to_string()),
                        deadline: deadline_parsed,
                        parallel_codes: assignment.parallel_codes.clone(),
                        sender_id: Some("test_user_github_actions".to_string()),
                        message_id: format!("test_msg_{}", uuid::Uuid::new_v4()),
                        relating_messages: vec![message_body.to_string()],
                    };
                    
                    let _ = crud::create_assignment(pool, new_assignment).await;
                }
            }
        }
        
        AIClassification::AssignmentUpdate { reference_keywords, new_deadline, new_title, new_description, parallel_codes, .. } => {
            // For updates, try to find and update existing assignment
            if let Some(course_name) = reference_keywords.first() {
                if let Ok(Some(course)) = crud::get_course_by_name(pool, course_name).await {
                    let active_assignments = crud::get_recent_assignments_for_update(pool).await.unwrap_or_default();
                    
                    // Find matching assignment
                    let matching = active_assignments.iter()
                        .find(|a| {
                            a.course_id == Some(course.id) &&
                            reference_keywords.iter().any(|kw| 
                                a.title.to_lowercase().contains(&kw.to_lowercase())
                            )
                        });
                    
                    if let Some(assignment) = matching {
                        let deadline_parsed = new_deadline.as_ref()
                            .and_then(|d| crud::parse_deadline(d).ok());
                        
                        let _ = crud::update_assignment_fields(
                            pool,
                            assignment.id,
                            deadline_parsed,
                            new_title,
                            new_description,
                            if parallel_codes.is_empty() { None } else { Some(parallel_codes) },
                            Some(format!("test_msg_{}", uuid::Uuid::new_v4())),
                            Some(message_body.to_string()),
                        ).await;
                    }
                }
            }
        }
        
        AIClassification::Unrecognized { .. } => {
            // Don't save unrecognized messages
        }
    }
}

async fn run_single_test(pool: &PgPool, test_case: &TestCase) -> TestResult {
    let gemini_available = std::env::var("GEMINI_API_KEY").is_ok();
    let groq_available = std::env::var("GROQ_API_KEY").is_ok();
    
    if !gemini_available && !groq_available {
        return TestResult {
            name: test_case.name.clone(),
            passed: false,
            expected: test_case.expected_type.clone(),
            actual: "error".to_string(),
            duration_ms: 0,
            category: test_case.category.clone(),
            error: Some("No API keys available".to_string()),
            message_preview: Some(test_case.message.chars().take(100).collect()),
            retry_count: 0,
        };
    }
    
    let courses_list = crud::get_all_courses_formatted(pool).await.unwrap_or_default();
    let assignments = crud::get_assignments(pool).await.unwrap_or_default();
    
    let course_map: HashMap<uuid::Uuid, String> = sqlx::query_as::<_, (uuid::Uuid, String)>(
        "SELECT id, name FROM courses"
    )
    .fetch_all(pool)
    .await
    .map(|rows| rows.into_iter().collect())
    .unwrap_or_default();
    
    match extract_with_ai(
        &test_case.message,
        &courses_list,
        &assignments,
        &course_map,
        None,
        "test_user_github_actions",
        pool,
        test_case.quoted_message.as_deref(),
        None,
    ).await {
        Ok(classification) => {
            let actual_type = match &classification {
                AIClassification::AssignmentInfo { .. } => "assignment_info",
                AIClassification::AssignmentUpdate { .. } => "assignment_update",
                AIClassification::MultipleAssignments { .. } => "multiple_assignments",
                AIClassification::Unrecognized { .. } => "unrecognized",
            };
            
            // 🔥 CRITICAL FIX: Save to database after successful classification
            if actual_type != "unrecognized" {
                save_classification_to_db(pool, classification.clone(), &test_case.message,).await;
            }
            
            TestResult {
                name: test_case.name.clone(),
                passed: actual_type == test_case.expected_type,
                expected: test_case.expected_type.clone(),
                actual: actual_type.to_string(),
                duration_ms: 0,
                category: test_case.category.clone(),
                error: None,
                message_preview: Some(test_case.message.chars().take(100).collect()),
                retry_count: 0,
            }
        }
        Err(e) => {
            TestResult {
                name: test_case.name.clone(),
                passed: false,
                expected: test_case.expected_type.clone(),
                actual: "error".to_string(),
                duration_ms: 0,
                category: test_case.category.clone(),
                error: Some(e),
                message_preview: Some(test_case.message.chars().take(100).collect()),
                retry_count: 0,
            }
        }
    }
}