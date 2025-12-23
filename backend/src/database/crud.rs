use sqlx::PgPool;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::models::AssignmentDisplay; 

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

// 3. UPDATE (Edit Judul & Deskripsi)
pub async fn update_assignment_details(
    pool: &PgPool, 
    assignment_id: Uuid, 
    new_title: &str,
    new_description: &str
) -> Result<String, sqlx::Error> {
    
    let result = sqlx::query!(
        "UPDATE assignments SET title = $1, description = $2 WHERE id = $3",
        new_title,
        new_description,
        assignment_id
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        Ok("Gagal: Tugas tidak ditemukan".to_string())
    } else {
        Ok("Sukses: Detail tugas berhasil diperbarui".to_string())
    }
}

// 4. DELETE (Hapus Tugas)
pub async fn delete_assignment(pool: &PgPool, assignment_id: Uuid) -> Result<String, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM assignments WHERE id = $1",
        assignment_id
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        Ok("Gagal: Tugas tidak ditemukan".to_string())
    } else {
        Ok("Sukses: Tugas berhasil dihapus".to_string())
    }
}