use crate::models::BotCommand;
use crate::database; // Import modul database
use sqlx::PgPool;    // Import PgPool

/// Handle bot commands and return response text
pub async fn handle_command(cmd: BotCommand, user_phone: &str, pool: &PgPool) -> String {
    match cmd {
        BotCommand::Ping => {
            println!("🏓 Ping command received from {}", user_phone);
            "🏓 Pong! Bot is alive and working!".to_string()
        }

        // READ
        BotCommand::Read | BotCommand::Tugas => {
            println!("📋 Read/Tugas command received from {}", user_phone);

            match database::crud::get_assignments(pool).await {
                Ok(tasks) => {
                    if tasks.is_empty() {
                        return "📂 *Daftar Tugas*\n\nBelum ada tugas tersimpan".to_string();
                    }

                    let mut response = String::from("📋 *Daftar Tugas Kuliah*\n\n");

                    for (i, t) in tasks.iter().enumerate() {
                        // 1.   Deadline
                        let deadline_str = match t.deadline {
                            Some(d) => d.format("%d %b %H:%M").to_string(),
                            None => "-".to_string(),
                        };

                        // 2. Pararel
                        let parallel_str = match &t.parallel_code {
                            Some(code) => format!("[{}] ", code.to_uppercase()),
                            None => "".to_string(), 
                        };

                        response.push_str(&format!(
                            "{}. [{}] {}{} *[{}]*\n   {}\n\n",
                            i + 1,
                            t.course_name,      // Matkul
                            parallel_str,       // Paralel 
                            t.title,            // Title
                            deadline_str,       // Deadline
                            t.description       // Desc
                        ));
                    }

                    response.push_str("_Ketik #help untuk bantuan_");
                    response
                }
                Err(e) => {
                    println!("❌ Error fetching assignments: {}", e);
                    "❌ Maaf, gagal mengambil data tugas dari database.".to_string()
                }
            }
        }

        // Expand
       BotCommand::Expand(id) => {
            println!("🔍 Expand command for assignment {} from {}", id, user_phone);
            // TODO: Fetch from database
            // SELECT message_id FROM assignments WHERE id = ?
            // Then call forward_message(user_phone, message_id)
            format!(
                "🔍 *Assignment #{}*\n\n\
                (Database not connected yet)\n\n\
                Once connected, the original message from the academic channel will be forwarded to you here.",
                id
            )
        }

        BotCommand::Done(id) => {
            println!("✅ Done command for assignment {} from {}", id, user_phone);
            format!("✅ Perintah selesai diterima untuk ID: {}", id)
        }

        BotCommand::Help => {
            "🤖 *WhatsApp Academic Bot*\n\n\
            *Perintah:*\n\
            • #tugas - Lihat daftar tugas\n\
            • #ping - Cek status bot\n\
            • #help - Bantuan\n\n\
            *Cara Tambah Tugas:*\n\
            Ketik: \"Tugas [Matkul] [Kode] Judul\"\n\
            Contoh: \"Tugas RPL K1 Buat Laporan\"".to_string()
        }

        BotCommand::UnknownCommand(cmd) => {
            format!("❓ Command tidak dikenali: {}", cmd)
        }
        
        _ => "Perintah belum diimplementasikan.".to_string(),
    }
}