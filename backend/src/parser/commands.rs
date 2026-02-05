// backend/src/commands.rs - Complete rewrite with message resending

use crate::database::crud::{
    get_active_assignments_for_user, 
    get_active_assignments_sorted, 
    mark_assignment_complete, 
    unmark_assignment_complete, 
    get_last_completed_assignment,
    delete_assignment,
    set_user_course_parallel,
    get_user_course_statuses,  
};

use crate::models::{BotCommand, AssignmentWithCourse};
use crate::tui::JobLogger;
use chrono::{DateTime, Duration, FixedOffset, Datelike, NaiveDate, Utc};
use sqlx::PgPool;

pub enum CommandResponse {
    Text(String),
    ResendMessages { messages: Vec<String>, summary: String },
    
    ProcessWithAI {
        message: String,
        force_mode: AIForceMode,
        target_assignment: Option<AssignmentWithCourse>,
    },
}

pub enum AIForceMode {
    Update, 
    NewOnly,
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
            
            // 7. Get current time in GMT+7
            let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
            let current_time = Utc::now().with_timezone(&gmt7);
            //let time_str = current_time.format("%H:%M:%S WIB").to_string();
            //let bot_duration = start_time.elapsed();
            
            use chrono::Timelike;
            let hour = current_time.hour();
            let motivation = match hour {
                0..=5 => "🌙 Masih begadang? Semangat!",
                6..=11 => "☀️ Selamat pagi! Semoga lancar hari ini",
                12..=17 => "🌤️ Selamat siang! Jangan lupa istirahat",
                18..=21 => "🌆 Selamat sore! Deadline checks?",
                _ => "🌃 Selamat malam! Jangan begadang ya",
            };
            
            
            let response_text = format!(
                "🏓 *PONG!* _v1.2_\n\
                _{}_",
                motivation,
            );
            
            CommandResponse::Text(response_text)
        }

        // yang admin only
        BotCommand::Update(id, message) => {
            // Only debug group
            let debug_group_id = std::env::var("DEBUG_GROUP_ID").ok();
            
            if debug_group_id.as_deref() != Some(chat_id) {
                return CommandResponse::Text(
                    "⛔ *AKSES DITOLAK*\n\n\
                    Command #update hanya boleh digunakan di Debug Group.\n\
                    _Admin only!_ 👮"
                        .to_string(),
                );
            }
    
            match get_active_assignments_sorted(pool, Some(logger)).await {
                Ok(assignments) => {
                    let idx = (id as usize).saturating_sub(1);
                    
                    if idx >= assignments.len() {
                        return CommandResponse::Text(format!(
                            "❌ Tugas nomor *{}* tidak ditemukan.\n\n\
                            💡 _Gunakan #tugas untuk lihat daftar terbaru_",
                            id
                        ));
                    }
                    
                    let target = assignments[idx].clone();
                    
                    logger.log(&format!(
                        "   Target: {} - {}",
                        target.course_name,
                        target.title
                    ));

                    CommandResponse::ProcessWithAI {
                        message,
                        force_mode: AIForceMode::Update,
                        target_assignment: Some(target),
                    }
                }
                Err(e) => {
                    logger.log(&format!("❌ Failed to fetch assignments: {}", e));
                    CommandResponse::Text(
                        "❌ Gagal mengambil daftar tugas.\n_Coba lagi sebentar._"
                            .to_string()
                    )
                }
            }
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
                                  
                                    body.push_str(&format!("✅ {}\n", matkul));
                                    body.push_str(&format!("*└─* Kelas: *{}*\n", code.to_uppercase()));
                                }
                                _ => {
            
                                    body.push_str(&format!("❌ {}\n", matkul));
                                    body.push_str("*└─* Kelas: _(belum diset)_\n");
                                }
                            }
            
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
         
            let codes_display = codes.join(" ");
            logger.log(&format!("⚙️ SetKelas command: {} [{}] from {}", matkul, codes_display, user_phone));
            
           
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

            let academic_channels = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
            let is_academic_channel = academic_channels
                .split(',')
                .any(|channel| channel.trim() == chat_id);

            if is_academic_channel {
                return CommandResponse::Text(
                    "⚠️ _Command ini tidak boleh dijalankan di grup akademik. Ketik command ini di chat pribadi atau grup lain ya!_"
                        .to_string(),
                );
            }

            match get_active_assignments_for_user(pool, user_phone, Some(logger)).await {
                Ok((assignments, user_settings)) => {
                    let has_settings = !user_settings.is_empty();
                    
            
                    let filtered_assignments: Vec<_> = assignments.into_iter().filter(|a| {
                      
                        if a.is_completed { return false; }
                   
                        if a.parallel_codes.is_empty() || a.parallel_codes.contains(&"all".to_string()) {
                            return true; 
                        }
                        
                       
                        if let Some(user_codes_str) = user_settings.get(&a.course_name) {
                         
                            let user_codes: Vec<&str> = user_codes_str.split(',').collect();
                            
            
                            for task_code in &a.parallel_codes {
                                let task_str = task_code.as_str();

                              
                                if user_codes.contains(&task_str) {
                                    return true;
                                }

                              
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
                            
                            // No match found
                            return false;
                        }
                        
                       
                        true 
                    }).collect();

                    let header = format!("*[To-Do]* _{}_", user_name);
                    let mut response = format_assignments_list(filtered_assignments, &header, false, true);
                    
                    if let CommandResponse::Text(ref mut text) = response {
                       
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
                    "⚠️ _Command ini tidak boleh dijalankan di grup akademik. Ketik command ini di chat pribadi atau grup lain ya!_"
                        .to_string(),
                );
            }

            match get_active_assignments_for_user(pool, user_phone, Some(logger)).await {
                Ok((assignments, user_settings)) => {
                  
                    let filtered_assignments: Vec<_> = assignments.into_iter().filter(|a| {
                    
                        if a.is_completed { return false; }
                        
                    
                        if a.parallel_codes.is_empty() || a.parallel_codes.contains(&"all".to_string()) {
                            return true; 
                        }
                        
                    
                        if let Some(user_codes_str) = user_settings.get(&a.course_name) {
                            let user_codes: Vec<&str> = user_codes_str.split(',').collect();
                            
                            for task_code in &a.parallel_codes {
                                let task_str = task_code.as_str();

                                
                                if user_codes.contains(&task_str) {
                                    return true;
                                }

                                
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
                    }).collect();

                    let idx = (index as usize).saturating_sub(1);

                    if idx >= filtered_assignments.len() {
                        CommandResponse::Text(format!(
                            "❌ Tugas *#{}* tidak ditemukan di to-do list kamu.\n\n\
                            💡 _Tip: Ketik #todo untuk lihat daftar tugas._",
                            index
                        ))
                    } else {
                        let assignment = &filtered_assignments[idx];

                     
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
            
            let academic_channels = std::env::var("ACADEMIC_CHANNELS").unwrap_or_default();
            let is_academic_channel = academic_channels
                .split(',')
                .any(|channel| channel.trim() == chat_id);

            if is_academic_channel {
                return CommandResponse::Text(
                    "⚠️ _Command ini tidak boleh dijalankan di grup akademik. Ketik command ini di chat pribadi atau grup lain ya!_"
                        .to_string(),
                );
            }

            match get_active_assignments_for_user(pool, user_phone, Some(logger)).await {
                Ok((assignments, user_settings)) => {
                    let filtered_assignments: Vec<_> = assignments.into_iter().filter(|a| {
                    
                        if a.is_completed { return false; }
                        
                       
                        if a.parallel_codes.is_empty() || a.parallel_codes.contains(&"all".to_string()) {
                            return true; 
                        }
                        
                       
                        if let Some(user_codes_str) = user_settings.get(&a.course_name) {
                            let user_codes: Vec<&str> = user_codes_str.split(',').collect();
                            
                            for task_code in &a.parallel_codes {
                                let task_str = task_code.as_str();

                            
                                if user_codes.contains(&task_str) {
                                    return true;
                                }

                        
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
                    }).collect();

                    let idx = (id as usize).saturating_sub(1);
                    
                    if idx >= filtered_assignments.len() {
                        return CommandResponse::Text(format!(
                            "❌ Tugas nomor *{}* tidak ditemukan di to-do list kamu.\n\n\
                            💡 _Tip: Ketik #todo untuk lihat daftar tugas._",
                            id
                        ));
                    }
                    
                    let assignment = &filtered_assignments[idx];
                    
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
            let debug_group_id = std::env::var("DEBUG_GROUP_ID").ok();

            let is_authorized = academic_channels
                .split(',')
                .map(|s| s.trim())
                .any(|allowed_id| allowed_id == chat_id)
                || debug_group_id.as_deref() == Some(chat_id);

            if !is_authorized {
                return CommandResponse::Text(
                    "⛔ *AKSES DITOLAK*\n\n\
                    Fitur hapus hanya boleh dilakukan di Grup Official oleh PJ Matkul/Admin.\n\
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
                "*[Halow aku Maarbot 👋]*\n\
                _Ilkomerz61's Memory Augmented Academic Recollection BOT_\n\n\
                *USER GUIDE (BACA KALO BINGUNG CARA SETUP MARBOT)*\n\
                https://ipb.link/marbot\n\n\
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
                - #setkelas <matkul> <kode1> <kode2> — atur kode pararel untuk matkul\n\
                - #mykelas — lihat settings kode parallel kamu\n\n\
                *Perintah Admin:*\n\
                - #delete <id> — hapus tugas (id dari #tugas)\n\
                - #update <id> <pesan> — update tugas dengan AI\n\n\
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
                "update" => {
                    "⚠️ *Cara pakai yang benar:*\n\n\
                    #update <nomor> <pesan update>\n\n\
                    *Contoh:*\n\
                    - #update 3 deadline besok jam 14:00\n\
                    - #update 1 diundur minggu depan\n\
                    - #update 5 judul: Quiz Kalkulus 3\n\n\
                    💡 _Gunakan #tugas untuk lihat nomor assignment_\n\
                    ⚠️ _Command ini hanya untuk Debug Group_"
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
        response.push_str(&format!("*├─* {}\n", due_text));
        response.push_str(&format!("*└─* `#{}{}`\n", course_alias, parallel_display));
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