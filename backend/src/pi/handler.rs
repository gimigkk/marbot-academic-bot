use sqlx::PgPool;
use chrono::{NaiveDateTime, Datelike};
use crate::tui::JobLogger;
use super::models::{NewPiTask, PiAIExtraction};
use super::crud::create_pi_task;
use super::prompts::build_pi_extraction_prompt;

// HELPER: Kirim Pesan ke WAHA dengan Typing State & Artificial Delay
async fn send_reply(chat_id: &str, text: &str) -> Result<(), String> {
    crate::waha::send_reply(chat_id, text).await
}

fn format_pi_tasks(tasks: Vec<super::models::PiTask>) -> String {
    use chrono::Datelike;
    let gmt7 = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    let now_gmt7 = chrono::Utc::now().with_timezone(&gmt7).naive_local();

    // Filter out past deadline tasks
    let active_tasks: Vec<_> = tasks.into_iter().filter(|t| {
        let duration = t.deadline.signed_duration_since(now_gmt7);
        duration.num_minutes() >= 0
    }).collect();

    if active_tasks.is_empty() {
        return "*[Daftar Tugas Pekan Ilkomerz]*\n\n📭 Belum ada tugas untuk periode ini.".to_string();
    }

    let mut response = String::from("*[Daftar Tugas Pekan Ilkomerz]*\n\n");
    let now = now_gmt7.date();

    for (i, task) in active_tasks.iter().enumerate() {
        let duration = task.deadline.signed_duration_since(now_gmt7);
        let due = task.deadline.date();
        let delta_days = (due - now).num_days();
        
        let status_emoji = if duration.num_minutes() <= 24 * 60 {
            "🔴"
        } else if duration.num_minutes() <= 48 * 60 {
            "🟠"
        } else if duration.num_minutes() <= 72 * 60 {
            "🟡"
        } else {
            "🟢"
        };

        let day = due.day();
        let month = match due.month() {
            1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr", 5 => "Mei", 6 => "Jun",
            7 => "Jul", 8 => "Agu", 9 => "Sep", 10 => "Okt", 11 => "Nov", 12 => "Des",
            _ => "???",
        };
        let date_str = format!("{} {} {}", day, month, due.year());
        let time_str = task.deadline.format("%H:%M").to_string();

        let day_name = match due.weekday() {
            chrono::Weekday::Mon => "Sen", chrono::Weekday::Tue => "Sel",
            chrono::Weekday::Wed => "Rab", chrono::Weekday::Thu => "Kam",
            chrono::Weekday::Fri => "Jum", chrono::Weekday::Sat => "Sab",
            chrono::Weekday::Sun => "Min",
        };

        let due_text = match delta_days {
            0 => {
                let hours_left = duration.num_hours();
                if hours_left > 0 {
                    let mins_left = duration.num_minutes() % 60;
                    if mins_left > 0 {
                        format!("{} jam {} menit lagi ({})", hours_left, mins_left, time_str)
                    } else {
                        format!("{} jam lagi ({})", hours_left, time_str)
                    }
                } else {
                    let mins_left = duration.num_minutes();
                    if mins_left > 0 {
                        format!("{} menit lagi ({})", mins_left, time_str)
                    } else {
                        format!("Segera deadline ({})", time_str)
                    }
                }
            },
            1 => format!("Besok ({} {})", date_str, time_str),
            d => format!("H-{} ({}, {} {})", d, day_name, date_str, time_str),
        };

        response.push_str(&format!("{} *[{}]* *{}*\n", status_emoji, i + 1, task.nama_tugas));
        response.push_str(&format!("*└─* {}\n\n", due_text));
    }

    response.trim_end().to_string()
}

// HELPER: Panggil AI (Groq API) untuk Ekstraksi JSON
async fn call_ai_extraction(prompt: &str) -> Result<PiAIExtraction, String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY tidak ditemukan di .env".to_string())?;

    let url = "https://api.groq.com/openai/v1/chat/completions";
    
    let request_body = serde_json::json!({
        "model": "llama-3.3-70b-versatile", // Gunakan model Groq yang cepat
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.1,
        "response_format": { "type": "json_object" } 
    });

    let client = reqwest::Client::new();
    let response = client.post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        let json_resp: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        
        // Ambil string JSON dari respon AI
        let content = json_resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("{}");
            
        // Parse string JSON tersebut ke struct PiAIExtraction
        let extraction: PiAIExtraction = serde_json::from_str(content)
            .map_err(|e| format!("Gagal parsing JSON dari AI: {}", e))?;
            
        Ok(extraction)
    } else {
        Err(format!("AI API Error HTTP: {}", response.status()))
    }
}


// MAIN HANDLER: Proses Pesan 
pub async fn process_pi_message(
    pool: &PgPool,
    message_body: &str,
    chat_id: &str,
    logger: &JobLogger,
) {
    let lower_body = message_body.trim().to_lowercase();

    // INTERCEPT: #tugas command
    if lower_body.starts_with("#tugas") || lower_body.starts_with("#tgs") {
        logger.log("📋 Command #tugas terdeteksi untuk Pekan Ilkomerz!");
        match super::crud::get_upcoming_pi_tasks(pool).await {
            Ok(tasks) => {
                let msg = format_pi_tasks(tasks);
                let _ = send_reply(chat_id, &msg).await;
                logger.set_status(crate::tui::state::JobStatus::Completed);
            }
            Err(e) => {
                logger.log(&format!("❌ Gagal mengambil tugas PI: {}", e));
                let _ = send_reply(chat_id, "❌ Gagal mengambil daftar tugas PI.").await;
                logger.set_status(crate::tui::state::JobStatus::Failed);
            }
        }
        return;
    }

    // INTERCEPT: #help command
    if lower_body.starts_with("#help") {
        logger.log("❓ Command #help terdeteksi untuk Pekan Ilkomerz!");
        let help_msg = "*[Bantuan Pekan Ilkomerz]*\n\n\
            *Perintah yang tersedia:*\n\
            - #tugas / #tgs — lihat daftar tugas PI aktif\n\
            - #delete <nomor> — hapus tugas (nomor dari #tugas)\n\
            - #update <nomor> <pesan> — perbarui tugas (nama/deadline)\n\
            - #help — tampilkan pesan bantuan ini\n\n\
            *Menambah Tugas Baru:*\n\
            Tag *#marbot* dan tulis pesan berisi tugas dan deadlinenya.\n\
            Contoh: _#marbot jangan lupa bikin proposal pi kumpul besok_\n\
            Kalau ada bug atau error bisa hubungin arya/gilang";
        let _ = send_reply(chat_id, help_msg).await;
        logger.set_status(crate::tui::state::JobStatus::Completed);
        return;
    }

    // INTERCEPT: #delete command
    if lower_body.starts_with("#delete") {
        logger.log("🗑️ Command #delete terdeteksi untuk Pekan Ilkomerz!");
        let parts: Vec<&str> = message_body.split_whitespace().collect();
        if parts.len() < 2 {
            let _ = send_reply(chat_id, "❌ Format salah.\n\n💡 *Cara penggunaan:*\n#delete <nomor_tugas>").await;
            logger.set_status(crate::tui::state::JobStatus::Failed);
            return;
        }

        let task_idx: usize = match parts[1].parse::<usize>() {
            Ok(idx) if idx > 0 => idx - 1,
            _ => {
                let _ = send_reply(chat_id, "❌ Nomor tugas tidak valid. Gunakan angka dari *#tugas*.").await;
                logger.set_status(crate::tui::state::JobStatus::Failed);
                return;
            }
        };

        match super::crud::get_upcoming_pi_tasks(pool).await {
            Ok(tasks) => {
                let gmt7 = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
                let now_gmt7 = chrono::Utc::now().with_timezone(&gmt7).naive_local();
                
                let active_tasks: Vec<_> = tasks.into_iter().filter(|t| {
                    let duration = t.deadline.signed_duration_since(now_gmt7);
                    duration.num_minutes() >= 0
                }).collect();

                if task_idx >= active_tasks.len() {
                    let _ = send_reply(chat_id, &format!("❌ Tugas nomor *{}* tidak ditemukan.\n\n💡 _Cek nomor terbaru dengan #tugas_", task_idx + 1)).await;
                    logger.set_status(crate::tui::state::JobStatus::Failed);
                    return;
                }

                let target_task = &active_tasks[task_idx];

                match super::crud::delete_pi_task(pool, target_task.id).await {
                    Ok(rows_affected) if rows_affected > 0 => {
                        let msg = format!("🗑️ *TUGAS DIHAPUS*\n\n✅ Tugas *{}* berhasil dihapus.", target_task.nama_tugas);
                        let _ = send_reply(chat_id, &msg).await;
                        logger.set_status(crate::tui::state::JobStatus::Completed);
                    }
                    Ok(_) => {
                        let _ = send_reply(chat_id, "❌ Gagal menghapus. Tugas mungkin sudah hilang.").await;
                        logger.set_status(crate::tui::state::JobStatus::Failed);
                    }
                    Err(e) => {
                        logger.log(&format!("❌ Gagal hapus tugas PI: {}", e));
                        let _ = send_reply(chat_id, "❌ Gagal menghapus tugas dari database.").await;
                        logger.set_status(crate::tui::state::JobStatus::Failed);
                    }
                }
            }
            Err(e) => {
                logger.log(&format!("❌ Gagal mengambil tugas PI: {}", e));
                let _ = send_reply(chat_id, "❌ Gagal mengambil daftar tugas untuk dihapus.").await;
                logger.set_status(crate::tui::state::JobStatus::Failed);
            }
        }
        return;
    }

    // INTERCEPT: #update command
    if lower_body.starts_with("#update") {
        logger.log("🔄 Command #update terdeteksi untuk Pekan Ilkomerz!");
        let parts: Vec<&str> = message_body.splitn(3, ' ').collect();
        if parts.len() < 3 {
            let _ = send_reply(chat_id, "❌ Format salah.\n\n💡 *Cara penggunaan:*\n#update <nomor_tugas> <pesan_baru>").await;
            logger.set_status(crate::tui::state::JobStatus::Failed);
            return;
        }

        let task_idx: usize = match parts[1].parse::<usize>() {
            Ok(idx) if idx > 0 => idx - 1,
            _ => {
                let _ = send_reply(chat_id, "❌ Nomor tugas tidak valid. Gunakan angka dari *#tugas*.").await;
                logger.set_status(crate::tui::state::JobStatus::Failed);
                return;
            }
        };

        let update_msg = parts[2].trim();

        // 1. Ambil tugas berdasarkan index
        match super::crud::get_upcoming_pi_tasks(pool).await {
            Ok(tasks) => {
                let gmt7 = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
                let now_gmt7 = chrono::Utc::now().with_timezone(&gmt7).naive_local();
                
                let active_tasks: Vec<_> = tasks.into_iter().filter(|t| {
                    let duration = t.deadline.signed_duration_since(now_gmt7);
                    duration.num_minutes() >= 0
                }).collect();

                if task_idx >= active_tasks.len() {
                    let _ = send_reply(chat_id, &format!("❌ Tugas nomor *{}* tidak ditemukan.\n\n💡 _Cek nomor terbaru dengan #tugas_", task_idx + 1)).await;
                    logger.set_status(crate::tui::state::JobStatus::Failed);
                    return;
                }

                let target_task = &active_tasks[task_idx];
                let old_title = &target_task.nama_tugas;
                let old_deadline = target_task.deadline.format("%Y-%m-%d %H:%M:%S");

                // 2. Ekstraksi update menggunakan AI
                let prompt = format!(
                    "Anda adalah asisten yang memperbarui data tugas kepanitiaan.\n\
                    Tugas saat ini:\n\
                    - Nama: {}\n\
                    - Deadline: {}\n\n\
                    Pesan pembaruan dari user: {}\n\n\
                    Penting: Analisis pesan pembaruan. Tentukan apa yang berubah (nama, deadline, atau keduanya). Jika ada yang tidak berubah, gunakan data dari 'Tugas saat ini'. Kembalikan JSON dengan format:\n\
                    {{\n  \"is_task\": true,\n  \"nama_tugas\": \"<nama_terbaru>\",\n  \"deadline\": \"<YYYY-MM-DD HH:MM:SS terbaru>\"\n}}\n\
                    Jika pesan bukan instruksi pembaruan yang valid, atur is_task ke false.",
                    old_title,
                    old_deadline,
                    update_msg
                );


                

                logger.log("🤖 Menghubungi AI untuk ekstraksi update...");
                match call_ai_extraction(&prompt).await {
                    Ok(ai_result) => {
                        if ai_result.is_task {
                            let new_nama = ai_result.nama_tugas.unwrap_or_else(|| old_title.clone());
                            let new_deadline_str = ai_result.deadline.unwrap_or_else(|| old_deadline.to_string());
                            
                            use chrono::NaiveDateTime;
                            let parsed_deadline = NaiveDateTime::parse_from_str(&new_deadline_str, "%Y-%m-%d %H:%M:%S")
                                .unwrap_or(target_task.deadline);

                            let updated_task = NewPiTask {
                                nama_tugas: new_nama.clone(),
                                deadline: parsed_deadline,
                            };

                            // 3. Simpan ke DB
                            match super::crud::update_pi_task(pool, target_task.id, updated_task).await {
                                Ok(saved) => {
                                    let mut changes = Vec::new();
                                    if old_title != &saved.nama_tugas {
                                        changes.push(format!("Nama: {} ➡️ {}", old_title, saved.nama_tugas));
                                    }
                                    if target_task.deadline != saved.deadline {
                                        changes.push(format!("Deadline: {} ➡️ {}", 
                                            target_task.deadline.format("%Y-%m-%d %H:%M WIB"), 
                                            saved.deadline.format("%Y-%m-%d %H:%M WIB")
                                        ));
                                    }

                                    let changes_str = if changes.is_empty() {
                                        "Tidak ada perubahan data.".to_string()
                                    } else {
                                        changes.join("\n")
                                    };

                                    let success_msg = format!(
                                        "✅ *UPDATE BERHASIL*\n\n🔄 *UPDATED*: {}\n_{}_",
                                        saved.nama_tugas,
                                        changes_str
                                    );
                                    let _ = send_reply(chat_id, &success_msg).await;
                                    logger.set_status(crate::tui::state::JobStatus::Completed);
                                }
                                Err(e) => {
                                    logger.log(&format!("❌ Gagal update tugas PI ke DB: {}", e));
                                    let _ = send_reply(chat_id, "❌ Gagal mengupdate tugas di database.").await;
                                    logger.set_status(crate::tui::state::JobStatus::Failed);
                                }
                            }
                        } else {
                            logger.log("💬 AI menganggap pesan bukan update yang valid.");
                            let _ = send_reply(chat_id, "❌ Pesan update tidak valid.").await;
                            logger.set_status(crate::tui::state::JobStatus::Completed);
                        }
                    }
                    Err(e) => {
                        logger.log(&format!("❌ Gagal mengekstrak update via AI: {}", e));
                        let _ = send_reply(chat_id, "❌ Terjadi kesalahan saat memproses update dengan AI.").await;
                        logger.set_status(crate::tui::state::JobStatus::Failed);
                    }
                }
                return;
            }
            Err(e) => {
                logger.log(&format!("❌ Gagal mengambil tugas PI: {}", e));
                let _ = send_reply(chat_id, "❌ Gagal mengambil daftar tugas untuk diupdate.").await;
                logger.set_status(crate::tui::state::JobStatus::Failed);
                return;
            }
        }
    }

    // 1. FILTER KETAT: Hanya proses jika ada "#marbot"
    if !lower_body.contains("#marbot") {
        logger.log("💬 Pesan PI diabaikan (tidak mengandung #marbot).");
        logger.set_status(crate::tui::state::JobStatus::Completed);
        return;
    }

    logger.log("🎪 Command #marbot terdeteksi! Memproses pesan Pekan Ilkomerz...");

    // 2. BERSIHKAN PESAN & BUILD PROMPT
    let clean_message = message_body.replace("(?i)#marbot", "").replace("#marbot", "").trim().to_string();
    let prompt = build_pi_extraction_prompt(&clean_message);
    
    // 3. PANGGIL AI
    logger.log("🤖 Menghubungi AI untuk ekstraksi jadwal/tugas...");
    let ai_result = match call_ai_extraction(&prompt).await {
        Ok(result) => result,
        Err(e) => {
            logger.log(&format!("❌ Gagal mengekstrak pesan via AI: {}", e));
            let _ = send_reply(chat_id, "AI Error🤕").await;
            logger.set_status(crate::tui::state::JobStatus::Failed);
            return;
        }
    };

    // 4. PROSES HASIL DARI AI
    if ai_result.is_task {
        let mut tasks_to_process = ai_result.tasks;

        // Fallback untuk backward compatibility atau jika AI mengembalikan format lama
        if tasks_to_process.is_empty() && ai_result.nama_tugas.is_some() {
            tasks_to_process.push(super::models::PiAITaskData {
                nama_tugas: ai_result.nama_tugas.unwrap(),
                deadline: ai_result.deadline,
            });
        }

        if tasks_to_process.is_empty() {
             logger.log("💬 AI menganggap pesan bukan task yang valid atau format tidak sesuai.");
             let _ = send_reply(chat_id, "❌ Tidak ada tugas spesifik yang dapat diekstrak.").await;
             logger.set_status(crate::tui::state::JobStatus::Completed);
             return;
        }

        let mut saved_tasks = Vec::new();

        for task_data in tasks_to_process {
            let nama_tugas = if task_data.nama_tugas.trim().is_empty() {
                "Tugas/Agenda Kepanitiaan (Tanpa Judul)".to_string()
            } else {
                task_data.nama_tugas
            };

            let deadline = task_data.deadline.and_then(|d_str| {
                NaiveDateTime::parse_from_str(&d_str, "%Y-%m-%d %H:%M:%S").ok()
            }).unwrap_or_else(|| {
                let besok = chrono::Local::now().naive_local() + chrono::Duration::days(1);
                chrono::NaiveDate::from_ymd_opt(besok.year(), besok.month(), besok.day())
                    .unwrap()
                    .and_hms_opt(23, 59, 59)
                    .unwrap()
            });

            let new_task = NewPiTask {
                nama_tugas,
                deadline,
            };

            // 5. SIMPAN KE DATABASE
            match create_pi_task(pool, new_task).await {
                Ok(saved) => {
                    saved_tasks.push(saved);
                }
                Err(e) => {
                    logger.log(&format!("❌ Gagal menyimpan tugas PI ke DB: {}", e));
                }
            }
        }

        if saved_tasks.is_empty() {
            let _ = send_reply(chat_id, "❌ Gagal menyimpan agenda PI ke database internal.").await;
            logger.set_status(crate::tui::state::JobStatus::Failed);
        } else {
            let mut success_msg = String::from("✅ *Agenda PI Berhasil Dicatat*\n\n");
            for saved in saved_tasks {
                success_msg.push_str(&format!("📝 *Tugas:* {}\n⏰ *Deadline:* {}\n\n",
                    saved.nama_tugas,
                    saved.deadline.format("%Y-%m-%d %H:%M WIB")
                ));
            }
            let _ = send_reply(chat_id, success_msg.trim_end()).await;
            logger.set_status(crate::tui::state::JobStatus::Completed);
        }
    } else {
        logger.log("💬 AI menganggap pesan tidak memiliki action item/tugas spesifik.");
        let _ = send_reply(chat_id, "tidak tahu tempe").await;
        logger.set_status(crate::tui::state::JobStatus::Completed);
    }
}

pub async fn send_daily_reminder(pool: &PgPool, logger: &JobLogger) -> Result<(), String> {
    let pi_group_id = std::env::var("PEKAN_ILKOMERS_GROUP_ID").unwrap_or_default().trim().to_string();
    if pi_group_id.is_empty() {
        logger.log("⚠️ PEKAN_ILKOMERS_GROUP_ID kosong, skip PI reminder");
        return Ok(());
    }

    match super::crud::get_upcoming_pi_tasks(pool).await {
        Ok(tasks) => {
            let gmt7 = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
            let now_gmt7 = chrono::Utc::now().with_timezone(&gmt7).naive_local();
            
            let active_tasks: Vec<_> = tasks.into_iter().filter(|t| {
                let duration = t.deadline.signed_duration_since(now_gmt7);
                duration.num_minutes() >= 0
            }).collect();

            if active_tasks.is_empty() {
                logger.log("📭 Tidak ada tugas PI aktif, skip reminder");
                return Ok(());
            }

            let msg = format_pi_tasks(active_tasks);
            let final_msg = msg.replacen(
                "*[Daftar Tugas Pekan Ilkomerz]*\n\n", 
                "🌄*[Pengingat Tugas Pekan Ilkomerz]*\n_Selamat pagi!_\n\n", 
                1
            );
            
            let _ = send_reply(&pi_group_id, &final_msg).await;
            logger.log("✅ Reminder PI harian berhasil dikirim");
            Ok(())
        }
        Err(e) => {
            let err_msg = format!("Gagal mengambil tugas PI untuk reminder: {}", e);
            logger.log(&err_msg);
            Err(err_msg)
        }
    }
}

pub async fn check_urgent_pi_deadlines(pool: &PgPool, logger: &JobLogger) -> Result<(), String> {
    use chrono::{Utc, Datelike};
    let gmt7 = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    let now_gmt7 = Utc::now().with_timezone(&gmt7).naive_local();
    let one_hour_later = now_gmt7 + chrono::Duration::hours(1);

    let rows = sqlx::query!(
        r#"
        SELECT id, nama_tugas, deadline 
        FROM pekan_ilkomers 
        WHERE deadline > $1 
          AND deadline <= $2 
          AND reminder_1h_sent = FALSE
        "#,
        now_gmt7,
        one_hour_later
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Ok(());
    }

    logger.log(&format!(
        "🚨 Menemukan {} tugas PI deadline < 1 jam",
        rows.len()
    ));

    let pi_group_id = std::env::var("PEKAN_ILKOMERS_GROUP_ID").unwrap_or_default().trim().to_string();

    if pi_group_id.is_empty() {
        logger.log("⚠️ PEKAN_ILKOMERS_GROUP_ID tidak diset, skip reminder PI");
        return Ok(());
    }

    let mut message = String::new();
    message.push_str("*[REMINDER PI! H-1 JAM]*\n\n");

    for (i, row) in rows.iter().enumerate() {
        let time_str = row.deadline.format("%H:%M").to_string();
        message.push_str(&format!("🔴 *[{}]* *{}*\n", i + 1, row.nama_tugas));
        message.push_str(&format!("*└─* Segera deadline pukul {}\n\n", time_str));
    }

    message.push_str("_Segera diselesaikan ya!_ 🔥");

    send_reply(&pi_group_id, &message).await.map_err(|e| e.to_string())?;

    for row in rows {
        sqlx::query("UPDATE pekan_ilkomers SET reminder_1h_sent = TRUE WHERE id = $1")
            .bind(row.id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }

    logger.log("✅ Reminder urgent PI berhasil dikirim.");
    Ok(())
}