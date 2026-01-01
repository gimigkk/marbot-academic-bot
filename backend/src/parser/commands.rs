// backend/src/commands.rs - Fixed for Optional Deadline

use crate::database::crud::{
    get_active_assignments_for_user, 
    get_active_assignments_sorted, 
    mark_assignment_complete, 
    unmark_assignment_complete, 
    get_last_completed_assignment,
    delete_assignment
};
use crate::models::BotCommand;
use chrono::{DateTime, Duration, FixedOffset, Datelike, NaiveDate, Utc};
use sqlx::PgPool;
use std::time::Instant;

/// Handle bot commands and return response text or forward action
pub enum CommandResponse {
    Text(String),
    ForwardMessage { message_id: String, warning: String },
}

/// Get current time in GMT+7 (Indonesian timezone)
fn get_gmt7_now() -> DateTime<FixedOffset> {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    Utc::now().with_timezone(&gmt7)
}

/// Handle bot commands and return response
#[allow(non_snake_case)]
pub async fn handle_command(
    cmd: BotCommand,
    user_phone: &str,
    user_name: &str,
    chat_id: &str,
    pool: &PgPool,
) -> CommandResponse {
    match cmd {
        BotCommand::Ping => {
            println!("🏓 Ping command received from {}\n", user_phone);
            
            let start_time = Instant::now();
            let db_start = Instant::now();

            let db_status = sqlx::query("SELECT 1").execute(pool).await;
            let db_duration = db_start.elapsed();

            let (db_icon, db_msg) = match db_status {
                Ok(_) => ("🟢", format!("{:.2?}", db_duration)),
                Err(_) => ("🔴", "Error / Disconnected".to_string()),
            };

            let bot_duration = start_time.elapsed();

            let response_text = format!(
                "🏓 *PONG! - System Diagnostic*\n\n\
                🖥️ *Server Status:*\n\
                • Bot Logic: 🟢 Online\n\
                • Database: {} Connected\n\n\
                ⏱️ *Real-time Latency:*\n\
                • 🗄️ Database Query: {}\n\
                • ⚙️ Bot Processing: {:.2?}\n\n\
                ",
                db_icon,
                db_msg,
                bot_duration
            );

            CommandResponse::Text(response_text)
        }

        BotCommand::Tugas => {
            println!("📋 Tugas command received from {}", user_phone);

            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => format_assignments_list(assignments, "*[Daftar Tugas Aktif]*", false, false),
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Todo => {
            println!("✅ Todo command received from {}", user_phone);

            match get_active_assignments_for_user(pool, user_phone).await {
                Ok(assignments) => {
                    let header = format!("*[To-Do] User ID: {}*", user_name);
                    format_assignments_list(assignments, &header, false, true)
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

        BotCommand::Today => {
            println!("📅 Today command received from {}", user_phone);

            match get_active_assignments_for_user(pool, user_phone).await {
                Ok(assignments) => {
                    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
                    let today = get_gmt7_now().date_naive();
                    
                    let today_assignments: Vec<_> = assignments
                        .into_iter()
                        .filter(|a| {
                            if let Some(deadline) = a.deadline {
                                deadline.with_timezone(&gmt7).date_naive() == today
                            } else {
                                false
                            }
                        })
                        .collect();

                    format_assignments_list(today_assignments, "*[Tugas Hari Ini]*", false, true)
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

            match get_active_assignments_for_user(pool, user_phone).await {
                Ok(assignments) => {
                    let now = get_gmt7_now();
                    let week_end = now + Duration::days(7);

                    let week_assignments: Vec<_> = assignments
                        .into_iter()
                        .filter(|a| {
                            if let Some(deadline) = a.deadline {
                                let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
                                let d = deadline.with_timezone(&gmt7);
                                d >= now && d <= week_end
                            } else {
                                false
                            }
                        })
                        .collect();

                    format_assignments_list(week_assignments, "📆 *Tugas Minggu Ini (7 Hari)*", false, true)
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
                "🔍 Expand command for assignment {} from {} in chat {}\n",
                index, user_phone, chat_id
            );

            let academic_channels = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
            let is_academic_channel = academic_channels
                .split(',')
                .any(|channel| channel.trim() == chat_id);

            if is_academic_channel {
                return CommandResponse::Text(
                    "⚠️ _Command ini tidak boleh dijalankan di grup akademik._\n\
                    Ketik command ini di chat pribadi ya.\n\n\
                    💡 _Gunakan #todo untuk lihat daftar tugas pribadi kamu._"
                        .to_string(),
                );
            }

            match get_active_assignments_for_user(pool, user_phone).await {
                Ok(assignments) => {
                    let incomplete: Vec<_> = assignments
                        .into_iter()
                        .filter(|a| !a.is_completed)
                        .collect();

                    let idx = (index as usize).saturating_sub(1);

                    if idx >= incomplete.len() {
                        CommandResponse::Text(format!(
                            "❌ Tugas *#{}* tidak ditemukan di to-do list kamu.\n\n\
                            💡 _Tip: Ketik #todo untuk lihat daftar tugas._",
                            index
                        ))
                    } else {
                        let assignment = &incomplete[idx];

                        let Some(message_id) = assignment.message_ids.last().cloned() else {
                            return CommandResponse::Text(
                                "❌ Pesan asli untuk tugas ini belum tersimpan.\n\
                                Coba cek daftar dengan *#todo*."
                                    .to_string(),
                            );
                        };

                        let status = status_dot(&assignment.deadline);
                        let done_status = if assignment.is_completed { 
                            "✅ SUDAH SELESAI" 
                        } else { 
                            "⬜ BELUM SELESAI" 
                        };
                        
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
                                "🧾 *Detail Tugas #{}*\nStatus: {}\n\n{} *{}*\n📌 {}\n⏰ Deadline: {}\n📝 {}{}\n\n\
                                _Keterangan: 🔴 deadline 0–2 hari lagi • 🟢 deadline > 2 hari_",
                                index,
                                done_status,
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
            println!("✅ Done command for assignment {} from {}\n", id, user_phone);
            
            match get_active_assignments_for_user(pool, user_phone).await {
                Ok(assignments) => {
                    let incomplete: Vec<_> = assignments
                        .into_iter()
                        .filter(|a| !a.is_completed)
                        .collect();

                    let idx = (id as usize).saturating_sub(1);
                    
                    if idx >= incomplete.len() {
                        return CommandResponse::Text(format!(
                            "❌ Tugas nomor *{}* tidak ditemukan di to-do list kamu.\n\n\
                            💡 _Tip: Ketik #todo untuk lihat daftar tugas._",
                            id
                        ));
                    }
                    
                    let assignment = &incomplete[idx];
                    
                    match mark_assignment_complete(pool, assignment.id, user_phone).await {
                        Ok(_) => CommandResponse::Text(format!(
                            "✅ Mantap! Tugas *{}* selesai.\n\n\
                            _Salah tandai? Ketik #undo_",
                            sanitize_wa_md(&assignment.title)
                        )),
                        Err(e) => CommandResponse::Text(format!("❌ Database error: {}", e))
                    }
                }
                Err(e) => CommandResponse::Text(format!("❌ Gagal mengambil data: {}", e))
            }
        }

        BotCommand::Undo => {
            println!("↩️  Undo command from {}\n", user_phone);
            
            match get_last_completed_assignment(pool, user_phone).await {
                Ok(Some(assignment)) => {
                    match unmark_assignment_complete(pool, assignment.id, user_phone).await {
                        Ok(_) => CommandResponse::Text(format!(
                            "↩️ Oke! Tugas *{}* ditandai belum selesai.\n\n\
                            _Ketik #todo untuk lihat daftar terbaru._",
                            sanitize_wa_md(&assignment.title)
                        )),
                        Err(e) => CommandResponse::Text(format!("❌ Database error: {}", e))
                    }
                }
                Ok(None) => {
                    CommandResponse::Text(
                        "❌ Tidak ada tugas yang baru saja kamu selesaikan.\n\n\
                        💡 _#undo hanya bisa membatalkan tugas terakhir yang kamu tandai selesai._"
                            .to_string(),
                    )
                }
                Err(e) => {
                    eprintln!("❌ Error fetching last completed: {}", e);
                    CommandResponse::Text(
                        "❌ Gagal mengambil data tugas terakhir."
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Delete(index) => {
            println!("🗑️ Delete command received from {} in chat {}", user_phone, chat_id);

            let academic_channels = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
            let is_authorized = academic_channels
                .split(',')
                .map(|s| s.trim())
                .any(|allowed_id| allowed_id == chat_id);

            if !is_authorized {
                return CommandResponse::Text(
                    "⛔ *AKSES DITOLAK*\n\n\
                    Fitur hapus hanya boleh dilakukan di Grup Official/Academic Channel oleh PJ Matkul.\n\
                    _Jangan iseng ya!_ 👮"
                        .to_string(),
                );
            }

            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    let idx = (index as usize).saturating_sub(1);

                    if idx >= assignments.len() {
                        return CommandResponse::Text(format!(
                            "❌ Tugas nomor *{}* tidak ditemukan.\nCek nomor terbaru dengan *#tugas*",
                            index
                        ));
                    }

                    let target_assignment = &assignments[idx];
                    let title = sanitize_wa_md(&target_assignment.title);
                    let course = sanitize_wa_md(&target_assignment.course_name);
                    let assignment_id = target_assignment.id;

                    match delete_assignment(pool, assignment_id).await {
                        Ok(true) => {
                            CommandResponse::Text(format!(
                                "🗑️ *TUGAS DIHAPUS*\n\n\
                                Mata Kuliah: {}\n\
                                Judul: {}\n\n\
                                _Tugas berhasil dihapus dari database._",
                                course, title
                            ))
                        },
                        Ok(false) => CommandResponse::Text("❌ Gagal menghapus. Tugas mungkin sudah hilang.".to_string()),
                        Err(e) => {
                            eprintln!("❌ DB Error on delete: {}", e);
                            CommandResponse::Text("❌ Terjadi kesalahan sistem.".to_string())
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Error fetching list for delete: {}", e);
                    CommandResponse::Text("❌ Gagal mengambil daftar tugas.".to_string())
                }
            }
        }

        BotCommand::Help => {
            println!("❓ Help command received from {}\n", user_phone);
            CommandResponse::Text(
                "*[MABOT — Academic Bot]*\n\n\
*Perintah Umum:*\n\
• #ping — cek bot hidup & latency\n\
• #tugas — lihat semua tugas (global)\n\
• #today — tugas deadline hari ini\n\
• #week — tugas 7 hari ke depan\n\
• #help — bantuan\n\n\
*Perintah Personal:*
• #todo — lihat tugas pribadi kamu\n\
• #<id> — lihat detail tugas dari #todo\n\
• #done <id> — tandai selesai\n\
• #undo — batalkan #done terakhir\n\n\
*Perintah Admin (Grup Akademik):*\n\
• #delete <id> — hapus tugas (id dari #tugas)\n\n\
*Penting:* #<id> dan #done selalu pakai nomor dari *#todo*. _Info tugas akan otomatis tersimpan via grup info akademik, tidak dari chat lain._

*Want to Contribute?*
github.com/gimigkk/marbot-academic-bot"
                    .to_string(),
            )
        }

        BotCommand::UnknownCommand(cmd) => {
            println!("❓ Unknown command '{}' from {}\n", cmd, user_phone);
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
    user_specific: bool,
) -> CommandResponse {
    let filtered_assignments: Vec<_> = if user_specific {
        assignments.into_iter().filter(|a| !a.is_completed).collect()
    } else {
        assignments
    };

    if filtered_assignments.is_empty() {
        if user_specific {
            return CommandResponse::Text(format!(
                "{}\n\n🎉 *Selamat!* Semua tugas sudah selesai!\n✨ _Kamu keren banget!_",
                header
            ));
        } else if show_legend {
            return CommandResponse::Text(format!(
                "{}\n\n📭 Belum ada tugas untuk periode ini.",
                header
            ));
        } else {
            return CommandResponse::Text(format!(
                "{}\n\n📭 Belum ada tugas.",
                header
            ));
        }
    }

    let mut response = String::new();
    response.push_str(header);
    response.push('\n');

    if show_legend {
        response.push_str("\nKeterangan:\n🔴 Deadline 0–2 hari\n🟢 Deadline > 2 hari\n⚪ Belum ada deadline\n\n");
    } else {
        response.push('\n');
    }

    for (i, a) in filtered_assignments.iter().enumerate() {
        let status_emoji = status_dot(&a.deadline);
        let title_fmt = format!("{}", preview_text(&sanitize_wa_md(&a.title), 25));
        let due_text = humanize_deadline(&a.deadline);
        let course = sanitize_wa_md(&a.course_name);

        let desc_line = a
            .description
            .as_ref()
            .map(|d| sanitize_wa_md(d))
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .map(|d| format!("📝 {}", preview_text(&d, 25)))
            .unwrap_or_default();

        let code_line = a
            .parallel_code
            .as_ref()
            .map(|c| format!("🧩 Kode: {}", sanitize_wa_md(c)))
            .unwrap_or_default();

        response.push_str(&format!("{} *[{}] [{}]*\n", status_emoji, i + 1, title_fmt));
        response.push_str(&format!("📌 {}\n", course));
        response.push_str(&format!("⏰ Deadline: {}\n", due_text));
        
        if !desc_line.is_empty() {
            response.push_str(&format!("{}\n", desc_line));
        }
        if !code_line.is_empty() {
            response.push_str(&format!("{}\n", code_line));
        }
        response.push('\n');
    }

    if user_specific {
        response.push_str("\n_🔎 Detail: #<nomor>_\n_✅ Selesai: #done <nomor>_");
    } else {
        response.push_str("\n_💡 Gunakan #todo untuk list personal_");
    }
    
    CommandResponse::Text(response)
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
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = get_gmt7_now().date_naive();
    let due = deadline_utc.with_timezone(&gmt7).date_naive();
    (due - now).num_days()
}

#[allow(non_snake_case)]
fn humanize_deadline(deadline: &Option<DateTime<Utc>>) -> String {
    match deadline {
        Some(deadline_utc) => {
            let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
            let deadline_gmt7 = deadline_utc.with_timezone(&gmt7);
            let now = get_gmt7_now().date_naive();
            let due = deadline_gmt7.date_naive();
            
            let delta = (due - now).num_days();
            let date_str = format_date_id(due);
            let time_str = deadline_gmt7.format("%d-%m %H:%M").to_string();

            match delta {
                0 => format!("Hari ini ({} {})", date_str, time_str),
                1 => format!("Besok ({} {})", date_str, time_str),
                d if d >= 2 => format!("H-{} ({} {})", d, date_str, time_str), 
                -1 => format!("Kemarin ({} {})", date_str, time_str),
                d => format!("lewat {} hari ({} {})", d.abs(), date_str, time_str),
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