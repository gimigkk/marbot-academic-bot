use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::Semaphore;

use marbot::models::AIClassification;
use marbot::parser::ai_extractor::extract_with_ai;
use marbot::database::crud;

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
}

#[derive(Debug, Serialize)]
struct TestSummary {
    total: usize,
    passed: usize,
    failed: usize,
    success_rate: f64,
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
    // Load test environment
    if std::path::Path::new(".env.test").exists() {
        dotenv::from_filename(".env.test").ok();
    } else {
        dotenv::dotenv().ok();
    }
    
    // Validate environment before starting tests
    println!("\n╔══════════════════════════════════════════════╗");
    println!("║  🔍 ENVIRONMENT CHECK");
    println!("╠══════════════════════════════════════════════╣");
    
    let gemini_key = std::env::var("GEMINI_API_KEY").ok();
    let groq_key = std::env::var("GROQ_API_KEY").ok();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    
    println!("║  GEMINI_API_KEY  : {}", if gemini_key.is_some() { 
        format!("✅ Set ({}...)", &gemini_key.as_ref().unwrap().chars().take(8).collect::<String>())
    } else { 
        "❌ Missing".to_string() 
    });
    
    println!("║  GROQ_API_KEY    : {}", if groq_key.is_some() { 
        format!("✅ Set ({}...)", &groq_key.as_ref().unwrap().chars().take(8).collect::<String>())
    } else { 
        "❌ Missing".to_string() 
    });
    
    println!("║  DATABASE_URL    : ✅ Set");
    println!("╚══════════════════════════════════════════════╝\n");
    
    if gemini_key.is_none() && groq_key.is_none() {
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
    
    println!("╔══════════════════════════════════════════════╗");
    println!("║  🧪 Running {} test cases", limit);
    println!("╚══════════════════════════════════════════════╝\n");
    
    // Run tests sequentially in CI (parallel=1), locally can be higher
    let concurrency = std::env::var("TEST_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);
    
    println!("⚙️  Concurrency: {} (set TEST_CONCURRENCY to override)\n", concurrency);
    
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let pool = Arc::new(pool);
    
    let mut handles = vec![];
    
    for (i, test_case) in test_cases.iter().take(limit).enumerate() {
        let sem = semaphore.clone();
        let pool = pool.clone();
        let test_case = test_case.clone();
        let test_num = i + 1;
        let total = limit;
        
        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            
            println!("┌─ [{}/{}] {}", test_num, total, test_case.name);
            
            let start = std::time::Instant::now();
            let result = run_single_test(&pool, &test_case).await;
            let duration = start.elapsed().as_millis() as u64;
            
            let mut test_result = result;
            test_result.duration_ms = duration;
            
            if test_result.passed {
                println!("└─ ✅ PASS ({} ms)\n", duration);
            } else {
                println!("└─ ❌ FAIL: Expected '{}', got '{}' ({} ms)", 
                    test_result.expected, test_result.actual, duration);
                if let Some(ref err) = test_result.error {
                    println!("   Error: {}\n", err);
                }
            }
            
            // Delay to respect rate limits (only if running multiple tests)
            if concurrency > 1 {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
            
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
        panic!("{} test case(s) failed", summary.failed);
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
    
    // Collect failure details
    let failures: Vec<FailureDetail> = results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| FailureDetail {
            name: r.name.clone(),
            category: r.category.clone().unwrap_or_else(|| "uncategorized".to_string()),
            expected: r.expected.clone(),
            actual: r.actual.clone(),
            message_preview: r.message_preview.clone().unwrap_or_default(),
            error: r.error.clone(),
        })
        .collect();
    
    TestSummary {
        total,
        passed,
        failed,
        success_rate,
        by_category,
        failures,
    }
}

fn print_summary(summary: &TestSummary) {
    println!("\n╔══════════════════════════════════════════════╗");
    println!("║  📊 TEST SUMMARY");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  ✅ Passed: {:<33} ║", summary.passed);
    println!("║  ❌ Failed: {:<33} ║", summary.failed);
    println!("║  📈 Success Rate: {:<26.1}% ║", summary.success_rate);
    println!("╚══════════════════════════════════════════════╝");
    
    if !summary.by_category.is_empty() {
        println!("\n╔══════════════════════════════════════════════╗");
        println!("║  📂 BY CATEGORY");
        println!("╠══════════════════════════════════════════════╣");
        
        let mut categories: Vec<_> = summary.by_category.iter().collect();
        categories.sort_by_key(|(name, _)| *name);
        
        for (category, stats) in categories {
            println!("║  {}: {}/{} ({:.0}%)", 
                category, stats.passed, stats.total, stats.success_rate);
        }
        println!("╚══════════════════════════════════════════════╝");
    }
    
    if !summary.failures.is_empty() {
        println!("\n╔══════════════════════════════════════════════╗");
        println!("║  ❌ FAILED TESTS ({} failures)", summary.failures.len());
        println!("╠══════════════════════════════════════════════╣");
        
        for (i, failure) in summary.failures.iter().enumerate() {
            println!("║  {}. {} [{}]", i + 1, failure.name, failure.category);
            println!("║     Expected: {}", failure.expected);
            println!("║     Got: {}", failure.actual);
            if let Some(ref err) = failure.error {
                println!("║     Error: {}", err);
            }
            let preview = if failure.message_preview.len() > 60 {
                format!("{}...", &failure.message_preview[..60])
            } else {
                failure.message_preview.clone()
            };
            println!("║     Message: {}", preview);
            println!("║");
        }
        println!("╚══════════════════════════════════════════════╝\n");
    }
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
            error: Some("No API keys available (GEMINI_API_KEY and GROQ_API_KEY both missing)".to_string()),
            message_preview: Some(test_case.message.chars().take(100).collect()),
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
                error: Some(format!("Database error - Failed to get courses: {}", e)),
                message_preview: Some(test_case.message.chars().take(100).collect()),
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
    
    // Run AI extraction
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
    
    // Process result
    match result {
        Ok(classification) => {
            let actual_type = match classification {
                AIClassification::AssignmentInfo { .. } => "assignment_info",
                AIClassification::AssignmentUpdate { .. } => "assignment_update",
                AIClassification::MultipleAssignments { .. } => "multiple_assignments",
                AIClassification::Unrecognized => "unrecognized",
            };
            
            let passed = actual_type == test_case.expected_type;
            
            // Log additional info for failures
            if !passed {
                eprintln!("   ℹ️  Expected: {}, Got: {}", test_case.expected_type, actual_type);
                match classification {
                    AIClassification::AssignmentInfo { ref title, ref course_name, .. } => {
                        eprintln!("   ℹ️  Detected: {} - {}", 
                            course_name.as_deref().unwrap_or("Unknown"), title);
                    }
                    AIClassification::MultipleAssignments { ref assignments, .. } => {
                        eprintln!("   ℹ️  Detected {} assignments", assignments.len());
                    }
                    _ => {}
                }
            }
            
            TestResult {
                name: test_case.name.clone(),
                passed,
                expected: test_case.expected_type.clone(),
                actual: actual_type.to_string(),
                duration_ms: 0,
                category: test_case.category.clone(),
                error: None,
                message_preview: Some(test_case.message.chars().take(100).collect()),
            }
        }
        Err(e) => {
            // Categorize errors for better debugging
            let error_type = if e.contains("rate limit") {
                "Rate Limit Error"
            } else if e.contains("API key") || e.contains("not set") {
                "API Key Error"
            } else if e.contains("All models failed") {
                "All AI Models Failed"
            } else if e.contains("Failed to deserialize") || e.contains("JSON") {
                "Response Parse Error"
            } else if e.contains("request") || e.contains("REQUEST FAILED") {
                "Network/Request Error"
            } else {
                "Unknown Error"
            };
            
            eprintln!("   ❌ {}: {}", error_type, e);
            
            TestResult {
                name: test_case.name.clone(),
                passed: false,
                expected: test_case.expected_type.clone(),
                actual: "error".to_string(),
                duration_ms: 0,
                category: test_case.category.clone(),
                error: Some(format!("{}: {}", error_type, e)),
                message_preview: Some(test_case.message.chars().take(100).collect()),
            }
        }
    }
}