use crate::models::BotCommand;

/// Handle bot commands and return response text
pub fn handle_command(cmd: BotCommand, user_phone: &str) -> String {
    match cmd {
        BotCommand::Ping => {
            println!("🏓 Ping command received from {}", user_phone);
            "🏓 Pong! Bot is alive and working!".to_string()
        }
        
        BotCommand::Tugas => {
            println!("📋 Tugas command received from {}", user_phone);
            // TODO: Fetch from database
            "📋 *Daftar Tugas*\n\n\
            Belum ada tugas tersimpan.\n\n\
            Kirim info tugas dan saya akan simpan otomatis!\n\
            Contoh: \"Tugas matematika dikumpulkan Jumat\"".to_string()
        }
        
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