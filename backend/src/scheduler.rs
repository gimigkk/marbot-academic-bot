use crate::database::crud::{self, get_active_assignments_for_user, get_daily_subscribers};
use crate::models::SendTextRequest;
use crate::tui::{state::LogEntry, JobLogger};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use sqlx::PgPool;
// use tokio::time::sleep;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};

/// Registers all scheduled jobs and starts the cron scheduler.
/// All jobs must be added before calling `sched.start()`.
pub async fn start_scheduler(
    pool: PgPool,
    log_tx: mpsc::UnboundedSender<LogEntry>,
) -> Result<(), JobSchedulerError> {
    let sched = JobScheduler::new().await?;

    let tui_state = crate::TUI_STATE.get().cloned();

    // 1. REMINDER HARIAN (GLOBAL + PERSONAL)
    // 07:00 WIB = 00:00 UTC
    let pool_pagi = pool.clone();
    let log_tx_pagi = log_tx.clone();
    let tui_state_pagi = tui_state.clone();

    sched
        .add(Job::new_async("0 30 20 * * *", move |_uuid, _l| {
            let pool = pool_pagi.clone();
            let log_tx = log_tx_pagi.clone();
            let tui_state = tui_state_pagi.clone();

            Box::pin(async move {
                let job_id = crate::tui::generate_job_id();

                if let Some(tui) = &tui_state {
                    tui.create_job(
                        job_id.clone(),
                        "SYSTEM".to_string(),
                        "Morning Routine".to_string(), // Ganti nama biar general
                        Some("Global & Personal Reminder (03:00 WIB)".to_string()),
                        None,
                        vec![
                            "#scheduler".to_string(),
                            "#daily".to_string(),
                            "#morning".to_string(),
                        ],
                    )
                    .await;
                }

                let logger = JobLogger::new(job_id, log_tx);

                logger.log("⏰ MEMULAI SAHUR ROUTINE (03:30 WIB)");

                logger.log("📡 Mengirim Reminder Global...");
                if let Err(e) =
                    run_reminder_task(pool.clone(), "_Selamat sahur Ilkomers!_", &logger).await
                {
                    logger.log(&format!("❌ Error reminder global: {}", e));
                }

                // REMINDER PERSONAL
                logger.log("🚀 Menjalankan Personal Daily Reminder...");
                if let Err(e) = run_personal_daily_reminder(pool, &logger).await {
                    logger.log(&format!("❌ Error personal daily reminder: {}", e));
                }

                logger.set_status(crate::tui::state::JobStatus::Completed);
            })
        })?)
        .await?;

    // 2. REMINDER DEADLINE MEPET (H-1 JAM)
    // Cek setiap 10 menit
    let pool_urgent = pool.clone();
    let log_tx_urgent = log_tx.clone();
    let tui_state_urgent = tui_state.clone();

    sched
        .add(Job::new_async("0 1/10 * * * *", move |_uuid, _l| {
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
                "#,
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
                        vec![
                            "#scheduler".to_string(),
                            "#urgent".to_string(),
                            "#alert".to_string(),
                        ],
                    )
                    .await;
                }

                let logger = JobLogger::new(job_id, log_tx);

                if let Err(e) = check_urgent_deadlines(pool, &logger).await {
                    logger.log(&format!("❌ Error checking urgent deadlines: {}", e));
                    logger.set_status(crate::tui::state::JobStatus::Failed);
                } else {
                    logger.set_status(crate::tui::state::JobStatus::Completed);
                }
            })
        })?)
        .await?;

    // 3. REMINDER PERSONAL (H-3 JAM) - PRIVATE CHAT
    // Cek setiap 10 menit
    let pool_personal = pool.clone();
    let log_tx_personal = log_tx.clone();
    sched
        .add(Job::new_async("0 5/10 * * * *", move |_uuid, _l| {
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
        })?)
        .await?;

    // REMINDER IFTAR LOGIC
    // Cek setiap menit. `last_sent` mencegah pengiriman ganda dalam satu hari.
    let log_tx_iftar = log_tx.clone();
    let iftar_last_sent: Arc<Mutex<Option<NaiveDate>>> = Arc::new(Mutex::new(None));

    sched
        .add(Job::new_async("0 * * * * *", move |_uuid, _l| {
            let last_sent = iftar_last_sent.clone();
            let log_tx = log_tx_iftar.clone();
            Box::pin(async move {
                check_iftar(last_sent, log_tx).await;
            })
        })?)
        .await?;

    sched.start().await?;
    Ok(())
}

// FITUR SPECIAL RAMADHAN
/// Cek setiap menit apakah waktu maghrib sudah tiba berdasarkan `ramadhan_dramaga.json`.
/// Job entry di Dashboard hanya dibuat saat pesan benar-benar dikirim.
/// `last_sent` di-flip ke hari ini hanya setelah pengiriman sukses.
async fn check_iftar(
    last_sent: Arc<Mutex<Option<NaiveDate>>>,
    log_tx: mpsc::UnboundedSender<LogEntry>,
) {
    static SCHEDULE_JSON: &str = include_str!("../ramadhan_dramaga.json");

    let wib_offset = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    let now_utc = Utc::now();
    let now_wib = now_utc.with_timezone(&wib_offset);
    let today = now_wib.date_naive();

    {
        let guard = last_sent.lock().unwrap();
        if *guard == Some(today) {
            return;
        }
    }

    let schedule: HashMap<String, String> = match serde_json::from_str(SCHEDULE_JSON) {
        Ok(s) => s,
        Err(_) => return,
    };

    #[allow(non_snake_case)]
    let maghrib_time = match schedule.get(&today.format("%Y-%m-%d").to_string()) {
        Some(t) => t.clone(),
        None => return, // Bukan hari Ramadhan, silent return
    };

    let time_parts: Vec<&str> = maghrib_time.split(':').collect();
    if time_parts.len() != 2 {
        return;
    }

    let h: u32 = time_parts[0].parse().unwrap_or(0);
    let m: u32 = time_parts[1].parse().unwrap_or(0);

    let maghrib_utc = today
        .and_hms_opt(h, m, 0)
        .unwrap()
        .and_local_timezone(wib_offset)
        .unwrap()
        .with_timezone(&Utc);

    // Fire dalam menit yang sama dengan maghrib
    let diff_secs = (now_utc - maghrib_utc).num_seconds();
    if diff_secs < 0 || diff_secs > 59 {
        return;
    }

    let job_id = crate::tui::generate_job_id();
    if let Some(tui) = crate::TUI_STATE.get() {
        tui.create_job(
            job_id.clone(),
            "SYSTEM".to_string(),
            "Ramadhan Iftar".to_string(),
            Some(format!("Buka puasa pukul {} WIB", maghrib_time)),
            None,
            vec![
                "#scheduler".to_string(),
                "#ramadhan".to_string(),
                "#iftar".to_string(),
            ],
        )
        .await;
    }

    let logger = JobLogger::new(job_id, log_tx);
    logger.log(&format!("🕌 Waktunya Buka Puasa! Maghrib: {} WIB", maghrib_time));

    let message = String::from(
        "🕌 *Selamat berbuka puasa Ilkomerz!* @all\n\n\
        Telah masuk waktu Maghrib untuk wilayah *Dramaga, Bogor* dan sekitarnya. اللَّهُمَّ لَكَ صُمْتُ، وَعَلَى رِزْقِكَ أَفْطَرْتُ",
    );

    if let Err(e) = send_to_channels(message, &logger).await {
        logger.log(&format!("❌ Gagal mengirim reminder iftar: {}", e));
        logger.set_status(crate::tui::state::JobStatus::Failed);
        return;
    }

    // Tandai sudah terkirim hari ini, hanya setelah sukses
    *last_sent.lock().unwrap() = Some(today);
    logger.log("✅ Reminder iftar berhasil dikirim.");
    logger.set_status(crate::tui::state::JobStatus::Completed);
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
        "#,
    )
    .bind(now)
    .bind(three_hours_later)
    .fetch_all(&pool)
    .await?;

    if tasks.is_empty() {
        return Ok(());
    }

    logger.log(&format!(
        "📨 Menemukan {} tugas untuk reminder personal",
        tasks.len()
    ));

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
            let interested_users = crud::get_users_for_course_reminder(&pool, cid, task.id).await?;

            let deadline_wib = task
                .deadline
                .unwrap()
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

                let is_match = if task.parallel_codes.is_empty()
                    || task.parallel_codes.contains(&"all".to_string())
                {
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

            logger.log(&format!(
                "   -> Mengirim PM ke {} mahasiswa untuk tugas '{}'",
                recipients.len(),
                task.title
            ));

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
                        mentions: None,
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
        "#,
    )
    .bind(now)
    .bind(one_hour_later)
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    let urgent_tasks: Vec<UrgentTask> = rows
        .iter()
        .map(|row| UrgentTask {
            id: row.get("id"),
            title: row.get("title"),
            course_name: row.get("course_name"),
            deadline: row.get("deadline"),
            parallel_codes: row.get("parallel_codes"),
        })
        .collect();

    logger.log(&format!(
        "🚨 Menemukan {} tugas deadline < 1 jam",
        urgent_tasks.len()
    ));

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
        None => "⚪",
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
            let gmt7 = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
            let deadline_gmt7 = deadline_utc.with_timezone(&gmt7);

            // Menggunakan waktu sekarang GMT+7
            let now_gmt7 = Utc::now().with_timezone(&gmt7);
            let now = now_gmt7.date_naive();
            let due = deadline_gmt7.date_naive();

            let delta = (due - now).num_days();
            let date_str = format_date_id(due);
            let time_str = deadline_gmt7.format("%H:%M").to_string();

            match delta {
                0 => {
                    let duration = deadline_gmt7.signed_duration_since(now_gmt7);
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
                }
                1 => format!("Besok ({} {})", date_str, time_str),

                d if d >= 2 => {
                    let day_name = get_day_name_id(due);
                    format!("H-{} ({}, {} {})", d, day_name, date_str, time_str)
                }

                -1 => format!("Kemarin ({} {})", date_str, time_str),
                d => format!("lewat {} hari ({} {})", d.abs(), date_str, time_str),
            }
        }
        None => "_Belum ada deadline_".to_string(),
    }
}

fn format_date_id(date: NaiveDate) -> String {
    let day = date.day();
    let month = match date.month() {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "Mei",
        6 => "Jun",
        7 => "Jul",
        8 => "Agu",
        9 => "Sep",
        10 => "Okt",
        11 => "Nov",
        12 => "Des",
        _ => "???",
    };
    format!("{} {} {}", day, month, date.year())
}

fn get_day_name_id(date: NaiveDate) -> &'static str {
    match date.weekday() {
        chrono::Weekday::Mon => "Sen",
        chrono::Weekday::Tue => "Sel",
        chrono::Weekday::Wed => "Rab",
        chrono::Weekday::Thu => "Kam",
        chrono::Weekday::Fri => "Jum",
        chrono::Weekday::Sat => "Sab",
        chrono::Weekday::Sun => "Min",
    }
}

fn sanitize_wa_md(s: &str) -> String {
    s.replace('*', "×")
        .replace('_', " ")
        .replace('~', "-")
        .replace('`', "'")
}

async fn run_personal_daily_reminder(
    pool: PgPool,
    logger: &JobLogger,
) -> Result<(), Box<dyn std::error::Error>> {
    let subscribers = get_daily_subscribers(&pool).await?;

    if subscribers.is_empty() {
        logger.log("ℹ️ Tidak ada subscriber daily reminder.");
        return Ok(());
    }

    logger.log(&format!(
        "📨 Mengirim daily summary ke {} user...",
        subscribers.len()
    ));

    let client = reqwest::Client::new();
    let waha_url = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string());
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());

    for user_phone in subscribers {
        match get_active_assignments_for_user(&pool, &user_phone, None).await {
            Ok((assignments, user_settings)) => {
                let filtered_assignments: Vec<_> = assignments
                    .into_iter()
                    .filter(|a| {
                        if a.is_completed {
                            return false;
                        }

                        if a.parallel_codes.is_empty()
                            || a.parallel_codes.contains(&"all".to_string())
                        {
                            return true;
                        }

                        if let Some(user_codes_str) = user_settings.get(&a.course_name) {
                            let user_codes: Vec<&str> = user_codes_str.split(',').collect();
                            for task_code in &a.parallel_codes {
                                let task_str = task_code.as_str();
                                if user_codes.contains(&task_str) {
                                    return true;
                                }

                                // Handle p/r variants
                                if task_str.starts_with('r') {
                                    let p_variant = format!("p{}", &task_str[1..]);
                                    if user_codes.contains(&p_variant.as_str()) {
                                        return true;
                                    }
                                } else if task_str.starts_with('p') {
                                    let r_variant = format!("r{}", &task_str[1..]);
                                    if user_codes.contains(&r_variant.as_str()) {
                                        return true;
                                    }
                                }
                            }
                            return false;
                        }
                        true
                    })
                    .collect();

                if filtered_assignments.is_empty() {
                    continue;
                }

                let mut message = String::new();
                message.push_str(
                    "🌞 *[Daily Reminder]*\n_Semangat pagi! Ini daftar tugas kamu:_\n\n",
                );

                for (i, a) in filtered_assignments.iter().enumerate() {
                    let status_emoji = status_dot(&a.deadline);
                    let due_text = humanize_deadline(&a.deadline);
                    let title = sanitize_wa_md(&a.title);
                    let course = sanitize_wa_md(&a.first_alias);

                    let parallel_display = if !a.parallel_codes.is_empty() {
                        format!(" {}", a.format_parallel_display())
                    } else {
                        String::new()
                    };

                    message.push_str(&format!("{} *[{}]* *{}*\n", status_emoji, i + 1, title));
                    message.push_str(&format!("*├─* {}\n", due_text));
                    message.push_str(&format!("*└─* `#{}{}`\n", course, parallel_display));
                    message.push('\n');
                }

                message.push_str("\n_#daily 0 untuk berhenti langganan._");

                let payload = SendTextRequest {
                    chat_id: user_phone.clone(),
                    text: message,
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

                // Delay
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
            Err(e) => {
                logger.log(&format!("⚠️ Error fetch todo user {}: {}", user_phone, e));
            }
        }
    }

    Ok(())
}