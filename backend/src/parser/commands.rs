// backend/src/commands.rs - Complete rewrite with message resending

use crate::database::crud::{
    get_active_assignments_for_user, 
    get_active_assignments_sorted, 
    mark_assignment_complete, 
    unmark_assignment_complete, 
    get_last_completed_assignment,
    delete_assignment,
    set_user_course_parallel,
    // ========== FITUR MYKELAS ==========
    get_user_course_statuses,  // Ditambahkan untuk fitur #mykelas
};

use crate::models::BotCommand;
use crate::tui::JobLogger;
use chrono::{DateTime, Duration, FixedOffset, Datelike, NaiveDate, Utc};
use sqlx::PgPool;
use std::time::Instant;

/// Command response - can be text or multiple messages to resend
pub enum CommandResponse {
    Text(String),
    ResendMessages { messages: Vec<String>, summary: String },
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
    logger: &JobLogger,
) -> CommandResponse {
    match cmd {
        BotCommand::Ping => {
            logger.log(&format!("🏓 Ping command received from {}", user_phone));
            
            let start_time = Instant::now();
            
            // 1. Database Health Check
            let db_start = Instant::now();
            let db_status = sqlx::query("SELECT 1").execute(pool).await;
            let db_duration = db_start.elapsed();
            
            let (db_icon, db_msg) = match db_status {
                Ok(_) => ("🟢", format!("{:.2?}", db_duration)),
                Err(_) => ("🔴", "Error / Disconnected".to_string()),
            };
            
            // 2. Count Active Assignments
            let assignment_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM assignments WHERE deleted_at IS NULL"
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            
            // 3. Count Active Users (users who have assignments)
            let active_users = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(DISTINCT user_phone) FROM user_assignments 
                WHERE is_completed = FALSE"
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            
            // 4. Get upcoming deadline (next task due)
            let next_deadline = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
                "SELECT deadline FROM assignments 
                WHERE deleted_at IS NULL AND deadline IS NOT NULL 
                ORDER BY deadline ASC LIMIT 1"
            )
            .fetch_one(pool)
            .await
            .ok()
            .flatten();
            
            let next_deadline_str = if let Some(deadline) = next_deadline {
                let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
                let deadline_gmt7 = deadline.with_timezone(&gmt7);
                let now_gmt7 = Utc::now().with_timezone(&gmt7);
                let hours_until = (deadline_gmt7 - now_gmt7).num_hours();
                
                if hours_until < 0 {
                    format!("⚠️ {} (overdue)", deadline_gmt7.format("%d %b, %H:%M"))
                } else if hours_until < 24 {
                    format!("🔥 {} ({} hrs)", deadline_gmt7.format("%d %b, %H:%M"), hours_until)
                } else {
                    format!("📅 {} ({} days)", deadline_gmt7.format("%d %b, %H:%M"), hours_until / 24)
                }
            } else {
                "✨ No deadlines".to_string()
            };
            
            // 5. Today's completed tasks
            let today_completed = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM user_assignments 
                WHERE is_completed = TRUE 
                AND completed_at >= CURRENT_DATE"
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            
            // 6. Gemini AI Status (check if API key is configured)
            let gemini_status = if std::env::var("GEMINI_API_KEY").is_ok() {
                "🟢 Configured"
            } else {
                "🔴 Missing"
            };
            
            // 7. Get current time in GMT+7
            let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
            let current_time = Utc::now().with_timezone(&gmt7);
            let time_str = current_time.format("%H:%M:%S WIB").to_string();
            
            // 8. Fun motivation based on time of day
            use chrono::Timelike;
            let hour = current_time.hour();
            let motivation = match hour {
                0..=5 => "🌙 Masih begadang? Semangat!",
                6..=11 => "☀️ Selamat pagi! Semoga lancar hari ini",
                12..=17 => "🌤️ Selamat siang! Jangan lupa istirahat",
                18..=21 => "🌆 Selamat sore! Deadline checks?",
                _ => "🌃 Selamat malam! Jangan begadang ya",
            };
            
            let bot_duration = start_time.elapsed();
            
            let response_text = format!(
                "🏓 *PONG!*\n\
                _{}_\n\n\
                🖥️ *System Health:*\n\
                - Bot: 🟢 Online ({:.2?})\n\
                - Database: {} {}\n\
                - AI Engine: {}\n\n\
                📊 *Live Stats:*\n\
                - Active Tasks: {}\n\
                - Active Users: {}\n\
                - Completed Today: {} ✓\n\
                - Next Deadline: {}\n\n\
                🕐 {} | _v1.0.0_",
                motivation,
                bot_duration,
                db_icon, db_msg,
                gemini_status,
                assignment_count,
                active_users,
                today_completed,
                next_deadline_str,
                time_str
            );
            
            CommandResponse::Text(response_text)
        }

        BotCommand::Tugas => {
            logger.log(&format!("📋 Tugas command received from {}", user_phone));

            match get_active_assignments_sorted(pool, Some(logger)).await {
                Ok(assignments) => format_assignments_list(assignments, "*[Daftar Tugas Aktif]*", false, false),
                Err(e) => {
                    logger.log(&format!("❌ Error fetching assignments: {}", e));
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::MyKelas => {
            logger.log(&format!("📚 MyKelas command received from {}", user_phone));
            
            match get_user_course_statuses(&pool, user_phone).await {
                Ok(statuses) => {
                    let clean_name = sanitize_wa_md(user_name);

                    if statuses.is_empty() {
                        CommandResponse::Text(
                            format!(
                                "⚙️ *SETTING KELAS* _{}_\n\n_Belum ada data mata kuliah._\n\n_Tambah: #setkelas <matkul> <kode>_", 
                                clean_name
                            )
                        )
                    } else {
                        let mut body = String::new();
                        
                        for status in statuses {
                            let matkul = sanitize_wa_md(&status.course_name);

                            match &status.parallel_code {
                                Some(code) if !code.is_empty() => {
                                  
                                    body.push_str(&format!("✅ *{}*\n", matkul));
                                    body.push_str(&format!("└ Kelas: *{}*\n", code.to_uppercase()));
                                }
                                _ => {
            
                                    body.push_str(&format!("❌ *{}*\n", matkul));
                                    body.push_str("└ Kelas: _(belum diset)_\n");
                                }
                            }
                            // Tambah baris kosong antar item biar tidak sumpek
                            body.push('\n');
                        }

                        let response = format!(
                            "⚙️ *SETTING KELAS* _{}_\n\n{}Ubah: `#setkelas <matkul> <kode>`",
                            clean_name, body
                        );

                        CommandResponse::Text(response)
                    }
                }
                Err(e) => {
                    logger.log(&format!("❌ Gagal mengambil data mykelas: {:?}", e));
                    CommandResponse::Text(
                        "❌ Terjadi kesalahan saat mengambil data kelas.".to_string()
                    )
                }
            }
        }

        BotCommand::SetKelas(matkul, codes) => {
            // codes sekarang adalah vector, misal ["k1", "p2"]
            // Join dengan spasi hanya untuk log display
            let codes_display = codes.join(" ");
            logger.log(&format!("⚙️ SetKelas command: {} [{}] from {}", matkul, codes_display, user_phone));
            
            // Panggil fungsi CRUD yang baru (passing referensi vector)
            match set_user_course_parallel(pool, user_phone, &matkul, &codes).await {
                Ok(msg) => CommandResponse::Text(msg),
                Err(e) => {
                    logger.log(&format!("❌ Error set kelas: {}", e));
                    CommandResponse::Text("❌ Gagal mengatur kelas. Terjadi kesalahan server.".to_string())
                }
            }
        }

        BotCommand::Todo => {
            logger.log(&format!("✅ Todo command received from {} ({})", user_name, user_phone));

            match get_active_assignments_for_user(pool, user_phone, Some(logger)).await {
                Ok((assignments, user_settings)) => {
                    let has_settings = !user_settings.is_empty();
                    
                    // --- UPDATE LOGIC FILTERING ---
                    let filtered_assignments: Vec<_> = assignments.into_iter().filter(|a| {
                        // 1. Kalau sudah selesai, skip 
                        if a.is_completed { return false; }
                        
                        // 2. Tugas General (all) -> AMBIL
                        if a.parallel_codes.is_empty() || a.parallel_codes.contains(&"all".to_string()) {
                            return true; 
                        }
                        
                        // 3. Cek setting user untuk matkul ini
                        if let Some(user_codes_str) = user_settings.get(&a.course_name) {
                            // user_codes_str bisa berisi "k1" atau "k1,p2"
                            // Kita pecah dulu menjadi vector
                            let user_codes: Vec<&str> = user_codes_str.split(',').collect();
                            
                            // Cek apakah ADA kode tugas yang cocok dengan kode user
                            // Misal: Tugas code "p2", User setting "k1,p2".
                            // p2 ada di dalam [k1, p2]? Ya -> Tampilkan.
                            
                            // Iterate kode tugas
                            for task_code in &a.parallel_codes {
                                if user_codes.contains(&task_code.as_str()) {
                                    return true;
                                }
                            }
                            
                            // Jika tidak ada yang cocok sama sekali
                            return false;
                        }
                        
                        // 4. Default: Jika user BELUM set kelas, tampilkan semua (biar aman)
                        true 
                    }).collect();

                    // ... sisa kode display sama ...
                    let header = format!("*[To-Do]* _{}_", user_name);
                    let mut response = format_assignments_list(filtered_assignments, &header, false, true);
                    
                    if let CommandResponse::Text(ref mut text) = response {
                         // Update pesan bantuan di bawah
                        if !has_settings {
                            text.push_str("\n\n⚠️ _Kamu belum mengatur kelas spesifik._\n_Ketik:_ `#setkelas <matkul> <k-kode> [p-kode]`\n_Contoh:_ `#setkelas pemrog k1 p2`");
                        } else {
                             text.push_str("\n\n_⚙️ Menampilkan tugas sesuai kelas kamu & tugas umum._");
                        }
                    }
                    
                    response
                }
                Err(e) => {
                    logger.log(&format!("❌ Error fetching assignments: {}", e));
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.".to_string(),
                    )
                }
            }
        }

        BotCommand::Today => {
            logger.log(&format!("📅 Today command received from {}", user_phone));

            match get_active_assignments_for_user(pool, user_phone, Some(logger)).await {
                Ok((assignments, _)) => {
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
                    logger.log(&format!("❌ Error fetching assignments: {}", e));
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Week => {
            logger.log(&format!("📆 Week command received from {}", user_phone));

            match get_active_assignments_for_user(pool, user_phone, Some(logger)).await {
                Ok((assignments, _)) => {
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
                    logger.log(&format!("❌ Error fetching assignments: {}", e));
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Expand(index) => {
            logger.log(&format!(
                "🔍 Expand command for assignment {} from {} in chat {}",
                index, user_phone, chat_id
            ));

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

            match get_active_assignments_for_user(pool, user_phone, Some(logger)).await {
                Ok((assignments, _)) => {
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

                        // Check if we have stored messages
                        if assignment.relating_messages.is_empty() {
                            return CommandResponse::Text(
                                "❌ Pesan asli untuk tugas ini belum tersimpan.\n\
                                Coba cek daftar dengan *#todo*."
                                    .to_string(),
                            );
                        }

                        let status = status_dot(&assignment.deadline);
                        let due_text = humanize_deadline(&assignment.deadline);
                        let title = sanitize_wa_md(&assignment.title);
                        let desc_full = assignment
                            .description
                            .as_ref()
                            .map(|d| sanitize_wa_md(d))
                            .map(|d| d.trim().to_string())
                            .filter(|d| !d.is_empty())
                            .unwrap_or_else(|| "—".to_string());

                        let code_line = if !assignment.parallel_codes.is_empty() {
                            format!("{}", assignment.format_parallel_display())
                        } else {
                            "null".to_string()
                        };

                        let summary = format!(
                            "*[{}]* 🧩 *{}*\n_{}_\n\n{} {}", 
                            title, 
                            code_line, 
                            desc_full, 
                            status, 
                            due_text
                        );

                        // Return messages to be resent
                        CommandResponse::ResendMessages {
                            messages: assignment.relating_messages.clone(),
                            summary,
                        }
                    }
                }
                Err(e) => {
                    logger.log(&format!("❌ Error fetching assignments: {}", e));
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n_Coba lagi sebentar ya._"
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Done(id) => {
            logger.log(&format!("✅ Done command for assignment {} from {}", id, user_phone));
            
            match get_active_assignments_for_user(pool, user_phone, Some(logger)).await {
                Ok((assignments, _)) => {
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
            logger.log(&format!("↩️  Undo command from {}", user_phone));
            
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
                    logger.log(&format!("❌ Error fetching last completed: {}", e));
                    CommandResponse::Text(
                        "❌ Gagal mengambil data tugas terakhir."
                            .to_string(),
                    )
                }
            }
        }

        BotCommand::Delete(index) => {
            logger.log(&format!("🗑️ Delete command received from {} in chat {}", user_phone, chat_id));

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

            match get_active_assignments_sorted(pool, Some(logger)).await {
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
                            logger.log(&format!("❌ DB Error on delete: {}", e));
                            CommandResponse::Text("❌ Terjadi kesalahan sistem.".to_string())
                        }
                    }
                }
                Err(e) => {
                    logger.log(&format!("❌ Error fetching list for delete: {}", e));
                    CommandResponse::Text("❌ Gagal mengambil daftar tugas.".to_string())
                }
            }
        }

        BotCommand::Help => {
            logger.log(&format!("❓ Help command received from {}", user_phone));
            CommandResponse::Text(
                "*[MABOT — Academic Bot]*\n\n\
                    *Perintah Umum:*\n\
                    - #ping — cek bot hidup & latency\n\
                    - #tugas — lihat semua tugas (global)\n\
                    - #today — tugas deadline hari ini\n\
                    - #week — tugas 7 hari ke depan\n\
                    - #help — bantuan\n\n\
                    *Perintah Personal:*\n\
                    - #todo — lihat tugas pribadi kamu\n\
                    - #<id> — lihat detail tugas dari #todo\n\
                    - #done <id> — tandai selesai\n\
                    - #undo — batalkan #done terakhir\n\n\
                    *Perintah Pengaturan:*\n\
                    - #setkelas <matkul> <kode1> [kode2]... — atur kelas pararel untuk matkul\n\
                    - #mykelas — lihat setting kelas parallel kamu\n\n\
                    *Perintah Admin (Grup Akademik):*\n\
                    - #delete <id> — hapus tugas (id dari #tugas)\n\n\
                    *Penting:* #<id> dan #done selalu pakai nomor dari *#todo*. _Info tugas akan otomatis tersimpan via grup info akademik, tidak dari chat lain._\n\n\
                    *Want to Contribute?*\n\
                    github.com/gimigkk/marbot-academic-bot"
                    .to_string(),
            )
        }

        BotCommand::MissingArgument(cmd) => {
            logger.log(&format!("⚠️ Missing argument for command '{}' from {}", cmd, user_phone));
            
            let usage_msg = match cmd.as_str() {
                "expand" => {
                    "⚠️ *Cara pakai yang benar:*\n\n\
                    #expand <nomor>\n\
                    atau cukup: #<nomor>\n\n\
                    *Contoh:*\n\
                    - #expand 1\n\
                    - #1\n\n\
                    💡 _Gunakan #todo untuk lihat daftar tugas dengan nomornya._"
                }
                "done" => {
                    "⚠️ *Cara pakai yang benar:*\n\n\
                    #done <nomor>\n\n\
                    *Contoh:*\n\
                    - #done 1\n\
                    - #done 3\n\n\
                    💡 _Gunakan #todo untuk lihat daftar tugas dengan nomornya._"
                }
                "delete" | "hapus" => {
                    "⚠️ *Cara pakai yang benar:*\n\n\
                    #delete <nomor>\n\n\
                    *Contoh:*\n\
                    - #delete 1\n\
                    - #hapus 2\n\n\
                    💡 _Gunakan #tugas untuk lihat daftar dengan nomornya._\n\
                    ⚠️ _Command ini hanya bisa dijalankan di grup akademik._"
                }
                "setkelas" => {
                    "⚠️ *Cara pakai yang benar:*\n\n\
                    #setkelas <matkul> <kode1> [kode2]...\n\n\
                    *Contoh:*\n\
                    - #setkelas pmk k1\n\
                    - #setkelas algorithm c3\n\
                    - #setkelas pemrog k1 p2\n\n\
                    💡 _Gunakan nama matkul yang benar (lihat di #tugas)_"
                }
                _ => {
                    "⚠️ Command ini membutuhkan argumen.\n\n\
                    Ketik *#help* untuk bantuan."
                }
            };
            
            CommandResponse::Text(usage_msg.to_string())
        }

        BotCommand::UnknownCommand(cmd) => {
            logger.log(&format!("❓ Unknown command '{}' from {}", cmd, user_phone));
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
        let course_alias = format!("{}", &a.first_alias);
        let title_fmt = sanitize_wa_md(&a.title);
        let due_text = humanize_deadline(&a.deadline);

        // Format parallel codes
        let parallel_display = if !a.parallel_codes.is_empty() {
            format!(" {}", a.format_parallel_display())
        } else {
            String::new()
        };

        response.push_str(&format!("{} *[{}]* *{}*\n", status_emoji, i + 1, title_fmt));
        response.push_str(&format!("*├* {}\n", due_text));
        response.push_str(&format!("*└* {}{}\n", course_alias, parallel_display));
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
        None => "⚪"
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
            let now_gmt7 = get_gmt7_now();
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
                        format!("{} jam lagi ({})", hours_left, time_str)
                    } else if hours_left == 0 {
                        let mins_left = duration.num_minutes();
                        format!("{} menit lagi ({})", mins_left, time_str)
                    } else {
                        format!("Lewat {} jam ({})", hours_left.abs(), time_str)
                    }
                },
                1 => format!("Besok ({} {})", date_str, time_str),
                d if d >= 2 => format!("H-{} ({} {})", d, date_str, time_str), 
                -1 => format!("Kemarin ({} {})", date_str, time_str),
                d => format!("lewat {} hari ({} {})", d.abs(), date_str, time_str),
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
    s.replace('*', "×")
        .replace('_', " ")
        .replace('~', "-")
        .replace('`', "'")
}