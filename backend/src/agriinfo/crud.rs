use sqlx::{PgPool, Result};
use super::models::{NewAgriinfoTask, AgriinfoTask};

pub async fn create_agriinfo_task(
    pool: &PgPool,
    task: NewAgriinfoTask,
) -> Result<AgriinfoTask, sqlx::Error> {
    let inserted = sqlx::query_as::<_, AgriinfoTask>(
        r#"
        INSERT INTO agriinformatics (nama_tugas, deadline)
        VALUES ($1, $2)
        RETURNING id, nama_tugas, deadline, reminder_1h_sent
        "#
    )
    .bind(&task.nama_tugas)
    .bind(task.deadline)
    .fetch_one(pool)
    .await?;

    Ok(inserted)
}

pub async fn update_agriinfo_task(
    pool: &PgPool,
    id: i32,
    task: NewAgriinfoTask,
) -> Result<AgriinfoTask, sqlx::Error> {
    let updated = sqlx::query_as::<_, AgriinfoTask>(
        r#"
        UPDATE agriinformatics 
        SET nama_tugas = $1, deadline = $2
        WHERE id = $3
        RETURNING id, nama_tugas, deadline, reminder_1h_sent
        "#
    )
    .bind(&task.nama_tugas)
    .bind(task.deadline)
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(updated)
}

pub async fn delete_agriinfo_task(
    pool: &PgPool,
    id: i32,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM agriinformatics
        WHERE id = $1
        "#
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn get_upcoming_agriinfo_tasks(pool: &PgPool) -> Result<Vec<AgriinfoTask>, sqlx::Error> {
    let tasks = sqlx::query_as::<_, AgriinfoTask>(
        r#"
        SELECT id, nama_tugas, deadline, reminder_1h_sent 
        FROM agriinformatics 
        WHERE deadline >= NOW() - INTERVAL '24 HOURS'
        ORDER BY deadline ASC
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(tasks)
}
