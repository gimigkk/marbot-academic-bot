// backend/src/scheduler.rs
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use sqlx::PgPool;
use crate::database::crud;
use crate::models::SendTextRequest;

pub async fn start_scheduler(pool: PgPool) -> Result<(), JobSchedulerError> {
    let sched = JobScheduler::new().await?;

    // --- JADWAL 1: Jam 07:00 WIB (00:00 UTC) ---
    let pool_pagi = pool.clone();
    sched.add(Job::new_async("0 0 0 * * *", move |_uuid, _l| {
        let pool = pool_pagi.clone();
        Box::pin(async move {
            println!("⏰ Menjalankan Reminder Pagi...");
            if let Err(e) = run_reminder_task(pool, "Selamat Pagi! ☀️").await {
                eprintln!("❌ Error reminder pagi: {}", e);
            }
        })
    })?).await?;

    // --- JADWAL 2: Jam 17:00 WIB (10:00 UTC) ---
    let pool_sore = pool.clone();
    sched.add(Job::new_async("0 0 10 * * *", move |_uuid, _l| {
        let pool = pool_sore.clone();
        Box::pin(async move {
            println!("⏰ Menjalankan Reminder Sore...");
            if let Err(e) = run_reminder_task(pool, "Selamat Sore! 🌇").await {
                eprintln!("❌ Error reminder sore: {}", e);
            }
        })
    })?).await?;

    // Mulai scheduler
    sched.start().await?;
    Ok(())
}

// Ambil tugas -> Format Pesan -> Kirim ke WA
async fn run_reminder_task(pool: PgPool, greeting: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Ambil tugas aktif dari database (menggunakan fungsi yang sudah ada di crud.rs)
    let assignments = crud::get_active_assignments_sorted(&pool).await?;

    if assignments.is_empty() {
        println!("📭 Tidak ada tugas aktif, skip reminder.");
        return Ok(());
    }

    // 2. Format pesan (mirip dengan command #tugas)
    let mut message = format!("{} \n*Pengingat Tugas Harian*\n\nBerikut daftar tugas yang belum selesai:\n\n", greeting);
    
    for (i, t) in assignments.iter().enumerate() {
        let deadline = t.deadline.format("%d/%m/%Y").to_string();
        message.push_str(&format!(
            "{}. *{}* - {}\n   📅 Deadline: {}\n\n", 
            i + 1, t.course_name, t.title, deadline
        ));
    }
    message.push_str("_Semangat mengerjakannya!_ 💪");

    // 3. Ambil daftar target (Group ID) dari Environment Variable
    // Kita baca ulang dari env agar dinamis
    let channels_env = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
    let target_channels: Vec<&str> = channels_env.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    // 4. Kirim ke semua channel
    let client = reqwest::Client::new();
    let waha_url = "http://localhost:3001/api/sendText"; // Sesuaikan URL WAHA
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());

    for chat_id in target_channels {
        let payload = SendTextRequest {
            chat_id: chat_id.to_string(),
            text: message.clone(),
            session: "default".to_string(),
        };

        println!("📤 Mengirim reminder ke {}", chat_id);
        let _ = client.post(waha_url)
            .header("X-Api-Key", &api_key)
            .json(&payload)
            .send()
            .await;
    }

    Ok(())
}