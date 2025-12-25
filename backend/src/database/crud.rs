use sqlx::{PgPool, Result};
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::models::{Assignment, NewAssignment, Course, AssignmentDisplay, AssignmentWithCourse};

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
    
    // A. Cari Course (ILIKE)
    let course = sqlx::query!(
        r#"
        SELECT id, name 
        FROM courses 
        WHERE id = $1
        LIMIT 1
        "#,
        new_assignment.course_id
    )
    .fetch_optional(&mut *tx)  // ✅ Use transaction
    .await?;

    // Validasi Course
    let real_course_name = match course {
        Some(c) => c.name,
        None => match new_assignment.course_id {
            Some(id) => {
                tx.commit().await?;  // Commit before returning
                return Ok(format!("Gagal: Mata kuliah dengan ID '{}' tidak ditemukan", id));
            }
            None => {
                tx.commit().await?;  // Commit before returning
                return Ok("Gagal: Mata kuliah tidak ditemukan (ID tidak ada)".to_string());
            }
        }
    };
    
    // kode paralel (huruf kecil)
    let clean_parallel = new_assignment.parallel_code.as_ref().map(|p| p.to_lowercase());

    // B. Insert Tugas
    sqlx::query!(
        r#"
        INSERT INTO assignments (
            course_id, parallel_code, title, description, 
            deadline, sender_id, message_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        new_assignment.course_id,
        clean_parallel,
        new_assignment.title,
        new_assignment.description,
        new_assignment.deadline,
        new_assignment.sender_id,
        new_assignment.message_id
    )
    .execute(&mut *tx)  // ✅ Use transaction
    .await?;

    tx.commit().await?;
    Ok(format!("Sukses! Tugas '{}' berhasil disimpan ke matkul '{}'\n", new_assignment.title, real_course_name))
}



// ========================================
// READ OPERATIONS
// ========================================

// 2. READ (Melihat SEMUA Tugas)
pub async fn get_assignments(pool: &PgPool) -> Result<Vec<AssignmentDisplay>, sqlx::Error> {
    let assignments = sqlx::query_as!(
        AssignmentDisplay,
        r#"
        SELECT 
            a.id, 
            c.name as "course_name!", 
            a.parallel_code, 
            a.title, 
            a.description, 
            a.deadline
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        ORDER BY a.deadline ASC
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(assignments)
}

/// Check if an assignment with this title already exists for a course
/// Uses case-insensitive comparison to catch duplicates like "LKP 13" vs "lkp 13"
pub async fn get_assignment_by_title_and_course(
    pool: &PgPool,
    title: &str,
    course_id: uuid::Uuid,
) -> Result<Option<Assignment>, sqlx::Error> {
    let mut tx = pool.begin().await?;  // ✅ Start transaction
    
    let result = sqlx::query_as::<_, Assignment>(
        r#"
        SELECT * FROM assignments
        WHERE title = $1 AND course_id = $2
        "#
    )
    .bind(title)
    .bind(course_id)
    .fetch_optional(&mut *tx)  // ✅ Use transaction instead of pool
    .await?;
    
    tx.commit().await?;  // ✅ Commit transaction
    Ok(result)
}

/// Get active assignments (not past deadline) with course info
pub async fn get_active_assignments(pool: &PgPool) -> Result<Vec<Assignment>> {
    let now = Utc::now();
    
    let assignments = sqlx::query_as::<_, Assignment>(
        r#"
        SELECT a.* 
        FROM assignments a
        WHERE a.deadline > $1 OR a.deadline IS NULL
        ORDER BY a.created_at DESC
        LIMIT 20
        "#
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    
    println!("✅ Found {} active assignments", assignments.len());
    
    Ok(assignments)
}

/// Yang ini versi sorted dari yang di atas, dipake di #tugas
/// Get active assignments sorted by deadline, then course name
/// Get active assignments sorted by deadline, then course name
pub async fn get_active_assignments_sorted(pool: &PgPool) -> Result<Vec<AssignmentWithCourse>, sqlx::Error> {
    let now = Utc::now();
    
    let assignments = sqlx::query_as!(
        AssignmentWithCourse,
        r#"
        SELECT 
            a.id,
            c.name as course_name,
            a.parallel_code,
            a.title,
            a.description,  
            a.deadline as "deadline!",
            a.message_id,
            a.sender_id
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.deadline >= $1 AND a.deadline IS NOT NULL
        ORDER BY a.deadline ASC, c.name ASC
        "#,
        now
    )
    .fetch_all(pool)
    .await?;
    
    println!("✅ Found {} active assignments\n", assignments.len());
    
    Ok(assignments)
}

/// Get recent assignments for update matching (doesn't filter by deadline)
/// Returns assignments sorted by recency (newest first)
pub async fn get_recent_assignments_for_update(
    pool: &PgPool,
    course_id: Option<uuid::Uuid>,
) -> Result<Vec<Assignment>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    
    let assignments = if let Some(cid) = course_id {
        // Get assignments from specific course, prioritize recent ones
        sqlx::query_as::<_, Assignment>(
            r#"
            SELECT * FROM assignments
            WHERE course_id = $1 
            AND deadline >= NOW() - INTERVAL '7 days'  -- Include assignments from last week
            ORDER BY created_at DESC  -- Most recent first
            LIMIT 10
            "#
        )
        .bind(cid)
        .fetch_all(&mut *tx)
        .await?
    } else {
        // Get assignments across all courses
        sqlx::query_as::<_, Assignment>(
            r#"
            SELECT * FROM assignments
            WHERE deadline >= NOW() - INTERVAL '7 days'
            ORDER BY created_at DESC
            LIMIT 10
            "#
        )
        .fetch_all(&mut *tx)
        .await?
    };
    
    tx.commit().await?;
    Ok(assignments)
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

/// Find course by name or alias (case-insensitive)
pub async fn get_course_by_name_or_alias(
    pool: &PgPool,
    search_term: &str,
) -> Result<Option<Course>> {
    let search_lower = search_term.to_lowercase();
    
    // Search by name OR any alias in the aliases array
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

/// Check if assignment already exists by message_id
pub async fn get_assignment_by_message_id(
    pool: &PgPool,
    message_id: &str,
) -> Result<Option<Assignment>> {
    let assignment = sqlx::query_as::<_, Assignment>(
        "SELECT * FROM assignments WHERE message_id = $1"
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await?;

    Ok(assignment)
}

/// Find assignments by keywords (for update detection) - IMPROVED VERSION
pub async fn find_assignment_by_keywords(
    pool: &PgPool,
    keywords: &[String],
    course_id: Option<Uuid>,
) -> Result<Vec<Assignment>> {
    if keywords.is_empty() {
        println!("⚠️ No keywords provided for search");
        return Ok(vec![]);
    }
    
    // Try different search strategies
    
    // Strategy 1: Search by course + keywords
    if let Some(cid) = course_id {
        println!("🔍 Strategy 1: Searching by course_id + keywords");
        let patterns: Vec<String> = keywords
            .iter()
            .map(|kw| format!("%{}%", kw.to_lowercase()))
            .collect();
        
        let mut query = String::from(
            "SELECT * FROM assignments WHERE course_id = $1 AND ("
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
        
        println!("🔍 Query: {}", query);
        println!("🔍 Course ID: {}", cid);
        println!("🔍 Keywords: {:?}", keywords);
        
        let mut sql_query = sqlx::query_as::<_, Assignment>(&query).bind(cid);
        
        for pattern in &patterns {
            sql_query = sql_query.bind(pattern).bind(pattern);
        }
        
        let assignments = sql_query.fetch_all(pool).await?;
        
        if !assignments.is_empty() {
            println!("✅ Found {} assignments with strategy 1", assignments.len());
            return Ok(assignments);
        }
    }
    
    // Strategy 2: Search by keywords only (broader search)
    println!("🔍 Strategy 2: Searching by keywords only");
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
        "SELECT * FROM assignments WHERE {} ORDER BY created_at DESC LIMIT 5",
        conditions.join(" OR ")  // Changed from AND to OR for broader matching
    );
    
    println!("🔍 Query: {}", query);
    
    let mut sql_query = sqlx::query_as::<_, Assignment>(&query);
    
    for pattern in &patterns {
        sql_query = sql_query.bind(pattern).bind(pattern);
    }
    
    let assignments = sql_query.fetch_all(pool).await?;
    
    println!("✅ Found {} matching assignments", assignments.len());
    
    Ok(assignments)
}

// ========================================
// UPDATE OPERATIONS
// ========================================

/// Update specific fields of an assignment
#[allow(non_snake_case)]
pub async fn update_assignment_fields(
    pool: &PgPool,
    id: Uuid,
    new_deadline: Option<DateTime<Utc>>,
    new_title: Option<String>,
    new_description: Option<String>,
) -> Result<Assignment> {
    println!("🔄 Updating assignment {} with deadline: {:?}, title: {:?}, description: {:?}", 
        id, new_deadline, new_title, new_description);
    
    let mut tx = pool.begin().await?;
    
    // Build dynamic query based on what changed
    let assignment = match (new_deadline, &new_title, &new_description) {
        // All three fields
        (Some(dl), Some(title), Some(desc)) => {
            sqlx::query_as::<_, Assignment>(
                r#"
                UPDATE assignments
                SET deadline = $2, title = $3, description = $4
                WHERE id = $1
                RETURNING *
                "#
            )
            .bind(id)
            .bind(dl)
            .bind(title)
            .bind(desc)
            .fetch_one(&mut *tx)
            .await?
        }
        // Deadline + Title
        (Some(dl), Some(title), None) => {
            sqlx::query_as::<_, Assignment>(
                r#"
                UPDATE assignments
                SET deadline = $2, title = $3
                WHERE id = $1
                RETURNING *
                "#
            )
            .bind(id)
            .bind(dl)
            .bind(title)
            .fetch_one(&mut *tx)
            .await?
        }
        // Deadline + Description
        (Some(dl), None, Some(desc)) => {
            sqlx::query_as::<_, Assignment>(
                r#"
                UPDATE assignments
                SET deadline = $2, description = $3
                WHERE id = $1
                RETURNING *
                "#
            )
            .bind(id)
            .bind(dl)
            .bind(desc)
            .fetch_one(&mut *tx)
            .await?
        }
        // Title + Description
        (None, Some(title), Some(desc)) => {
            sqlx::query_as::<_, Assignment>(
                r#"
                UPDATE assignments
                SET title = $2, description = $3
                WHERE id = $1
                RETURNING *
                "#
            )
            .bind(id)
            .bind(title)
            .bind(desc)
            .fetch_one(&mut *tx)
            .await?
        }
        // Only Deadline
        (Some(dl), None, None) => {
            sqlx::query_as::<_, Assignment>(
                r#"
                UPDATE assignments
                SET deadline = $2
                WHERE id = $1
                RETURNING *
                "#
            )
            .bind(id)
            .bind(dl)
            .fetch_one(&mut *tx)
            .await?
        }
        // Only Title
        (None, Some(title), None) => {
            sqlx::query_as::<_, Assignment>(
                r#"
                UPDATE assignments
                SET title = $2
                WHERE id = $1
                RETURNING *
                "#
            )
            .bind(id)
            .bind(title)
            .fetch_one(&mut *tx)
            .await?
        }
        // Only Description
        (None, None, Some(desc)) => {
            sqlx::query_as::<_, Assignment>(
                r#"
                UPDATE assignments
                SET description = $2
                WHERE id = $1
                RETURNING *
                "#
            )
            .bind(id)
            .bind(desc)
            .fetch_one(&mut *tx)
            .await?
        }
        // Nothing to update
        (None, None, None) => {
            sqlx::query_as::<_, Assignment>(
                "SELECT * FROM assignments WHERE id = $1"
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?
        }
    };
    
    tx.commit().await?;
    
    println!("✅ Successfully updated assignment: {}", assignment.title);
    
    Ok(assignment)
}

/// Parse deadline string (YYYY-MM-DD) to DateTime<Utc>
pub fn parse_deadline(deadline_str: &str) -> Result<DateTime<Utc>, String> {
    use chrono::NaiveDate;
    
    NaiveDate::parse_from_str(deadline_str, "%Y-%m-%d")
        .map_err(|e| format!("Failed to parse date '{}': {}", deadline_str, e))
        .map(|date| date.and_hms_opt(23, 59, 59).unwrap().and_utc())
}