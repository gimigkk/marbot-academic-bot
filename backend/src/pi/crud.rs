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

pub async fn get_upcoming_pi_tasks(pool: &PgPool) -> Result<Vec<PiTask>, sqlx::Error> {
    let tasks = sqlx::query_as::<_, PiTask>(
        r#"
        SELECT id, nama_tugas, deadline 
        FROM pekan_ilkomers 
        WHERE deadline >= NOW()
        ORDER BY deadline ASC
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(tasks)
}