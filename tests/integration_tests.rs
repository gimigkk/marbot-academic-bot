use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::fs;

// Import from your main crate
use your_crate_name::models::AIClassification;
use your_crate_name::parser::ai_extractor::extract_with_ai;
use your_crate_name::database::crud;

#[derive(Debug, Deserialize)]
struct TestCase {
    name: String,
    message: String,
    expected_type: String,
    #[serde(default)]
    quoted_message: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct TestResult {
    name: String,
    passed: bool,
    expected: String,
    actual: String,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tokio::test]
async fn run_all_test_cases() {
    // Connect to database
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    
    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to database");
    
    // Load test cases
    let test_data = fs::read_to_string("tests/test_cases.json")
        .expect("Failed to read test_cases.json");
    
    let test_cases: Vec<TestCase> = serde_json::from_str(&test_data)
        .expect("Failed to parse test_cases.json");
    
    // Get test limit from env or run all
    let limit = std::env::var("TEST_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(test_cases.len());
    
    println!("\n╔══════════════════════════════════════════════╗");
    println!("║  🧪 Running {} test cases", limit);
    println!("╚══════════════════════════════════════════════╝\n");
    
    let mut results = Vec::new();
    let mut passed = 0;
    let mut failed = 0;
    
    for (i, test_case) in test_cases.iter().take(limit).enumerate() {
        println!("┌─ [{}/{}] {}", i + 1, limit, test_case.name);
        
        let start = std::time::Instant::now();
        let result = run_single_test(&pool, test_case).await;
        let duration = start.elapsed().as_millis() as u64;
        
        let mut test_result = result;
        test_result.duration_ms = duration;
        
        if test_result.passed {
            println!("└─ ✅ PASS ({} ms)\n", duration);
            passed += 1;
        } else {
            println!("└─ ❌ FAIL: Expected '{}', got '{}' ({} ms)", 
                test_result.expected, test_result.actual, duration);
            if let Some(ref err) = test_result.error {
                println!("   Error: {}\n", err);
            }
            failed += 1;
        }
        
        results.push(test_result);
        
        // Rate limit: 2s between calls to avoid API limits
        if i < limit - 1 {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }
    
    // Save results
    let results_json = serde_json::to_string_pretty(&results)
        .expect("Failed to serialize results");
    fs::write("test-results.json", results_json)
        .expect("Failed to write test-results.json");
    
    // Print summary
    println!("╔══════════════════════════════════════════════╗");
    println!("║  📊 TEST SUMMARY");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  ✅ Passed: {:<33} ║", passed);
    println!("║  ❌ Failed: {:<33} ║", failed);
    println!("║  📈 Success Rate: {:<26.1}% ║", 
        (passed as f64 / (passed + failed) as f64) * 100.0);
    println!("╚══════════════════════════════════════════════╝\n");
    
    // Fail if any tests failed
    if failed > 0 {
        panic!("{} test case(s) failed", failed);
    }
}

async fn run_single_test(pool: &PgPool, test_case: &TestCase) -> TestResult {
    // Get context data
    let courses_list = match crud::get_all_courses_formatted(pool).await {
        Ok(list) => list,
        Err(e) => {
            return TestResult {
                name: test_case.name.clone(),
                passed: false,
                expected: test_case.expected_type.clone(),
                actual: "error".to_string(),
                duration_ms: 0,
                error: Some(format!("Failed to get courses: {}", e)),
            };
        }
    };
    
    let assignments = crud::get_assignments(pool).await.unwrap_or_default();
    
    let course_map: HashMap<uuid::Uuid, String> = sqlx::query_as(
        "SELECT id, name FROM courses"
    )
    .fetch_all(pool)
    .await
    .map(|rows| rows.into_iter().collect())
    .unwrap_or_default();
    
    // Call AI extraction
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
        Ok(classification) => {
            let actual_type = match classification {
                AIClassification::AssignmentInfo { .. } => "assignment_info",
                AIClassification::AssignmentUpdate { .. } => "assignment_update",
                AIClassification::MultipleAssignments { .. } => "multiple_assignments",
                AIClassification::Unrecognized => "unrecognized",
            };
            
            TestResult {
                name: test_case.name.clone(),
                passed: actual_type == test_case.expected_type,
                expected: test_case.expected_type.clone(),
                actual: actual_type.to_string(),
                duration_ms: 0,
                error: None,
            }
        }
        Err(e) => TestResult {
            name: test_case.name.clone(),
            passed: false,
            expected: test_case.expected_type.clone(),
            actual: "error".to_string(),
            duration_ms: 0,
            error: Some(e),
        }
    }
}