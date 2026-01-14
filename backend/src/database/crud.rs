use sqlx::{PgPool, Result};
use uuid::Uuid;
use chrono::{DateTime, Utc, FixedOffset, TimeZone, NaiveDateTime};
use std::collections::HashMap;

use crate::models::{Assignment, NewAssignment, Course, AssignmentWithCourse};

// ========================================
// CREATE OPERATIONS
// ========================================

/// Create a new assignment in the database
#[allow(non_snake_case)]
pub async fn create_assignment(
    pool: &PgPool,
    new_assignment: NewAssignment,
) -> Result<String, sqlx::Error> {
    let mut tx = pool.begin().await?;
    
    // A. Find Course
    let course = sqlx::query!(
        r#"
        SELECT id, name 
        FROM courses 
        WHERE id = $1
        LIMIT 1
        "#,
        new_assignment.course_id
    )
    .fetch_optional(&mut *tx)
    .await?;

    // Validate Course
    let real_course_name = match course {
        Some(c) => c.name,
        None => match new_assignment.course_id {
            Some(id) => {
                tx.commit().await?;
                return Ok(format!("Gagal: Mata kuliah dengan ID '{}' tidak ditemukan", id));
            }
            None => {
                tx.commit().await?;
                return Ok("Gagal: Mata kuliah tidak ditemukan (ID tidak ada)".to_string());
            }
        }
    };
    
    // FIXED: NewAssignment already has Vec<String>, just normalize to lowercase
    let clean_parallel_codes: Vec<String> = new_assignment.parallel_codes
        .iter()
        .map(|p| p.to_lowercase())
        .collect();

    // B. Insert Assignment
    sqlx::query(
        r#"
        INSERT INTO assignments (
            course_id, parallel_codes, title, description, 
            deadline, sender_id, message_ids, relating_messages
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#
    )
    .bind(new_assignment.course_id)
    .bind(&clean_parallel_codes)
    .bind(&new_assignment.title)
    .bind(&new_assignment.description)
    .bind(new_assignment.deadline)
    .bind(&new_assignment.sender_id)
    .bind(&vec![new_assignment.message_id.clone()])  // Wrap in Vec
    .bind(&new_assignment.relating_messages)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    
    let parallel_display = if clean_parallel_codes.is_empty() {
        "no specific parallels".to_string()
    } else {
        format!("parallels: [{}]", clean_parallel_codes.join(", "))
    };
    
    Ok(format!("Sukses! Tugas '{}' berhasil disimpan ke matkul '{}' ({})\n", 
        new_assignment.title, real_course_name, parallel_display))
}

// ========================================
// COMPLETION OPERATIONS
// ========================================

/// Mark assignment as complete
pub async fn mark_assignment_complete(
    pool: &PgPool,
    assignment_id: Uuid,
    user_id: &str
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        INSERT INTO user_completions (assignment_id, user_id)
        VALUES ($1, $2)
        ON CONFLICT (user_id, assignment_id) DO NOTHING
        "#,
        assignment_id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Mark assignment as incomplete (Undo)
pub async fn unmark_assignment_complete(
    pool: &PgPool,
    assignment_id: Uuid,
    user_id: &str
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        DELETE FROM user_completions 
        WHERE assignment_id = $1 AND user_id = $2
        "#,
        assignment_id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

// ========================================
// READ OPERATIONS
// ========================================

/// Get the most recently completed assignment for a user
pub async fn get_last_completed_assignment(
    pool: &PgPool,
    user_id: &str
) -> Result<Option<AssignmentWithCourse>, sqlx::Error> {
    let assignment = sqlx::query_as::<_, AssignmentWithCourse>(
        r#"
        SELECT 
            a.id,
            c.name as course_name,
            a.parallel_codes,
            a.title,
            c.aliases[1] as first_alias,
            a.description,  
            a.deadline,
            a.message_ids,
            a.sender_id,
            true as is_completed,
            a.relating_messages
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        JOIN user_completions uc ON uc.assignment_id = a.id
        WHERE uc.user_id = $1
        ORDER BY uc.completed_at DESC
        LIMIT 1
        "#
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    
    Ok(assignment)
}

/// Get recent assignments for duplicate detection (larger limit)
pub async fn get_recent_assignments_for_duplicate_check(
    pool: &PgPool
) -> Result<Vec<Assignment>, sqlx::Error> {
    let assignments = sqlx::query_as::<_, Assignment>(
        r#"
        SELECT 
            id,
            created_at,
            course_id,
            title,
            description,
            deadline,
            parallel_codes,
            sender_id,
            message_ids,
            reminder_1h_sent,
            relating_messages
        FROM assignments
        WHERE created_at > NOW() - INTERVAL '30 days'
        ORDER BY created_at DESC
        LIMIT 100
        "#
    )
    .fetch_all(pool)
    .await?;
    
    println!("🔍 Fetched {} assignments for duplicate check", assignments.len());
    
    Ok(assignments)
}

/// Get recent assignments for update matching (larger limit)
pub async fn get_recent_assignments_for_matching(
    pool: &PgPool
) -> Result<Vec<Assignment>, sqlx::Error> {
    let assignments = sqlx::query_as::<_, Assignment>(
        r#"
        SELECT 
            id,
            created_at,
            course_id,
            title,
            description,
            deadline,
            parallel_codes,
            sender_id,
            message_ids,
            reminder_1h_sent,
            relating_messages
        FROM assignments
        WHERE created_at > NOW() - INTERVAL '30 days'
        ORDER BY created_at DESC
        LIMIT 100
        "#
    )
    .fetch_all(pool)
    .await?;
    
    println!("🔍 Fetched {} assignments for update matching", assignments.len());
    
    Ok(assignments)
}

/// Get assignments for classification context (standard limit of 20)
pub async fn get_assignments_for_classification(pool: &PgPool) -> Result<Vec<Assignment>> {
    println!("🔍 DEBUG: Calling get_assignments_for_classification()...");
    
    let result = sqlx::query_as::<_, Assignment>(
        r#"
        SELECT 
            id,
            created_at,
            course_id,
            title,
            description,
            deadline,
            parallel_codes,
            sender_id,
            message_ids,
            reminder_1h_sent,
            relating_messages
        FROM assignments
        ORDER BY created_at DESC
        LIMIT 15
        "#
    )
    .fetch_all(pool)
    .await;

    match &result {
        Ok(assignments) => {
            println!("✅ DEBUG: Successfully fetched {} assignments for classification", assignments.len());
            if assignments.is_empty() {
                println!("⚠️  DEBUG: No assignments found in database!");
            }
        }
        Err(e) => {
            eprintln!("❌ DEBUG: get_assignments_for_classification() failed: {:?}", e);
        }
    }

    result
}

// Update the original get_assignments to use LIMIT 20 as well
pub async fn get_assignments(pool: &PgPool) -> Result<Vec<Assignment>> {
    println!("🔍 DEBUG: Calling get_assignments()...");
    
    let result = sqlx::query_as::<_, Assignment>(
        r#"
        SELECT 
            id,
            created_at,
            course_id,
            title,
            description,
            deadline,
            parallel_codes,
            sender_id,
            message_ids,
            reminder_1h_sent,
            relating_messages
        FROM assignments
        ORDER BY created_at DESC
        LIMIT 20
        "#
    )
    .fetch_all(pool)
    .await;

    match &result {
        Ok(assignments) => {
            println!("✅ DEBUG: Successfully fetched {} assignments", assignments.len());
            if assignments.is_empty() {
                println!("⚠️  DEBUG: No assignments found in database!");
            }
        }
        Err(e) => {
            eprintln!("❌ DEBUG: get_assignments() failed: {:?}", e);
        }
    }

    result
}

/// Get all courses as a HashMap for AI context
pub async fn get_courses_map(pool: &PgPool) -> Result<HashMap<Uuid, String>, sqlx::Error> {
    let courses = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, name FROM courses"
    )
    .fetch_all(pool)
    .await?;
    
    Ok(courses.into_iter().collect())
}

/// Check if an assignment with this title already exists for a course
pub async fn get_assignment_by_title_and_course(
    pool: &PgPool,
    title: &str,
    course_id: uuid::Uuid,
) -> Result<Option<Assignment>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    
    let result = sqlx::query_as::<_, Assignment>(
        r#"
        SELECT 
            id,
            created_at,
            course_id,
            title,
            description,
            deadline,
            parallel_codes,
            sender_id,
            message_ids,
            reminder_1h_sent,
            relating_messages
        FROM assignments
        WHERE title = $1 AND course_id = $2
        "#
    )
    .bind(title)
    .bind(course_id)
    .fetch_optional(&mut *tx)
    .await?;
    
    tx.commit().await?;
    Ok(result)
}

/// Get active assignments (not past deadline)
pub async fn get_active_assignments(pool: &PgPool) -> Result<Vec<Assignment>> {
    let now = Utc::now();
    
    let assignments = sqlx::query_as::<_, Assignment>(
        r#"
        SELECT 
            id,
            created_at,
            course_id,
            title,
            description,
            deadline,
            parallel_codes,
            sender_id,
            message_ids,
            reminder_1h_sent,
            relating_messages
        FROM assignments
        WHERE deadline > $1 OR deadline IS NULL
        ORDER BY created_at DESC
        LIMIT 100
        "#
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    
    println!("✅ Found {} active assignments", assignments.len());
    
    Ok(assignments)
}

pub async fn get_active_assignments_sorted(pool: &PgPool) -> Result<Vec<AssignmentWithCourse>, sqlx::Error> {
    let now = Utc::now();
    
    let assignments = sqlx::query_as::<_, AssignmentWithCourse>(
        r#"
        SELECT 
            a.id,
            c.name as course_name,
            a.parallel_codes,
            a.title,
            c.aliases[1] as first_alias,
            a.description,  
            a.deadline,
            a.message_ids,
            a.sender_id,
            false as is_completed,
            a.relating_messages
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.deadline IS NULL OR a.deadline >= $1
        ORDER BY 
            CASE WHEN a.deadline IS NULL THEN 0 ELSE 1 END,
            a.deadline ASC NULLS FIRST,
            c.name ASC
        "#
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    
    println!("✅ Found {} active assignments (scheduler)\n", assignments.len());
    
    Ok(assignments)
}

#[allow(non_snake_case)]
pub async fn get_active_assignments_for_user(
    pool: &PgPool, 
    user_id: &str
) -> Result<(Vec<AssignmentWithCourse>, HashMap<String, String>), sqlx::Error> {
    let now = Utc::now();
    
    println!("🔍 Fetching assignments for user: {}", user_id);
    
    let assignments = sqlx::query_as::<_, AssignmentWithCourse>(
        r#"
        SELECT 
            a.id,
            c.name as course_name,
            a.parallel_codes,
            a.title,
            c.aliases[1] as first_alias,
            a.description,  
            a.deadline,
            a.message_ids,
            a.sender_id,
            EXISTS(
                SELECT 1 FROM user_completions uc 
                WHERE uc.assignment_id = a.id 
                AND uc.user_id = $2
            ) as is_completed,
            a.relating_messages
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.deadline IS NULL OR a.deadline >= $1
        ORDER BY 
            CASE WHEN a.deadline IS NULL THEN 0 ELSE 1 END,
            a.deadline ASC NULLS FIRST,
            c.name ASC
        "#
    )
    .bind(now)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    
    println!("✅ Found {} assignments for user {}", assignments.len(), user_id);
    
    // Ambil setting user mapping Course NAME ke Parallel Code (karena struct AssignmentWithCourse punya course_name)
    let user_settings = sqlx::query!(
        "SELECT c.name, ucs.parallel_code 
         FROM user_course_settings ucs
         JOIN courses c ON ucs.course_id = c.id
         WHERE ucs.user_id = $1",
        user_id
    )
    .fetch_all(pool)
    .await?;
    
    let mut settings_map = HashMap::new();
    for s in user_settings {
        settings_map.insert(s.name, s.parallel_code);
    }
    
    for (i, a) in assignments.iter().enumerate() {
        let deadline_str = match a.deadline {
            Some(d) => d.to_string(),
            None => "⚠️ NO DEADLINE".to_string()
        };
        let parallel_display = if a.parallel_codes.is_empty() {
            "N/A".to_string()
        } else {
            format!("[{}]", a.parallel_codes.join(", "))
        };
        println!("  {}. {} - Deadline: {} - Parallels: {} - Completed: {}", 
            i + 1, a.title, deadline_str, parallel_display, a.is_completed);
    }

    println!("");
    
    Ok((assignments, settings_map))
}

/// Find course by name (case-insensitive)
pub async fn get_course_by_name(
    pool: &PgPool,
    course_name: &str,
) -> Result<Option<Course>> {
    let course = sqlx::query_as::<_, Course>(
        "SELECT * FROM courses WHERE LOWER(name) = LOWER($1)"
    )
    .bind(course_name)
    .fetch_optional(pool)
    .await?;

    Ok(course)
}

/// Find course by name or alias
pub async fn get_course_by_name_or_alias(
    pool: &PgPool,
    search_term: &str,
) -> Result<Option<Course>> {
    let search_lower = search_term.to_lowercase();
    
    let course = sqlx::query_as::<_, Course>(
        r#"
        SELECT * FROM courses 
        WHERE LOWER(name) = LOWER($1) 
           OR EXISTS (
               SELECT 1 FROM unnest(aliases) AS alias 
               WHERE LOWER(alias) = LOWER($1)
           )
        LIMIT 1
        "#
    )
    .bind(&search_lower)
    .fetch_optional(pool)
    .await?;

    Ok(course)
}

/// Get all courses formatted with their aliases for AI prompt
pub async fn get_all_courses_formatted(pool: &PgPool) -> Result<String> {
    let courses = sqlx::query_as::<_, Course>(
        "SELECT * FROM courses ORDER BY name"
    )
    .fetch_all(pool)
    .await?;
    
    let formatted = courses
        .iter()
        .map(|c| {
            if let Some(ref aliases) = c.aliases {
                if !aliases.is_empty() {
                    format!("{} (aliases: {})", c.name, aliases.join(", "))
                } else {
                    c.name.clone()
                }
            } else {
                c.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n- ");
    
    Ok(format!("- {}", formatted))
}

/// Get assignment with course by ID
pub async fn get_assignment_with_course_by_id(
    pool: &PgPool,
    assignment_id: Uuid,
) -> Result<Option<AssignmentWithCourse>, sqlx::Error> {
    let assignment = sqlx::query_as::<_, AssignmentWithCourse>(
        r#"
        SELECT 
            a.id,
            c.name as course_name,
            a.parallel_codes,
            a.title,
            c.aliases[1] as first_alias,
            a.description,  
            a.deadline,
            a.message_ids,
            a.sender_id,
            false as is_completed,
            a.relating_messages
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.id = $1
        "#
    )
    .bind(assignment_id)
    .fetch_optional(pool)
    .await?;
    
    Ok(assignment)
}

/// Find assignments by keywords (for update detection)
pub async fn find_assignment_by_keywords(
    pool: &PgPool,
    keywords: &[String],
    course_id: Option<Uuid>,
) -> Result<Vec<Assignment>> {
    if keywords.is_empty() {
        return Ok(vec![]);
    }
    
    // Strategy 1: Search by course + keywords
    if let Some(cid) = course_id {
        let patterns: Vec<String> = keywords
            .iter()
            .map(|kw| format!("%{}%", kw.to_lowercase()))
            .collect();
        
        let mut query = String::from(
            r#"SELECT 
                id, created_at, course_id, title, description, deadline, 
                parallel_codes, sender_id, message_ids, reminder_1h_sent, relating_messages
            FROM assignments WHERE course_id = $1 AND ("#
        );
        
        let mut conditions = Vec::new();
        for i in 0..keywords.len() {
            conditions.push(format!(
                "(LOWER(title) LIKE ${} OR LOWER(description) LIKE ${})",
                i * 2 + 2,
                i * 2 + 3
            ));
        }
        
        query.push_str(&conditions.join(" AND "));
        query.push_str(") ORDER BY created_at DESC LIMIT 5");
        
        let mut sql_query = sqlx::query_as::<_, Assignment>(&query).bind(cid);
        
        for pattern in &patterns {
            sql_query = sql_query.bind(pattern).bind(pattern);
        }
        
        let assignments = sql_query.fetch_all(pool).await?;
        
        if !assignments.is_empty() {
            return Ok(assignments);
        }
    }
    
    // Strategy 2: Search by keywords only
    let patterns: Vec<String> = keywords
        .iter()
        .map(|kw| format!("%{}%", kw.to_lowercase()))
        .collect();
    
    let mut conditions = Vec::new();
    for i in 0..keywords.len() {
        conditions.push(format!(
            "(LOWER(title) LIKE ${} OR LOWER(description) LIKE ${})",
            i * 2 + 1,
            i * 2 + 2
        ));
    }
    
    let query = format!(
        r#"SELECT 
            id, created_at, course_id, title, description, deadline, 
            parallel_codes, sender_id, message_ids, reminder_1h_sent, relating_messages
        FROM assignments WHERE {} ORDER BY created_at DESC LIMIT 5"#,
        conditions.join(" OR ")
    );
    
    let mut sql_query = sqlx::query_as::<_, Assignment>(&query);
    
    for pattern in &patterns {
        sql_query = sql_query.bind(pattern).bind(pattern);
    }
    
    let assignments = sql_query.fetch_all(pool).await?;
    
    Ok(assignments)
}

// ========================================
// UPDATE OPERATIONS
// ========================================

/// Update specific fields of an assignment
#[allow(non_snake_case)]
#[allow(non_snake_case)]
pub async fn update_assignment_fields(
    pool: &PgPool,
    id: Uuid,
    new_deadline: Option<DateTime<Utc>>,
    new_title: Option<String>,
    new_description: Option<String>,
    new_parallel_codes: Option<Vec<String>>,
    incoming_message_id: Option<String>,
    incoming_message_content: Option<String>,
) -> Result<Assignment> {
    let mut tx = pool.begin().await?;
    
    // Fetch current assignment
    let current = sqlx::query_as::<_, Assignment>(
        r#"SELECT 
            id, created_at, course_id, title, description, deadline,
            parallel_codes, sender_id, message_ids, reminder_1h_sent, relating_messages
        FROM assignments WHERE id = $1"#
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    
    // Use new values if provided, otherwise keep current
    let final_deadline = new_deadline.or(current.deadline);
    let final_title = new_title.unwrap_or(current.title);
    let final_description = new_description.unwrap_or(current.description);
    
    // Handle parallel_codes properly
    let final_parallel_codes: Vec<String> = if let Some(codes) = new_parallel_codes {
        codes.iter().map(|c| c.to_lowercase()).collect()
    } else {
        current.parallel_codes
    };
    
    // ✅ FIX: Build the new arrays in Rust, not in SQL
    let final_message_ids = if let Some(new_msg_id) = incoming_message_id {
        let mut ids = current.message_ids.clone();
        ids.push(new_msg_id);
        ids
    } else {
        current.message_ids
    };
    
    let final_relating_messages = if let Some(new_msg_content) = incoming_message_content {
        let mut messages = current.relating_messages.clone();
        messages.push(new_msg_content);
        messages
    } else {
        current.relating_messages
    };
    
    // ✅ FIX: Simple UPDATE with direct array binding
    let assignment = sqlx::query_as::<_, Assignment>(
        r#"
        UPDATE assignments
        SET deadline = $2, 
            title = $3, 
            description = $4,
            parallel_codes = $5,
            message_ids = $6,
            relating_messages = $7
        WHERE id = $1
        RETURNING 
            id, created_at, course_id, title, description, deadline,
            parallel_codes, sender_id, message_ids, reminder_1h_sent, relating_messages
        "#
    )
    .bind(id)
    .bind(final_deadline)
    .bind(&final_title)
    .bind(&final_description)
    .bind(&final_parallel_codes)
    .bind(&final_message_ids)
    .bind(&final_relating_messages)
    .fetch_one(&mut *tx)
    .await?;
    
    tx.commit().await?;
    
    println!("   \x1b[32m✅ Updated\x1b[0m\n");
    
    Ok(assignment)
}

// ========================================
// DELETE OPERATIONS
// ========================================

/// Delete assignment by ID
pub async fn delete_assignment(
    pool: &PgPool,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM assignments WHERE id = $1",
        id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Parse deadline string with TIMESTAMP support (YYYY-MM-DD HH:MM)
#[allow(non_snake_case)]
pub fn parse_deadline(deadline_str: &str) -> Result<DateTime<Utc>, String> {
    let deadline_str = deadline_str.trim();
    
    // Define WIB timezone (UTC+7)
    let wib = FixedOffset::east_opt(7 * 3600).unwrap();
    
    // Try parsing with timestamp first (YYYY-MM-DD HH:MM)
    if let Ok(naive_dt) = NaiveDateTime::parse_from_str(deadline_str, "%Y-%m-%d %H:%M") {
        return match wib.from_local_datetime(&naive_dt).single() {
            Some(dt_wib) => Ok(dt_wib.with_timezone(&Utc)),
            None => Err(format!("Invalid datetime (ambiguous): {}", deadline_str))
        };
    }
    
    // Fallback: Try parsing date only (YYYY-MM-DD) - default to 23:59
    if let Ok(date) = chrono::NaiveDate::parse_from_str(deadline_str, "%Y-%m-%d") {
        let naive_datetime = date.and_hms_opt(23, 59, 59).unwrap();
        return match wib.from_local_datetime(&naive_datetime).single() {
            Some(dt_wib) => Ok(dt_wib.with_timezone(&Utc)),
            None => Err(format!("Invalid date (ambiguous): {}", deadline_str))
        };
    }
    
    Err(format!("Failed to parse deadline '{}'. Expected format: 'YYYY-MM-DD HH:MM' or 'YYYY-MM-DD'", deadline_str))
}

/// Set user preference for a specific course parallel
pub async fn set_user_course_parallel(
    pool: &PgPool,
    user_id: &str,
    course_name_query: &str,
    parallel_code: &str,
) -> Result<String, sqlx::Error> {
    // 1. Cari Course dulu berdasarkan nama/alias (reuse fungsi yang sudah ada)
    let course = get_course_by_name_or_alias(pool, course_name_query).await?;
    
    match course {
        Some(c) => {
            let clean_code = parallel_code.to_lowercase();
            
            // 2. Upsert (Insert atau Update jika sudah ada)
            sqlx::query!(
                r#"
                INSERT INTO user_course_settings (user_id, course_id, parallel_code)
                VALUES ($1, $2, $3)
                ON CONFLICT (user_id, course_id) 
                DO UPDATE SET parallel_code = $3, created_at = NOW()
                "#,
                user_id,
                c.id,
                clean_code
            )
            .execute(pool)
            .await?;
            
            Ok(format!("✅ Berhasil! Kelas untuk *{}* diatur ke *{}*.", c.name, clean_code.to_uppercase()))
        },
        None => Ok(format!("❌ Mata kuliah *{}* tidak ditemukan.", course_name_query))
    }
}

// ========================================
// ADDITIONAL HELPER IF NEEDED
// ========================================

/// Helper to normalize parallel codes
pub fn normalize_parallel_codes(codes: Vec<String>) -> Vec<String> {
    codes.iter()
        .map(|c| c.to_lowercase())
        .collect()
}