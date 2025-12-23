use sqlx::{PgPool, Result};
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::models::{Assignment, NewAssignment, Course, AssignmentDisplay};

// ========================================
// CREATE OPERATIONS
// ========================================

/// Create a new assignment in the database
pub async fn create_assignment(
    pool: &PgPool,
    new_assignment: NewAssignment,
) -> Result<String, sqlx::Error> {
    
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
    .fetch_optional(pool)
    .await?;

    // Validasi Course
    let real_course_name = match course {
        Some(c) => c.name,
        None => match new_assignment.course_id {
            Some(id) => return Ok(format!("Gagal: Mata kuliah dengan ID '{}' tidak ditemukan", id)),
            None => return Ok("Gagal: Mata kuliah tidak ditemukan (ID tidak ada)".to_string()),
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
    .execute(pool)
    .await?;

    Ok(format!("Sukses! Tugas '{}' berhasil disimpan ke matkul '{}'", new_assignment.title, real_course_name))
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

/// TODO: CHANGE IT TO COURSE ALIAS
/// Get all courses formatted as "course1, course2, course3"
pub async fn get_all_courses_formatted(pool: &PgPool) -> Result<String> {
    let courses = sqlx::query_as::<_, Course>(
        "SELECT * FROM courses ORDER BY name"
    )
    .fetch_all(pool)
    .await?;
    
    let formatted = courses
        .iter()
        .map(|c| c.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    
    Ok(formatted)
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

/// Find assignments by keywords (for update detection)
pub async fn find_assignment_by_keywords(
    pool: &PgPool,
    keywords: &[String],
    _course_id: Option<Uuid>,
) -> Result<Vec<Assignment>> {
    if keywords.is_empty() {
        return Ok(vec![]);
    }
    
    // Pre-build all patterns to ensure they live long enough
    let patterns: Vec<String> = keywords
        .iter()
        .map(|kw| format!("%{}%", kw.to_lowercase()))
        .collect();
    
    // Build pattern matching for each keyword
    let mut conditions = Vec::new();
    
    for (i, _) in keywords.iter().enumerate() {
        conditions.push(format!(
            "(LOWER(title) LIKE ${} OR LOWER(description) LIKE ${})",
            i * 2 + 1, 
            i * 2 + 2
        ));
    }
    
    let query = format!(
        "SELECT * FROM assignments WHERE {} ORDER BY created_at DESC LIMIT 5",
        conditions.join(" AND ")
    );
    
    println!("🔍 Searching with query: {}", query);
    println!("🔍 Keywords: {:?}", keywords);
    
    let mut sql_query = sqlx::query_as::<_, Assignment>(&query);
    
    // Bind each pattern twice (for title and description)
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
pub async fn update_assignment_fields(
    pool: &PgPool,
    id: Uuid,
    new_deadline: Option<DateTime<Utc>>,
    new_description: Option<String>,
) -> Result<Assignment> {
    // Build dynamic query based on what changed
    let assignment = match (new_deadline, &new_description) {
        (Some(dl), Some(desc)) => {
            // Update both deadline and description
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
            .fetch_one(pool)
            .await?
        }
        (Some(dl), None) => {
            // Update only deadline
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
            .fetch_one(pool)
            .await?
        }
        (None, Some(desc)) => {
            // Update only description
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
            .fetch_one(pool)
            .await?
        }
        (None, None) => {
            // Nothing to update, just fetch the assignment
            sqlx::query_as::<_, Assignment>(
                "SELECT * FROM assignments WHERE id = $1"
            )
            .bind(id)
            .fetch_one(pool)
            .await?
        }
    };
    
    println!("✅ Updated assignment: {}", assignment.title);
    
    Ok(assignment)
}