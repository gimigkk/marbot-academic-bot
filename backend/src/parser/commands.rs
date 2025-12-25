use crate::database::crud::get_active_assignments_sorted;
use crate::models::BotCommand;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use sqlx::PgPool;

/// Handle bot commands and return response text or forward action
pub enum CommandResponse {
    Text(String),
    ForwardMessage { message_id: String, warning: String },
}

/// Handle bot commands and return response
pub async fn handle_command(
    cmd: BotCommand,
    user_phone: &str,
    chat_id: &str,
    pool: &PgPool,
) -> CommandResponse {
    match cmd {
        BotCommand::Ping => {
            println!("🏓 Ping command received from {}", user_phone);
            CommandResponse::Text("Ilkom Jaya Jaya Jaya!!!!! ✅".to_string())
        }

        BotCommand::Tugas => {
            println!("📋 Tugas command received from {}", user_phone);

            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => format_assignments_list(assignments, "📋 *Daftar Tugas*", true),
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Today => {
            println!("📅 Today command received from {}", user_phone);

            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    let today = Local::now().date_naive();
                    let today_assignments: Vec<_> = assignments
                        .into_iter()
                        .filter(|a| a.deadline.with_timezone(&Local).date_naive() == today)
                        .collect();

        
                    format_assignments_list(today_assignments, "📅 *Tugas Hari Ini*", false)
                }
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Week => {
            println!("📆 Week command received from {}", user_phone);

            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    let now = Local::now();
                    let week_end = now + Duration::days(7);

                    let week_assignments: Vec<_> = assignments
                        .into_iter()
                        .filter(|a| {
                            let d = a.deadline.with_timezone(&Local);
                            d >= now && d <= week_end
                        })
                        .collect();

                    format_assignments_list(week_assignments, "📆 *Tugas Minggu Ini (7 Hari)*", true)
                }
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Expand(index) => {
            println!(
                "🔍 Expand command for assignment {} from {} in chat {}",
                index, user_phone, chat_id
            );

            // Check if command is from academic channel group
            let academic_channels = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
            let is_academic_channel = academic_channels
                .split(',')
                .any(|channel| channel.trim() == chat_id);

            if is_academic_channel {
                return CommandResponse::Text(
                    "⚠️ _Command ini tidak boleh dijalankan di grup akademik._\nKetik command ini di chat pribadi ya."
                        .to_string(),
                );
            }

            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    let idx = (index as usize).saturating_sub(1);

                    if idx >= assignments.len() {
                        CommandResponse::Text(format!(
                            "❌ Tugas *#{}* tidak ditemukan.\nKetik *#tugas* untuk melihat daftar tugas.",
                            index
                        ))
                    } else {
                        let assignment = &assignments[idx];

                        let Some(message_id) = assignment.message_id.clone() else {
                            return CommandResponse::Text(
                                "❌ Pesan asli untuk tugas ini belum tersimpan.\nCoba cek daftar dengan *#tugas*."
                                    .to_string(),
                            );
                        };

                        let status = status_dot(&assignment.deadline);
                        let due_text = humanize_deadline(&assignment.deadline);

                        let course = sanitize_wa_md(&assignment.course_name);
                        let title = sanitize_wa_md(&assignment.title);

                        let desc_full = assignment
                            .description
                            .as_ref()
                            .map(|d| sanitize_wa_md(d))
                            .map(|d| d.trim().to_string())
                            .filter(|d| !d.is_empty())
                            .unwrap_or_else(|| "—".to_string());

                        let code_line = assignment
                            .parallel_code
                            .as_ref()
                            .map(|c| format!("\n🧩 Pararel: {}", sanitize_wa_md(c)))
                            .unwrap_or_default();

                        CommandResponse::ForwardMessage {
                            message_id,
                            warning: format!(
                                "🧾 *Detail Tugas #{}*\n\n{} *{}*\n📌 {}\n⏰ Deadline: {}\n📝 {}{}\n\n_Keterangan: 🔴 deadline 0–2 hari lagi • 🟢 deadline > 2 hari_",
                                index,
                                status,
                                course,
                                title,
                                due_text,
                                desc_full,
                                code_line
                            ),
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Done(id) => {
            println!("✅ Done command for assignment {} from {}", id, user_phone);
            // TODO: Update database (fitur selesai belum ada di repo ini)
            CommandResponse::Text(format!(
                "✅ Oke!\nTugas *#{}* akan ditandai selesai setelah fitur penyelesaian tugas diaktifkan.\n\nKetik *#tugas* untuk lihat daftar.",
                id
            ))
        }

        BotCommand::Help => {
            println!("❓ Help command received from {}", user_phone);
            CommandResponse::Text(
                "🤖 *MAA — Academic Bot*\n\n\
*Perintah penting:*\n\
• #ping — cek bot hidup\n\
• #tugas — lihat semua tugas\n\
• #today — tugas deadline hari ini\n\
• #week — tugas 7 hari ke depan\n\
• #tugas <id> | #<id> — lihat pesan aslinya\n\
• #done <id> — tandai selesai (coming soon)\n\
• #help — bantuan\n\n
_Tips: Kirim info tugas di grup akademik, nanti saya simpan otomatis._"
                    .to_string(),
            )
        }

        BotCommand::UnknownCommand(cmd) => {
            println!("❓ Unknown command '{}' from {}", cmd, user_phone);
            CommandResponse::Text(format!(
                "❓ Command tidak dikenali: *{}*\n\nKetik *#help* untuk melihat daftar command yang tersedia.",
                sanitize_wa_md(&cmd)
            ))
        }
    }
}

fn format_assignments_list(
    assignments: Vec<crate::models::AssignmentWithCourse>,
    header: &str,
    show_legend: bool,
) -> CommandResponse {
    if assignments.is_empty() {
        if show_legend {
            return CommandResponse::Text(format!(
                "{}\n\n📭 Belum ada tugas untuk periode ini.",
                header
            ));
        } else {
            
            return CommandResponse::Text(format!(
                "{}\n\n📭 Belum ada tugas untuk hari ini.",
                header
            ));
        }
    }

    let mut response = String::new();
    response.push_str(header);
    response.push('\n');

    if show_legend {
        response.push_str("\nKeterangan:\n🔴 Deadline 0–2 hari\n🟢 Deadline > 2 hari\n\n");
    } else {
        response.push('\n');
    }

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
            .map(|d| format!("📝 {}", preview_text(&d, 120)))
            .unwrap_or_default();

        let code_line = a
            .parallel_code
            .as_ref()
            .map(|c| format!("🧩 Kode: {}", sanitize_wa_md(c)))
            .unwrap_or_default();

        response.push_str(&format!("{}) {} *{}*\n", i + 1, status, course));
        response.push_str(&format!("📌 {}\n", title));
        response.push_str(&format!("⏰ Deadline: {}\n", due_text));
        if !desc_line.is_empty() {
            response.push_str(&format!("{}\n", desc_line));
        }
        if !code_line.is_empty() {
            response.push_str(&format!("{}\n", code_line));
        }
        response.push('\n');
    }

    response.push_str("_🔎 Detail: ketik #<nomor> atau #tugas <nomor> • Selesai: #done <id>_");
    CommandResponse::Text(response)
}

/// 🔴 deadline 0–2 hari lagi, 🟢 setelahnya
fn status_dot(deadline_utc: &DateTime<Utc>) -> &'static str {
    if days_left(deadline_utc) <= 2 {
        "🔴"
    } else {
        "🟢"
    }
}


fn days_left(deadline_utc: &DateTime<Utc>) -> i64 {
    let now = Local::now().date_naive();
    let due = deadline_utc.with_timezone(&Local).date_naive();
    (due - now).num_days()
}


fn humanize_deadline(deadline_utc: &DateTime<Utc>) -> String {
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


/// Format date like "26 Des 2025"
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

/// Potong text
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
