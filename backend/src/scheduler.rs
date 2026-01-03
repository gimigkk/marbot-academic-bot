// backend/src/scheduler.rs - Fixed for Optional Deadline
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use sqlx::PgPool;
use crate::database::crud;
use crate::models::SendTextRequest;

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};

pub async fn start_scheduler(pool: PgPool) -> Result<(), JobSchedulerError> {
    let sched = JobScheduler::new().await?;

    // 07:00 WIB (00:00 UTC)
    let pool_pagi = pool.clone();
    sched.add(Job::new_async("0 0 0 * * *", move |_uuid, _l| {
        let pool = pool_pagi.clone();
        Box::pin(async move {
            println!("⏰ REMINDER PAGI (07:00 WIB):");
            if let Err(e) = run_reminder_task(pool, "☀️ Selamat pagi Ilkomers!").await {
                eprintln!("❌ Error reminder pagi: {}", e);
            }
        })
    })?).await?;

    // 17:00 WIB (10:00 UTC)
    let pool_sore = pool.clone();
    sched.add(Job::new_async("0 0 10 * * *", move |_uuid, _l| {
        let pool = pool_sore.clone();
        Box::pin(async move {
            println!("⏰ REMINDER SORE (17:00 WIB):");
            if let Err(e) = run_reminder_task(pool, "🌇 Selamat sore Ilkomers!").await {
                eprintln!("❌ Error reminder sore: {}", e);
            }
        })
    })?).await?;

    sched.start().await?;
    Ok(())
}

async fn run_reminder_task(pool: PgPool, greeting: &str) -> Result<(), Box<dyn std::error::Error>> {
    let assignments = crud::get_active_assignments_sorted(&pool).await?;

    if assignments.is_empty() {
        println!("📭 Tidak ada tugas aktif, skip reminder.");
        return Ok(());
    }

    let mut message = String::new();
    message.push_str(greeting);
    message.push_str("\n*Pengingat Tugas*\n\n");
    message.push_str("Keterangan:\n🔴 Deadline 0–2 hari\n🟢 Deadline > 2 hari\n⚪ Belum ada deadline\n\n");

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

        message.push_str(&format!("{} *[{}] [{}]*\n", status, i + 1, course));
        message.push_str(&format!("{} *[{}] [{}]*\n", status, i + 1, title));
        message.push_str(&format!("📌 {}\n", course));
        if !desc_line.is_empty() {
            message.push_str(&format!("{}\n", desc_line));
        }
        message.push('\n');
    }

    message.push_str("_Semangat!_ 💪");

    let channels_env = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
    let target_channels: Vec<&str> = channels_env
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if target_channels.is_empty() {
        println!("⚠️ ACADEMIC_CHANNELS kosong, skip kirim reminder.");
        return Ok(());
    }

    let client = reqwest::Client::new();
    let waha_url = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://localhost:3001".to_string());
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());

    for chat_id in target_channels {
        let payload = SendTextRequest {
            chat_id: chat_id.to_string(),
            text: message.clone(),
            session: "default".to_string(),
        };

        println!("📤 Mengirim reminder ke {}", chat_id);
        let _ = client
            .post(format!("{}/api/sendText", waha_url))
            .header("X-Api-Key", &api_key)
            .json(&payload)
            .send()
            .await;
    }

    Ok(())
}

/// Status indicator based on deadline
#[allow(non_snake_case)]
fn status_dot(deadline: &Option<DateTime<Utc>>) -> &'static str {
    match deadline {
        Some(d) => {
            let days = days_left(d);
            if days < 1 {
                "🔴"
            } else if days == 1 {
                "🟠"
            } else if days == 2 {
                "🟡"
            } else {
                "🟢"
            }
        }
        None => "⚪" // No deadline set
    }
}

fn days_left(deadline_utc: &DateTime<Utc>) -> i64 {
    let now = Local::now().date_naive();
    let due = deadline_utc.with_timezone(&Local).date_naive();
    (due - now).num_days()
}

#[allow(non_snake_case)]
fn humanize_deadline(deadline: &Option<DateTime<Utc>>) -> String {
    match deadline {
        Some(deadline_utc) => {
            let delta = days_left(deadline_utc);
            let due = deadline_utc.with_timezone(&Local).date_naive();
            let date_str = format_date_id(due);

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
    let one_line = s
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut out = String::new();
    for (i, ch) in one_line.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn sanitize_wa_md(s: &str) -> String {
    s.replace('*', "×")
        .replace('_', " ")
        .replace('~', "-")
        .replace('`', "'")
}