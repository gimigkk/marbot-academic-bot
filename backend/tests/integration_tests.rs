use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::Semaphore;

use marbot::models::AIClassification;
use marbot::parser::ai_extractor::extract_with_ai;
use marbot::database::crud;

// ANSI color codes for pretty terminal output
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
    
    // NEW: Deep validation fields
    #[serde(default)]
    expected_course: Option<String>,
    #[serde(default)]
    expected_title: Option<String>,
    #[serde(default)]
    expected_parallel_codes: Option<Vec<String>>,
    #[serde(default)]
    expected_deadline_present: Option<bool>,
    #[serde(default)]
    expected_count: Option<usize>, // For multiple_assignments
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
    
    // NEW: Detailed validation results
    #[serde(skip_serializing_if = "Option::is_none")]
    validation_details: Option<ValidationDetails>,
}

#[derive(Debug, Serialize, Clone)]
struct ValidationDetails {
    type_match: bool,
    course_match: Option<bool>,
    title_match: Option<bool>,
    parallel_match: Option<bool>,
    deadline_match: Option<bool>,
    count_match: Option<bool>,
    context_used: Option<bool>,
    schedule_hint_found: Option<bool>,
    ai_tier_used: Option<String>, // "gemini", "groq-reasoning", "groq-standard", "groq-vision"
}

#[derive(Debug, Serialize)]
struct TestSummary {
    total: usize,
    passed: usize,
    failed: usize,
    success_rate: f64,
    by_category: HashMap<String, CategoryStats>,
    by_ai_tier: HashMap<String, usize>,
    failures: Vec<FailureDetail>,
    context_usage_stats: ContextUsageStats,
}

#[derive(Debug, Serialize)]
struct CategoryStats {
    total: usize,
    passed: usize,
    failed: usize,
    success_rate: f64,
}

#[derive(Debug, Serialize)]
struct ContextUsageStats {
    total_with_context: usize,
    total_with_schedule: usize,
    total_with_quoted: usize,
}

#[derive(Debug, Serialize)]
struct FailureDetail {
    name: String,
    category: String,
    expected: String,
    actual: String,
    message_preview: String,
    error: Option<String>,
    validation_issues: Vec<String>,
}

#[tokio::test]
async fn run_all_test_cases() {
    // Load test environment
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
        .unwrap_or(3);
    
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
    println!("     • Max retries: {}{}{}", YELLOW, max_retries, RESET);
    println!("     • Retry delay: {}{}s{}", YELLOW, retry_delay, RESET);
    println!("     • {}Deep validation enabled{}", CYAN, RESET);
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
            
            // Print test header
            let progress = format!("[{}/{}]", test_num, total);
            let category_tag = test_case.category.as_ref()
                .map(|c| format!(" {}{}{}", DIM, c, RESET))
                .unwrap_or_default();
            
            println!("{}┌─{}─ {}{:>8}{} {} {}", 
                GRAY, RESET, BLUE, progress, RESET, test_case.name, category_tag);
            
            let start = std::time::Instant::now();
            let result = run_single_test(&pool, &test_case).await;
            let duration = start.elapsed().as_millis() as u64;
            
            let mut test_result = result;
            test_result.duration_ms = duration;
            
            // Print detailed result
            if test_result.passed {
                println!("{}└─{}─ {}✅ PASS{} {}({}ms){}", 
                    GRAY, RESET, GREEN, RESET, DIM, duration, RESET);
                
                // Show validation details for passed tests
                if let Some(ref details) = test_result.validation_details {
                    if let Some(ref tier) = details.ai_tier_used {
                        println!("   {}AI Tier:{} {}{}{}", 
                            DIM, RESET, CYAN, tier, RESET);
                    }
                    if details.context_used == Some(true) {
                        println!("   {}Context:{} {}✓ Used{}", 
                            DIM, RESET, GREEN, RESET);
                    }
                    if details.schedule_hint_found == Some(true) {
                        println!("   {}Schedule:{} {}✓ Found{}", 
                            DIM, RESET, GREEN, RESET);
                    }
                }
            } else {
                println!("{}└─{}─ {}❌ FAIL{}", GRAY, RESET, RED, RESET);
                println!("   {}Expected:{} {}{}{}", 
                    YELLOW, RESET, CYAN, test_result.expected, RESET);
                println!("   {}Got:{}      {}{}{}", 
                    YELLOW, RESET, MAGENTA, test_result.actual, RESET);
                
                // Show validation failures
                if let Some(ref details) = test_result.validation_details {
                    if details.course_match == Some(false) {
                        println!("   {}⚠️  Course mismatch{}", YELLOW, RESET);
                    }
                    if details.title_match == Some(false) {
                        println!("   {}⚠️  Title mismatch{}", YELLOW, RESET);
                    }
                    if details.parallel_match == Some(false) {
                        println!("   {}⚠️  Parallel codes mismatch{}", YELLOW, RESET);
                    }
                    if details.deadline_match == Some(false) {
                        println!("   {}⚠️  Deadline presence mismatch{}", YELLOW, RESET);
                    }
                    if details.count_match == Some(false) {
                        println!("   {}⚠️  Assignment count mismatch{}", YELLOW, RESET);
                    }
                    if let Some(ref tier) = details.ai_tier_used {
                        println!("   {}AI Tier:{} {}{}{}", 
                            DIM, RESET, YELLOW, tier, RESET);
                    }
                }
                
                if let Some(ref err) = test_result.error {
                    let short_err = if err.len() > 100 {
                        format!("{}...", &err[..100])
                    } else {
                        err.clone()
                    };
                    println!("   {}Error:{} {}{}{}", YELLOW, RESET, RED, short_err, RESET);
                }
                
                // Show message preview for failed tests
                let preview = test_case.message.chars().take(80).collect::<String>();
                println!("   {}Message:{} {}{}{}", 
                    YELLOW, RESET, DIM, preview, RESET);
            }
            println!();
            
            // Delay to respect rate limits
            let delay_secs = std::env::var("TEST_DELAY_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(3);
            
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
    
    // Sort by test name for consistent output
    results.sort_by(|a, b| a.name.cmp(&b.name));
    
    // Generate summary
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
    
    // Print detailed summary
    print_summary(&summary);
    
    if summary.failed > 0 {
        panic!("{}{} test case(s) failed{}", RED, summary.failed, RESET);
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
    
    // Group by category
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
    
    // Calculate success rates
    for stats in by_category.values_mut() {
        stats.success_rate = if stats.total > 0 {
            (stats.passed as f64 / stats.total as f64) * 100.0
        } else {
            0.0
        };
    }
    
    // Track AI tier usage
    let mut by_ai_tier: HashMap<String, usize> = HashMap::new();
    for result in results {
        if let Some(ref details) = result.validation_details {
            if let Some(ref tier) = details.ai_tier_used {
                *by_ai_tier.entry(tier.clone()).or_insert(0) += 1;
            }
        }
    }
    
    // Context usage stats
    let total_with_context = results.iter()
        .filter(|r| r.validation_details.as_ref()
            .and_then(|d| d.context_used) == Some(true))
        .count();
    
    let total_with_schedule = results.iter()
        .filter(|r| r.validation_details.as_ref()
            .and_then(|d| d.schedule_hint_found) == Some(true))
        .count();
    
    let total_with_quoted = results.iter()
        .filter(|r| r.message_preview.as_ref()
            .map(|m| m.contains("Quoted")) == Some(true))
        .count();
    
    // Collect failure details with validation issues
    let failures: Vec<FailureDetail> = results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| {
            let mut validation_issues = Vec::new();
            
            if let Some(ref details) = r.validation_details {
                if details.course_match == Some(false) {
                    validation_issues.push("Course mismatch".to_string());
                }
                if details.title_match == Some(false) {
                    validation_issues.push("Title mismatch".to_string());
                }
                if details.parallel_match == Some(false) {
                    validation_issues.push("Parallel codes mismatch".to_string());
                }
                if details.deadline_match == Some(false) {
                    validation_issues.push("Deadline presence mismatch".to_string());
                }
                if details.count_match == Some(false) {
                    validation_issues.push("Assignment count mismatch".to_string());
                }
            }
            
            FailureDetail {
                name: r.name.clone(),
                category: r.category.clone().unwrap_or_else(|| "uncategorized".to_string()),
                expected: r.expected.clone(),
                actual: r.actual.clone(),
                message_preview: r.message_preview.clone().unwrap_or_default(),
                error: r.error.clone(),
                validation_issues,
            }
        })
        .collect();
    
    TestSummary {
        total,
        passed,
        failed,
        success_rate,
        by_category,
        by_ai_tier,
        failures,
        context_usage_stats: ContextUsageStats {
            total_with_context,
            total_with_schedule,
            total_with_quoted,
        },
    }
}

fn print_summary(summary: &TestSummary) {
    // Determine status
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
        CYAN, RESET, "📊 COMPREHENSIVE TEST SUMMARY", CYAN, RESET);
    println!("{}╠══════════════════════════════════════════════════════════════════╣{}", 
        CYAN, RESET);
    
    // Overall stats
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
    println!("{}╚══════════════════════════════════════════════════════════════════╝{}", 
        CYAN, RESET);
    
    // AI Tier Usage
    if !summary.by_ai_tier.is_empty() {
        println!("\n{}╔══════════════════════════════════════════════════════════════════╗{}", 
            CYAN, RESET);
        println!("{}║{} {:^64} {}║{}", 
            CYAN, RESET, "🤖 AI TIER USAGE", CYAN, RESET);
        println!("{}╠══════════════════════════════════════════════════════════════════╣{}", 
            CYAN, RESET);
        
        for (tier, count) in &summary.by_ai_tier {
            let percentage = (*count as f64 / summary.total as f64) * 100.0;
            println!("{}║{} {:<30} {:>3} tests ({:>5.1}%)              {}║{}", 
                CYAN, RESET, tier, count, percentage, CYAN, RESET);
        }
        println!("{}╚══════════════════════════════════════════════════════════════════╝{}", 
            CYAN, RESET);
    }
    
    // Context Usage Stats
    println!("\n{}╔══════════════════════════════════════════════════════════════════╗{}", 
        CYAN, RESET);
    println!("{}║{} {:^64} {}║{}", 
        CYAN, RESET, "🧠 CONTEXT BUILDER USAGE", CYAN, RESET);
    println!("{}╠══════════════════════════════════════════════════════════════════╣{}", 
        CYAN, RESET);
    println!("{}║{} Context Used:      {:>3} tests                                    {}║{}", 
        CYAN, RESET, summary.context_usage_stats.total_with_context, CYAN, RESET);
    println!("{}║{} Schedule Hints:    {:>3} tests                                    {}║{}", 
        CYAN, RESET, summary.context_usage_stats.total_with_schedule, CYAN, RESET);
    println!("{}║{} Quoted Messages:   {:>3} tests                                    {}║{}", 
        CYAN, RESET, summary.context_usage_stats.total_with_quoted, CYAN, RESET);
    println!("{}╚══════════════════════════════════════════════════════════════════╝{}", 
        CYAN, RESET);
    
    // Category breakdown
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
    
    // Failed tests details
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
            
            // Show validation issues
            if !failure.validation_issues.is_empty() {
                println!("{}║{}    {}Issues:{}", 
                    RED, RESET, YELLOW, RESET);
                for issue in &failure.validation_issues {
                    println!("{}║{}      - {}{}{}", 
                        RED, RESET, YELLOW, issue, RESET);
                }
            }
            
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
        println!("{}║{} {:^64} {}║{}", 
            GREEN, RESET, "Your overengineered system works perfectly!", GREEN, RESET);
        println!("{}╚══════════════════════════════════════════════════════════════════╝{}", 
            GREEN, RESET);
    }
    
    println!();
}

async fn run_single_test(pool: &PgPool, test_case: &TestCase) -> TestResult {
    // Validate API keys
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
            validation_details: None,
        };
    }
    
    // Get database data
    let courses_list = match crud::get_all_courses_formatted(pool).await {
        Ok(list) => list,
        Err(e) => {
            return TestResult {
                name: test_case.name.clone(),
                passed: false,
                expected: test_case.expected_type.clone(),
                actual: "error".to_string(),
                duration_ms: 0,
                category: test_case.category.clone(),
                error: Some(format!("Database error: {}", e)),
                message_preview: Some(test_case.message.chars().take(100).collect()),
                validation_details: None,
            };
        }
    };
    
    let assignments = crud::get_assignments(pool).await.unwrap_or_default();
    
    let course_map: HashMap<uuid::Uuid, String> = sqlx::query_as::<_, (uuid::Uuid, String)>(
        "SELECT id, name FROM courses"
    )
    .fetch_all(pool)
    .await
    .map(|rows| rows.into_iter().collect())
    .unwrap_or_default();
    
    // Retry logic for rate limiting
    let max_retries = std::env::var("TEST_MAX_RETRIES")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(5);
    
    let retry_delay_secs = std::env::var("TEST_RETRY_DELAY_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    
    let mut attempt = 0;
    let result = loop {
        attempt += 1;
        
        let result = extract_with_ai(
            &test_case.message,
            &courses_list,
            &assignments,
            &course_map,
            None,
            "test_user_github_actions",
            pool,
            test_case.quoted_message.as_deref(),
            None,
        ).await;
        
        match result {
            Ok(classification) => break Ok(classification),
            Err(e) => {
                let is_rate_limit = e.contains("rate limit") 
                    || e.contains("All models failed")
                    || e.contains("429");
                
                if is_rate_limit && attempt < max_retries {
                    println!("   {}⏳ Rate limited, retry {}/{} in {}s{}", 
                        YELLOW, attempt, max_retries, retry_delay_secs, RESET);
                    tokio::time::sleep(tokio::time::Duration::from_secs(retry_delay_secs)).await;
                    continue;
                } else if is_rate_limit {
                    break Err(format!("Rate limit: Max retries ({}) exceeded", max_retries));
                } else {
                    break Err(e);
                }
            }
        }
    };
    
    // Process result with deep validation
    match result {
        Ok(classification) => {
            let actual_type = match classification {
                AIClassification::AssignmentInfo { .. } => "assignment_info",
                AIClassification::AssignmentUpdate { .. } => "assignment_update",
                AIClassification::MultipleAssignments { .. } => "multiple_assignments",
                AIClassification::Unrecognized { .. }=> "unrecognized",
            };
            
            // DEEP VALIDATION
            let mut validation_details = ValidationDetails {
                type_match: actual_type == test_case.expected_type,
                course_match: None,
                title_match: None,
                parallel_match: None,
                deadline_match: None,
                count_match: None,
                context_used: None, // TODO: Capture from extract_with_ai
                schedule_hint_found: None, // TODO: Capture from context builder
                ai_tier_used: None, // TODO: Capture which tier was used
            };
            
            // Validate specific fields based on classification type
            match &classification {
                AIClassification::AssignmentInfo { course_name, title, deadline, parallel_codes, .. } => {
                    if let Some(ref expected_course) = test_case.expected_course {
                        validation_details.course_match = Some(
                            course_name.as_ref().map(|c| c.eq_ignore_ascii_case(expected_course)).unwrap_or(false)
                        );
                    }
                    
                    if let Some(ref expected_title) = test_case.expected_title {
                        validation_details.title_match = Some(title.contains(expected_title));
                    }
                    
                    if let Some(ref expected_parallels) = test_case.expected_parallel_codes {
                        let matches = expected_parallels.iter()
                            .all(|exp| parallel_codes.iter().any(|act| act.eq_ignore_ascii_case(exp)));
                        validation_details.parallel_match = Some(matches);
                    }
                    
                    if let Some(expected_deadline_present) = test_case.expected_deadline_present {
                        validation_details.deadline_match = Some(
                            deadline.is_some() == expected_deadline_present
                        );
                    }
                }
                AIClassification::MultipleAssignments { assignments, .. } => {
                    if let Some(expected_count) = test_case.expected_count {
                        validation_details.count_match = Some(assignments.len() == expected_count);
                    }
                }
                _ => {}
            }
            
            // Overall pass = type match + all enabled validations pass
            let all_validations_pass = [
                validation_details.course_match,
                validation_details.title_match,
                validation_details.parallel_match,
                validation_details.deadline_match,
                validation_details.count_match,
            ].iter().all(|v| v.is_none() || *v == Some(true));
            
            let passed = validation_details.type_match && all_validations_pass;
            
            TestResult {
                name: test_case.name.clone(),
                passed,
                expected: test_case.expected_type.clone(),
                actual: actual_type.to_string(),
                duration_ms: 0,
                category: test_case.category.clone(),
                error: None,
                message_preview: Some(test_case.message.chars().take(100).collect()),
                validation_details: Some(validation_details),
            }
        }
        Err(e) => {
            let error_type = if e.contains("rate limit") {
                "Rate Limit"
            } else if e.contains("API key") {
                "API Key"
            } else if e.contains("All models failed") {
                "All Models Failed"
            } else if e.contains("JSON") {
                "Parse Error"
            } else {
                "Network Error"
            };
            
            TestResult {
                name: test_case.name.clone(),
                passed: false,
                expected: test_case.expected_type.clone(),
                actual: "error".to_string(),
                duration_ms: 0,
                category: test_case.category.clone(),
                error: Some(format!("{}: {}", error_type, e)),
                message_preview: Some(test_case.message.chars().take(100).collect()),
                validation_details: None,
            }
        }
    }
}