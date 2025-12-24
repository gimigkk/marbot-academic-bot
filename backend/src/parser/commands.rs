use crate::models::BotCommand;
use crate::database::crud::get_active_assignments_sorted;
use sqlx::PgPool;

/// Handle bot commands and return response text
pub async fn handle_command(cmd: BotCommand, user_phone: &str, pool: &PgPool) -> String {
    match cmd {
        BotCommand::Ping => {
            println!("🏓 Ping command received from {}", user_phone);
            "🏓 Pong! Bot is alive and working!".to_string()
        }
        
        BotCommand::Tugas => {
            println!("📋 Tugas command received from {}", user_phone);
            
            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    if assignments.is_empty() {
                        "📋 *Daftar Tugas*\n\n\
                        Belum ada tugas tersimpan.\n\n\
                        Kirim info tugas dan saya akan simpan otomatis!\n\
                        Contoh: \"Tugas matematika dikumpulkan Jumat\"".to_string()
                    } else {
                        let mut response = "📋 *Daftar Tugas*\n\n".to_string();
                        
                        for (i, assignment) in assignments.iter().enumerate() {
                            let deadline = format_deadline_date(&assignment.deadline);
                            
                            let desc = assignment.description
                                .as_ref()
                                .map(|d| d.as_str())
                                .unwrap_or("Tidak ada deskripsi");
                            
                            response.push_str(&format!(
                                "*[ {} ]*\n#{} - {} - *{}*\n{}\n\n",
                                i + 1,
                                assignment.course_name,
                                assignment.title,
                                deadline,
                                desc
                            ));
                        }
                        
                        response.push_str("💡 _Gunakan #expand <id> untuk lihat detail lengkap_");
                        response
                    }
                }
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n\
                    Silakan coba lagi nanti.".to_string()
                }
            }
        }
        
        BotCommand::Expand(id) => {
            println!("🔍 Expand command for assignment {} from {}", id, user_phone);
            "⚠️ Command ini sedang dalam pengembangan dan belum tersedia.".to_string()
        }
        
        BotCommand::Done(id) => {
            println!("✅ Done command for assignment {} from {}", id, user_phone);
            // TODO: Update database
            format!(
                "✅ Great job!\n\n\
                Assignment #{} will be marked as complete once database is connected.",
                id
            )
        }
        
        BotCommand::Help => {
            println!("❓ Help command received from {}", user_phone);
            "🤖 *WhatsApp Academic Bot*\n\n\
            *Perintah:*\n\
            • #ping - Cek bot hidup\n\
            • #tugas - Lihat semua tugas\n\
            • #expand <id> - Lihat detail tugas\n\
            • #done <id> - Tandai tugas selesai\n\
            • #help - Tampilkan bantuan\n\n\
            *Pesan Natural:*\n\
            Kirim info tugas secara natural!\n\
            Contoh: \"Tugas bahasa Inggris deadline Senin\"".to_string()
        }
        
        BotCommand::UnknownCommand(cmd) => {
            println!("❓ Unknown command '{}' from {}", cmd, user_phone);
            format!(
                "❓ Command tidak dikenali: {}\n\n\
                Ketik #help untuk melihat daftar command yang tersedia.",
                cmd
            )
        }
    }
}

/// Format deadline as YY/MM/DD
fn format_deadline_date(deadline: &chrono::DateTime<chrono::Utc>) -> String {
    deadline.format("%y/%m/%d").to_string()
}