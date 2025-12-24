use crate::models::BotCommand;
use crate::database::crud::get_active_assignments_sorted;
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
    pool: &PgPool
) -> CommandResponse {
    match cmd {
        BotCommand::Ping => {
            println!("🏓 Ping command received from {}", user_phone);
            CommandResponse::Text("Pong!".to_string())
        }
        
        BotCommand::Tugas => {
            println!("📋 Tugas command received from {}", user_phone);
            
            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    format_assignments_list(assignments, "📋 *Daftar Tugas*")
                }
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.\n\
                        Silakan coba lagi nanti.".to_string()
                    )
                }
            }
        }
        
        BotCommand::Today => {
            println!("📅 Today command received from {}", user_phone);
            
            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    let today = chrono::Utc::now().date_naive();
                    let today_assignments: Vec<_> = assignments
                        .into_iter()
                        .filter(|a| a.deadline.date_naive() == today)
                        .collect();
                    
                    format_assignments_list(today_assignments, "📅 *Tugas Hari Ini*")
                }
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.".to_string()
                    )
                }
            }
        }
        
        BotCommand::Week => {
            println!("📆 Week command received from {}", user_phone);
            
            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    let now = chrono::Utc::now();
                    let week_end = now + chrono::Duration::days(7);
                    
                    let week_assignments: Vec<_> = assignments
                        .into_iter()
                        .filter(|a| a.deadline >= now && a.deadline <= week_end)
                        .collect();
                    
                    format_assignments_list(week_assignments, "📆 *Tugas Minggu Ini (7 Hari)*")
                }
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.".to_string()
                    )
                }
            }
        }
        
        BotCommand::Expand(index) => {
            println!("🔍 Expand command for assignment {} from {} in chat {}", index, user_phone, chat_id);
            
            // Check if command is from academic channel group
            let academic_channels = std::env::var("ACADEMIC_CHANNELS")
                .unwrap_or_default();
            
            let is_academic_channel = academic_channels
                .split(',')
                .any(|channel| channel.trim() == chat_id);
            
            if is_academic_channel {
                return CommandResponse::Text(
                    "_Command ini tidak boleh dijalankan dalam group akademik!_".to_string()
                );
            }
            
            // Get all active assignments sorted
            match get_active_assignments_sorted(pool).await {
                Ok(assignments) => {
                    // Convert 1-based index to 0-based
                    let idx = (index as usize).saturating_sub(1);
                    
                    if idx >= assignments.len() {
                        CommandResponse::Text(
                            format!("❌ Tugas #{} tidak ditemukan.\n\
                                   Gunakan #tugas untuk melihat daftar tugas.", index)
                        )
                    } else {
                        let assignment = &assignments[idx];
                        CommandResponse::ForwardMessage {
                            message_id: assignment.message_id.clone().unwrap_or_else(|| "Unknown".to_string()),
                            warning: "_Ada kemungkinan info tugas telah berubah, mohon crosscheck lagi!_".to_string(),
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Error fetching assignments: {}", e);
                    CommandResponse::Text(
                        "❌ Maaf, terjadi kesalahan saat mengambil data tugas.".to_string()
                    )
                }
            }
        }
        
        BotCommand::Done(id) => {
            println!("✅ Done command for assignment {} from {}", id, user_phone);
            // TODO: Update database
            CommandResponse::Text(format!(
                "✅ Great job!\n\n\
                Assignment #{} will be marked as complete once database is connected.",
                id
            ))
        }
        
        BotCommand::Help => {
            println!("❓ Help command received from {}", user_phone);
            CommandResponse::Text(
                "🤖 *WhatsApp Academic Bot*\n\n\
                *Perintah:*\n\
                • #ping - Cek bot hidup\n\
                • #tugas - Lihat semua tugas\n\
                • #today - Lihat tugas hari ini\n\
                • #week - Lihat tugas 7 hari ke depan\n\
                • #tugas <id> | #<id> - Lihat pesan asli\n\
                • #done <id> - Tandai tugas selesai\n\
                • #help - Tampilkan bantuan\n\n\
                *Bahasa Natural:*\n\
                Info Tugas yang ada di grup akademik\nsudah otomatis saya simpan!".to_string()
            )
        }
        
        BotCommand::UnknownCommand(cmd) => {
            println!("❓ Unknown command '{}' from {}", cmd, user_phone);
            CommandResponse::Text(format!(
                "❓ Command tidak dikenali: {}\n\n\
                Ketik #help untuk melihat daftar command yang tersedia.",
                cmd
            ))
        }
    }
}

// Helper function to format assignment lists
fn format_assignments_list(
    assignments: Vec<crate::models::AssignmentWithCourse>, 
    header: &str
) -> CommandResponse {
    if assignments.is_empty() {
        CommandResponse::Text(
            format!("{}\n\n\
                    Belum ada tugas untuk periode ini.\n\n\
                    Kirim info tugas dan saya akan simpan otomatis!\n\
                    Contoh: \"Tugas matematika dikumpulkan Jumat\"", header)
        )
    } else {
        let mut response = format!("{}\n\n", header);
        
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
        
        response.push_str("\n_Gunakan #tugas <id> | #<id> untuk lihat detail lengkap_");
        CommandResponse::Text(response)
    }
}

/// Format deadline as YY/MM/DD
fn format_deadline_date(deadline: &chrono::DateTime<chrono::Utc>) -> String {
    deadline.format("%y/%m/%d").to_string()
}