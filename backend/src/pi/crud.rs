use sqlx::{PgPool, Result};
use super::models::{NewPiTask, PiTask};

pub async fn create_pi_task(
    pool: &PgPool,
    task: NewPiTask,
) -> Result<PiTask, sqlx::Error> {
    let inserted = sqlx::query_as::<_, PiTask>(
        r#"
        INSERT INTO pekan_ilkomers (nama_tugas, deadline)
        VALUES ($1, $2)
        RETURNING id, nama_tugas, deadline
        "#
    )
    .bind(&task.nama_tugas)
    .bind(task.deadline)
    .fetch_one(pool)
    .await?;

    Ok(inserted)
}

pub async fn update_pi_task(
    pool: &PgPool,
    id: i32,
    task: NewPiTask,
) -> Result<PiTask, sqlx::Error> {
    let updated = sqlx::query_as::<_, PiTask>(
        r#"
        UPDATE pekan_ilkomers 
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

pub async fn delete_pi_task(
    pool: &PgPool,
    id: i32,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM pekan_ilkomers
        WHERE id = $1
        "#
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn get_upcoming_pi_tasks(pool: &PgPool) -> Result<Vec<PiTask>, sqlx::Error> {
    let tasks = sqlx::query_as::<_, PiTask>(
        r#"
        SELECT id, nama_tugas, deadline 
        FROM pekan_ilkomers 
        WHERE deadline >= NOW() - INTERVAL '24 HOURS'
        ORDER BY deadline ASC
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(tasks)
}