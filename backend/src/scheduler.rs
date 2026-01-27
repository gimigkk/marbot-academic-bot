// backend/src/scheduler.rs
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use sqlx::PgPool;
use crate::database::crud;
use crate::models::SendTextRequest;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use crate::tui::{JobLogger, state::LogEntry};
use tokio::sync::mpsc;
//use tokio::time::sleep;

pub async fn start_scheduler(
    pool: PgPool,
    log_tx: mpsc::UnboundedSender<LogEntry>,
) -> Result<(), JobSchedulerError> {
    let sched = JobScheduler::new().await?;


    let tui_state = crate::TUI_STATE.get().cloned();

    // 1. REMINDER HARIAN (UTC TIME)
    // 07:00 WIB = 00:00 UTC
    let pool_pagi = pool.clone();
    let log_tx_pagi = log_tx.clone();
    let tui_state_pagi = tui_state.clone();
    
    sched.add(Job::new_async("0 0 0 * * *", move |_uuid, _l| {
        let pool = pool_pagi.clone();
        let log_tx = log_tx_pagi.clone();
        let tui_state = tui_state_pagi.clone();
        
        Box::pin(async move {
            let job_id = crate::tui::generate_job_id();
            
        
            if let Some(tui) = &tui_state {
                tui.create_job(
                    job_id.clone(),
                    "SYSTEM".to_string(),
                    "Daily Reminder".to_string(),
                    Some("Morning task reminder (07:00 WIB)".to_string()),
                    None,
                    vec!["#scheduler".to_string(), "#reminder".to_string(), "#daily".to_string()],
                ).await;
            }
            
            let logger = JobLogger::new(job_id, log_tx);
            
            logger.log("⏰ REMINDER PAGI (00:00 UTC / 07:00 WIB)");
            if let Err(e) = run_reminder_task(pool, "_Selamat pagi Ilkomers!_", &logger).await {
                logger.log(&format!("❌ Error reminder pagi: {}", e));
                logger.set_status(crate::tui::state::JobStatus::Failed);
            } else {
                logger.set_status(crate::tui::state::JobStatus::Completed);
            }
        })
    })?).await?;

    // 2. REMINDER DEADLINE MEPET (H-1 JAM)
    // Cek setiap 10 menit 
    let pool_urgent = pool.clone();
    let log_tx_urgent = log_tx.clone();
    let tui_state_urgent = tui_state.clone();
    
    sched.add(Job::new_async("0 1/10 * * * *", move |_uuid, _l| {
        let pool = pool_urgent.clone();
        let log_tx = log_tx_urgent.clone();
        let tui_state = tui_state_urgent.clone();
        
        Box::pin(async move {
        
            let now = Utc::now();
            let one_hour_later = now + chrono::Duration::hours(1);
            
            let urgent_count = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(*) 
                FROM assignments a
                WHERE a.deadline > $1 
                  AND a.deadline <= $2 
                  AND a.reminder_1h_sent = FALSE
                "#
            )
            .bind(now)
            .bind(one_hour_later)
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
            
            // Only create job entry if there's work to do
            if urgent_count == 0 {
                return; // Silent return - no logs, no job entry
            }
            
            let job_id = crate::tui::generate_job_id();
            
            // Register system job in TUI
            if let Some(tui) = &tui_state {
                tui.create_job(
                    job_id.clone(),
                    "SYSTEM".to_string(),
                    "Urgent Alert".to_string(),
                    Some(format!("Found {} urgent deadline(s)", urgent_count)),
                    None,
                    vec!["#scheduler".to_string(), "#urgent".to_string(), "#alert".to_string()],
                ).await;
            }
            
            let logger = JobLogger::new(job_id, log_tx);
            
            if let Err(e) = check_urgent_deadlines(pool, &logger).await {
                logger.log(&format!("❌ Error checking urgent deadlines: {}", e));
                logger.set_status(crate::tui::state::JobStatus::Failed);
            } else {
                logger.set_status(crate::tui::state::JobStatus::Completed);
            }
        })
    })?).await?;

    sched.start().await?;

    // 3. REMINDER PERSONAL (H-3 JAM) - PRIVATE CHAT
    // Cek setiap 10 menit
    let pool_personal = pool.clone();
    let log_tx_personal = log_tx.clone();
    sched.add(Job::new_async("0 5/10 * * * *", move |_uuid, _l| {
        let pool = pool_personal.clone();
        let log_tx = log_tx_personal.clone();
        Box::pin(async move {
            let job_id = crate::tui::generate_job_id();
            let logger = JobLogger::new(job_id, log_tx);
            
            //logger.log("🕵️ Mengecek Personal Reminder (H-3 Jam)..."); <-- menuh"in logger gw cok
            if let Err(e) = check_personal_reminders(pool, &logger).await {
                logger.log(&format!("❌ Error personal reminder: {}", e));
                logger.set_status(crate::tui::state::JobStatus::Failed);
            } else {
                logger.set_status(crate::tui::state::JobStatus::Completed);
            }
        })
    })?).await?;
    Ok(())
}



async fn check_personal_reminders(
    pool: PgPool,
    logger: &JobLogger,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let three_hours_later = now + chrono::Duration::hours(3);

    // 1. Find tasks with deadline <= 3 hours & personal_reminder_sent = FALSE
    let tasks = sqlx::query_as::<_, crate::models::Assignment>(
        r#"
        SELECT *
        FROM assignments 
        WHERE deadline > $1 
          AND deadline <= $2 
          AND personal_reminder_sent = FALSE
        "#
    )
    .bind(now)
    .bind(three_hours_later)
    .fetch_all(&pool)
    .await?;

    if tasks.is_empty() {
        return Ok(());
    }

    logger.log(&format!("📨 Menemukan {} tugas untuk reminder personal", tasks.len()));

    use futures::stream::{self, StreamExt};

    let client = reqwest::Client::new();
    let waha_url = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string());
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());

    for task in tasks {
    
        let course_name: String = sqlx::query_scalar("SELECT name FROM courses WHERE id = $1")
            .bind(task.course_id)
            .fetch_one(&pool)
            .await?;

        if let Some(cid) = task.course_id {

            let interested_users = crud::get_users_for_course_reminder(&pool, cid).await?;
            
            let deadline_wib = task.deadline.unwrap()
                .with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap());
            let time_str = deadline_wib.format("%H:%M").to_string();

            let parallel_display = if !task.parallel_codes.is_empty() {
                format!(" {}", task.format_parallel_display())
            } else {
                String::new()
            };

            let mut recipients = Vec::new();
            
         
            for (user_id, user_codes_str) in interested_users {
                let user_codes: Vec<&str> = user_codes_str.split(',').collect();
                
                let is_match = if task.parallel_codes.is_empty() || task.parallel_codes.contains(&"all".to_string()) {
                    true 
                } else {
                    task.parallel_codes.iter().any(|task_code| {
                        let task_str = task_code.to_lowercase();
                        
                        user_codes.iter().any(|user_code| {
                            let user_str = user_code.trim().to_lowercase();
                        
                            if user_str == task_str {
                                return true;
                            }

                
                            if task_str.starts_with('r') && user_str.starts_with('p') {
                                return task_str[1..] == user_str[1..];
                            }
                            if task_str.starts_with('p') && user_str.starts_with('r') {
                                return task_str[1..] == user_str[1..];
                            }
                            
                            false
                        })
                    })
                };

                if is_match {
                    recipients.push(user_id);
                }
            }

            if recipients.is_empty() {
                continue;
            }

            logger.log(&format!("   -> Mengirim PM ke {} mahasiswa untuk tugas '{}'", recipients.len(), task.title));

            let message = format!(
                "*[PENGINGAT PRIBADI H < 3 JAM]*\n\n\
                📌 *{}*\n\
                📚 {}{}\n\
                ⏰ Deadline: Pukul *{}* WIB\n\n\
                _Segera kumpulkan jika belum!_",
                sanitize_wa_md(&task.title),
                course_name,
                parallel_display,
                time_str
            );

            let client_ref = &client;
            let waha_url_ref = &waha_url;
            let api_key_ref = &api_key;
            let message_ref = &message;

            stream::iter(recipients)
                .for_each_concurrent(10, |user_id| async move {
                    let payload = SendTextRequest {
                        chat_id: user_id,
                        text: message_ref.clone(),
                        session: "default".to_string(),
                        reply_to: None,
                        mentions:None
                    };

                    let _ = client_ref
                        .post(format!("{}/api/sendText", waha_url_ref))
                        .header("X-Api-Key", api_key_ref)
                        .json(&payload)
                        .send()
                        .await;
                    
                    // DELAYYY
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                })
                .await;
        }

        // 4. Update status 
        sqlx::query("UPDATE assignments SET personal_reminder_sent = TRUE WHERE id = $1")
            .bind(task.id)
            .execute(&pool)
            .await?;
    }

    Ok(())
}


// --- LOGIC REMINDER HARIAN ---
async fn run_reminder_task(
    pool: PgPool,
    greeting: &str,
    logger: &JobLogger,
) -> Result<(), Box<dyn std::error::Error>> {
    let assignments = crud::get_active_assignments_sorted(&pool, Some(logger)).await?;

    if assignments.is_empty() {
        logger.log("📭 Tidak ada tugas aktif, skip reminder");
        return Ok(());
    }

    let mut message = String::new();
    message.push_str("🌄*[Pengingat Tugas]*\n");
    message.push_str(greeting);
    message.push_str("\n\n");

    for (i, a) in assignments.iter().enumerate() {
        let status = status_dot(&a.deadline);
        let due_text = humanize_deadline(&a.deadline);
        let course = sanitize_wa_md(&a.first_alias);
        let title = sanitize_wa_md(&a.title);

        // Format parallel codes
        let parallel_display = if !a.parallel_codes.is_empty() {
            format!(" {}", a.format_parallel_display())
        } else {
            String::new()
        };

        message.push_str(&format!("{} *[{}]* *{}*\n", status, i + 1, title));
        message.push_str(&format!("*├─* {}\n", due_text));
        message.push_str(&format!("*└─* `#{}{}`\n", course, parallel_display));
        message.push('\n');
    }

    message.push_str("_Semangat!_ 💪");
    send_to_channels(message, logger).await
}

// REMINDER H-1 JAM  
async fn check_urgent_deadlines(
    pool: PgPool,
    logger: &JobLogger,
) -> Result<(), Box<dyn std::error::Error>> {
    use sqlx::Row;

    let now = Utc::now();
    let one_hour_later = now + chrono::Duration::hours(1);

    struct UrgentTask {
        id: uuid::Uuid,
        title: String,
        course_name: String,
        deadline: Option<DateTime<Utc>>,
        parallel_codes: Vec<String>,
    }

    let rows = sqlx::query(
        r#"
        SELECT 
            a.id, 
            a.title, 
            COALESCE(c.aliases[1], c.name) as course_name, 
            a.deadline,
            a.parallel_codes
        FROM assignments a
        JOIN courses c ON a.course_id = c.id
        WHERE a.deadline > $1 
          AND a.deadline <= $2 
          AND a.reminder_1h_sent = FALSE
        "#
    )
    .bind(now)
    .bind(one_hour_later)
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    let urgent_tasks: Vec<UrgentTask> = rows.iter().map(|row| {
        UrgentTask {
            id: row.get("id"),
            title: row.get("title"),
            course_name: row.get("course_name"),
            deadline: row.get("deadline"),
            parallel_codes: row.get("parallel_codes"),
        }
    }).collect();

    logger.log(&format!("🚨 Menemukan {} tugas deadline < 1 jam", urgent_tasks.len()));

    let mut message = String::new();

    message.push_str("*[REMINDER! H-1 JAM]*\n\n");

    for (i, task) in urgent_tasks.iter().enumerate() {
        let status = status_dot(&task.deadline);
        let due_text = humanize_deadline(&task.deadline);
        let course = sanitize_wa_md(&task.course_name);
        let title = sanitize_wa_md(&task.title);

        let parallel_display = if !task.parallel_codes.is_empty() {
            format!(" [{}]", task.parallel_codes.join(", ").to_uppercase())
        } else {
            String::new()
        };

        message.push_str(&format!("{} *[{}]* *{}*\n", status, i + 1, title));
        message.push_str(&format!("*├─* {}\n", due_text));
        message.push_str(&format!("*└─* `#{}{}`\n", course, parallel_display));
        message.push('\n');
    }
    
    message.push_str("_Segera kumpulkan!_ 🔥");

    send_to_channels(message, logger).await?;
    for task in urgent_tasks {
        sqlx::query("UPDATE assignments SET reminder_1h_sent = TRUE WHERE id = $1")
            .bind(task.id)
            .execute(&pool)
            .await?;
    }
        
    logger.log("✅ Reminder urgent gabungan berhasil dikirim.");

    Ok(())
}


// --- HELPER FUNCTIONS ---
async fn send_to_channels(
    message: String,
    logger: &JobLogger,
) -> Result<(), Box<dyn std::error::Error>> {
    let channels_env = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
    let target_channels: Vec<&str> = channels_env
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if target_channels.is_empty() {
        logger.log("⚠️ ACADEMIC_CHANNELS kosong, skip kirim");
        return Ok(());
    }

    let client = reqwest::Client::new();
    let waha_url = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string());
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    let payload = SendTextRequest {
        chat_id: target_channels[0].to_string(),
        text: message.clone(),
        session: "default".to_string(),
        reply_to: None,
        mentions: None,
    };

    let _ = client
        .post(format!("{}/api/sendText", waha_url))
        .header("X-Api-Key", &api_key)
        .json(&payload)
        .send()
        .await;
    
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
            let deadline_wib = deadline_utc.with_timezone(&wib_offset);
            let due_wib = deadline_wib.date_naive();
            let date_str = format_date_id(due_wib);
            let time_str = deadline_wib.format("%H:%M").to_string();

            let now_wib = Utc::now().with_timezone(&wib_offset);

            match delta {
                0 => {
                    let duration = deadline_wib.signed_duration_since(now_wib);
                    let hours_left = duration.num_hours();
                    
                    if hours_left > 0 {
                        let mins_left = duration.num_minutes() % 60;
                        if mins_left > 0 {
                            format!("{} jam {} menit lagi ({})", hours_left, mins_left, time_str)
                        } else {
                            format!("{} jam lagi ({})", hours_left, time_str)
                        }
                    } else if hours_left == 0 {
                        let mins_left = duration.num_minutes();
                        if mins_left > 0 {
                            format!("{} menit lagi ({})", mins_left, time_str)
                        } else {
                            format!("Baru saja lewat ({})", time_str)
                        }
                    } else {
                        format!("Lewat {} jam ({})", hours_left.abs(), time_str)
                    }
                },
                1 => format!("Besok ({})", date_str),
                d if d >= 2 => format!("H-{} ({})", d, date_str), 
                -1 => format!("Kemarin ({})", date_str),
                d => format!("lewat {} hari ({})", d.abs(), date_str),
            }
        }
        None => "_Belum ada deadline_".to_string()
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

fn sanitize_wa_md(s: &str) -> String {
    s.replace('*', "×").replace('_', " ").replace('~', "-").replace('`', "'")
}