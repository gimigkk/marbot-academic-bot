use sqlx::PgPool;
use chrono::NaiveDateTime;
use crate::tui::JobLogger;
use super::models::NewPiTask;
use super::crud::create_pi_task;

// Helper function untuk mengirim pesan ke WAHA (kamu bisa sesuaikan dengan fungsi di main.rs)
async fn send_reply(chat_id: &str, text: &str) -> Result<(), String> {
    let waha_url = format!("{}/api/sendText", std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string()));
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    
    let payload = serde_json::json!({
        "chatId": chat_id,
        "text": text,
        "session": "default"
    });
    
    let client = reqwest::Client::new();
    let res = client.post(waha_url)
        .header("X-Api-Key", api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
        
    if res.status().is_success() { Ok(()) } else { Err("API Error".to_string()) }
}

pub async fn process_pi_message(
    pool: &PgPool,
    message_body: &str,
    chat_id: &str,
    logger: &JobLogger,
) {
    logger.log("🎪 Memproses pesan untuk Pekan Ilkomerz...");

    // TODO: Di sini kamu bisa memanggil Google Gemini API khusus untuk memparsing PI
    // Prompt-nya bisa diset seperti: 
    // "Kamu adalah asisten acara kepanitiaan. Ekstrak nama_tugas dan deadline (YYYY-MM-DD HH:MM:SS) dari pesan berikut: {message_body}"
    
    // *SIMULASI HASIL PARSING AI SEMENTARA* // (Ganti bagian ini nanti dengan fungsi pemanggil AI milikmu)
    let is_meeting_or_task = message_body.to_lowercase().contains("jam") || message_body.to_lowercase().contains("besok");
    
    if is_meeting_or_task {
        // Contoh fallback parsing manual/dummy AI
        let nama_tugas = format!("Agenda/Tugas dari pesan: {}", message_body.chars().take(20).collect::<String>());
        
        // Asumsi deadline besok jam 15:00 jika tidak terdeteksi baik
        let besok = chrono::Local::now().naive_local() + chrono::Duration::days(1);
        let deadline_dummy = chrono::NaiveDate::from_ymd_opt(besok.year(), besok.month(), besok.day())
            .unwrap()
            .and_hms_opt(15, 0, 0)
            .unwrap();

        let new_task = NewPiTask {
            nama_tugas: nama_tugas.clone(),
            deadline: deadline_dummy,
        };

        match create_pi_task(pool, new_task).await {
            Ok(saved) => {
                let success_msg = format!(
                    "✅ *Agenda PI Dicatat*\n\n📝 *Tugas:* {}\n⏰ *Deadline:* {}",
                    saved.nama_tugas,
                    saved.deadline.format("%Y-%m-%d %H:%M WIB")
                );
                let _ = send_reply(chat_id, &success_msg).await;
                logger.set_status(crate::tui::state::JobStatus::Completed);
            }
            Err(e) => {
                logger.log(&format!("❌ Gagal menyimpan tugas PI: {}", e));
                let _ = send_reply(chat_id, "❌ Gagal menyimpan agenda PI ke database.").await;
                logger.set_status(crate::tui::state::JobStatus::Failed);
            }
        }
    } else {
        logger.log("💬 Pesan PI informal/tidak terdeteksi sebagai agenda. Diabaikan.");
        logger.set_status(crate::tui::state::JobStatus::Completed);
    }
}