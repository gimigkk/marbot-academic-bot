// backend/src/scheduler.rs
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use sqlx::PgPool;
use crate::database::crud;
use crate::models::{SendTextRequest, SendImageRequest, FileContent}; 
use chrono::{DateTime, Datelike, NaiveDate, Utc};

pub async fn start_scheduler(pool: PgPool) -> Result<(), JobSchedulerError> {
    let sched = JobScheduler::new().await?;

    // 1. REMINDER HARIAN (UTC TIME)    
    // URL Gambar untuk Pagi & Sore (Versi Raw Github)
    let image_pagi = "https://raw.githubusercontent.com/gimigkk/marbot-academic-bot/6b2b72dca7ca954fe5e8eef81649d9fff24515c9/asset/pagi.jpg"; 
    let image_sore = "https://raw.githubusercontent.com/gimigkk/marbot-academic-bot/6b2b72dca7ca954fe5e8eef81649d9fff24515c9/asset/malam.jpg";

    // 07:00 WIB = 00:00 UTC
    let pool_pagi = pool.clone();
    let img_pagi_url = image_pagi.to_string();
    sched.add(Job::new_async("0 0 0 * * *", move |_uuid, _l| {
        let pool = pool_pagi.clone();
        let img = img_pagi_url.clone();
        Box::pin(async move {
            println!("⏰ REMINDER PAGI (00:00 UTC / 07:00 WIB):");
            if let Err(e) = run_reminder_task(pool, "☀️ Selamat pagi Ilkomers!", Some(img)).await {
                eprintln!("❌ Error reminder pagi: {}", e);
            }
        })
    })?).await?;

    // 17:00 WIB = 10:00 UTC
    let pool_sore = pool.clone();
    let img_sore_url = image_sore.to_string();
    sched.add(Job::new_async("0 0 11 * * *", move |_uuid, _l| {
        let pool = pool_sore.clone();
        let img = img_sore_url.clone();
        Box::pin(async move {
            println!("⏰ REMINDER SORE (10:00 UTC / 17:00 WIB):");
            if let Err(e) = run_reminder_task(pool, "🌇 Selamat sore Ilkomers!", Some(img)).await {
                eprintln!("❌ Error reminder sore: {}", e);
            }
        })
    })?).await?;

   
    // 2. REMINDER DEADLINE MEPET (H-1 JAM)
    // Cek setiap 10 menit (Menit ke-1, 11, 21, dst)
    let pool_urgent = pool.clone();
    sched.add(Job::new_async("0 1/10 * * * *", move |_uuid, _l| {
        let pool = pool_urgent.clone();
        Box::pin(async move {
            if let Err(e) = check_urgent_deadlines(pool).await {
                eprintln!("❌ Error checking urgent deadlines: {}", e);
            }
        })
    })?).await?;

    sched.start().await?;
    Ok(())
}

// --- LOGIC REMINDER HARIAN ---

async fn run_reminder_task(
    pool: PgPool, 
    greeting: &str, 
    image_url: Option<String> 
) -> Result<(), Box<dyn std::error::Error>> {
    
    let assignments = crud::get_active_assignments_sorted(&pool).await?;

    if assignments.is_empty() {
        println!("📭 Tidak ada tugas aktif, skip reminder.");
        return Ok(());
    }

    let mut message = String::new();
    message.push_str(greeting);
    message.push_str("\n*Pengingat Tugas*\n\n");

    for (i, a) in assignments.iter().enumerate() {
        let status = status_dot(&a.deadline);
        let due_text = humanize_deadline(&a.deadline);

        let course = sanitize_wa_md(&a.course_name);
        let title = sanitize_wa_md(&a.title);

        let desc_line = a
            .description
            .as_ref()
            .map(|d| sanitize_wa_md(d))
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .map(|d| format!("📝 {}", preview_text(&d, 25)))
            .unwrap_or_default();

        message.push_str(&format!("{} *[{}] [{}]*\n", status, i + 1, title));
        message.push_str(&format!("📌 {}\n", course));
        message.push_str(&format!("⏰ {}\n", due_text));
        if !desc_line.is_empty() {
            message.push_str(&format!("{}\n", desc_line));
        }
        message.push('\n');
    }

    message.push_str("_Semangat!_ 💪");
    
    // Panggil fungsi kirim dengan opsi gambar
    send_to_channels(message, image_url).await
}

// --- LOGIC REMINDER H-1 JAM ---
async fn check_urgent_deadlines(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Range: Sekarang s.d. 1 jam ke depan (UTC)
    let now = Utc::now();
    let one_hour_later = now + chrono::Duration::hours(1);

    // 2. Query tugas deadline < 1 jam & belum diingatkan
    let urgent_tasks = sqlx::query!(
        r#"
        SELECT 
            a.id, a.title, c.name as course_name, a.deadline
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.deadline > $1 
          AND a.deadline <= $2 
          AND a.reminder_1h_sent = FALSE
        "#,
        now,
        one_hour_later
    )
    .fetch_all(&pool)
    .await?;

    if urgent_tasks.is_empty() {
        return Ok(());
    }

    println!("🚨 Menemukan {} tugas deadline < 1 jam!", urgent_tasks.len());

    for task in urgent_tasks {
        let deadline_wib = task.deadline
            .map(|d| d.with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap()))
            .unwrap(); 
            
        let time_str = deadline_wib.format("%H:%M").to_string();
        
        let message = format!(
            "⚠️ *JANGAN LUPA KUMPULKAN H-1 JAM* ⚠️\n\n\
            📌 *{}*\n\
            📚 {}\n\
            ⏰ Deadline: Pukul *{}* WIB\n\
            \n\
            _Segera kumpulkan sebelum tugas ditutup!_",
            sanitize_wa_md(&task.title),
            sanitize_wa_md(&task.course_name),
            time_str
        );

        // Kirim Pesan tanpa gambar (None)
        send_to_channels(message, None).await?;

        // Tandai sudah dikirim
        sqlx::query!(
            "UPDATE assignments SET reminder_1h_sent = TRUE WHERE id = $1",
            task.id
        )
        .execute(&pool)
        .await?;
        
        println!("✅ Reminder urgent dikirim untuk: {}", task.title);
    }

    Ok(())
}

// --- HELPER FUNCTIONS ---

async fn send_to_channels(
    message: String, 
    image_url: Option<String>
) -> Result<(), Box<dyn std::error::Error>> {
    
    let channels_env = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
    let target_channels: Vec<&str> = channels_env
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if target_channels.is_empty() {
        println!("⚠️ ACADEMIC_CHANNELS kosong, skip kirim.");
        return Ok(());
    }

    let client = reqwest::Client::new();
    let waha_url = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string());
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());

    for chat_id in target_channels {
        if let Some(url) = &image_url {
            // === LOGIKA KIRIM GAMBAR ===
            println!("📤 Mengirim reminder bergambar ke {}", chat_id);
            
            let payload = SendImageRequest {
                chatId: chat_id.to_string(),
                file: FileContent {
                    url: url.clone(),
                    mimetype: "image/jpeg".to_string(),
                    filename: "reminder.jpg".to_string(),
                },
                caption: message.clone(),
                session: "default".to_string(),
            };

            let _ = client
                .post(format!("{}/api/sendImage", waha_url))
                .header("X-Api-Key", &api_key)
                .json(&payload)
                .send()
                .await;
                
        } else {
            // === LOGIKA KIRIM TEKS BIASA ===
            let payload = SendTextRequest {
                chat_id: chat_id.to_string(),
                text: message.clone(),
                session: "default".to_string(),
            };

            let _ = client
                .post(format!("{}/api/sendText", waha_url))
                .header("X-Api-Key", &api_key)
                .json(&payload)
                .send()
                .await;
        }
    }
    Ok(())
}

#[allow(non_snake_case)]
fn status_dot(deadline: &Option<DateTime<Utc>>) -> &'static str {
    match deadline {
        Some(d) => {
            let days = days_left(d);
            if days < 1 { "🔴" } 
            else if days == 1 { "🟠" } 
            else if days == 2 { "🟡" } 
            else { "🟢" }
        }
        None => "⚪"
    }
}

fn days_left(deadline_utc: &DateTime<Utc>) -> i64 {
    let wib_offset = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    let now_wib = Utc::now().with_timezone(&wib_offset).date_naive();
    let due_wib = deadline_utc.with_timezone(&wib_offset).date_naive();
    
    (due_wib - now_wib).num_days()
}

#[allow(non_snake_case)]
fn humanize_deadline(deadline: &Option<DateTime<Utc>>) -> String {
    match deadline {
        Some(deadline_utc) => {
            let delta = days_left(deadline_utc);
            let wib_offset = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
            let due_wib = deadline_utc.with_timezone(&wib_offset).date_naive();
            let date_str = format_date_id(due_wib);

            match delta {
                0 => format!("Hari ini ({})", date_str),
                1 => format!("Besok ({})", date_str),
                d if d >= 2 => format!("H-{} ({})", d, date_str), 
                -1 => format!("Kemarin ({})", date_str),
                d => format!("lewat {} hari ({})", d.abs(), date_str),
            }
        }
        None => "⚠️ Belum ada deadline".to_string()
    }
}

fn format_date_id(date: NaiveDate) -> String {
    let day = date.day();
    let month = match date.month() {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "Mei", 6 => "Jun", 7 => "Jul", 8 => "Agu",
        9 => "Sep", 10 => "Okt", 11 => "Nov", 12 => "Des",
        _ => "???",
    };
    format!("{} {} {}", day, month, date.year())
}

fn preview_text(s: &str, max_chars: usize) -> String {
    let one_line = s.replace('\n', " ").split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (i, ch) in one_line.chars().enumerate() {
        if i >= max_chars { out.push('…'); return out; }
        out.push(ch);
    }
    out
}

fn sanitize_wa_md(s: &str) -> String {
    s.replace('*', "×").replace('_', " ").replace('~', "-").replace('`', "'")
}