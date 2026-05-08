use sqlx::PgPool;
use chrono::NaiveDateTime;
use crate::tui::JobLogger;
use super::models::{NewPiTask, PiAIExtraction};
use super::crud::create_pi_task;
use super::prompts::build_pi_extraction_prompt;

// HELPER: Kirim Pesan ke WAHA
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
        let content = json_resp["choices"]["message"]["content"]
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
    // 1. FILTER KETAT: Hanya proses jika ada "#marbot"
    if !message_body.to_lowercase().contains("#marbot") {
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
            let _ = send_reply(chat_id, "Waduh, Marbot lagi pusing (AI Error). Coba lagi nanti ya! 🤕").await;
            logger.set_status(crate::tui::state::JobStatus::Failed);
            return;
        }
    };

    // 4. PROSES HASIL DARI AI
    if ai_result.is_task {
        let nama_tugas = ai_result.nama_tugas.unwrap_or_else(|| "Tugas/Agenda Kepanitiaan (Tanpa Judul)".to_string());
        
        // Parsing String format (YYYY-MM-DD HH:MM:SS) 
        let deadline = ai_result.deadline.and_then(|d_str| {
            NaiveDateTime::parse_from_str(&d_str, "%Y-%m-%d %H:%M:%S").ok()
        }).unwrap_or_else(|| {
            let besok = chrono::Local::now().naive_local() + chrono::Duration::days(1);
            chrono::NaiveDate::from_ymd_opt(besok.year(), besok.month(), besok.day())
                .unwrap()
                .and_hms_opt(23, 59, 59)
                .unwrap()
        });

        let new_task = NewPiTask {
            nama_tugas: nama_tugas.clone(),
            deadline,
        };

        // 5. SIMPAN KE DATABASE
        match create_pi_task(pool, new_task).await {
            Ok(saved) => {
                let success_msg = format!(
                    "✅ *Agenda PI Berhasil Dicatat*\n\n📝 *Tugas:* {}\n⏰ *Deadline:* {}",
                    saved.nama_tugas,
                    saved.deadline.format("%Y-%m-%d %H:%M WIB")
                );
                let _ = send_reply(chat_id, &success_msg).await;
                logger.set_status(crate::tui::state::JobStatus::Completed);
            }
            Err(e) => {
                logger.log(&format!("❌ Gagal menyimpan tugas PI ke DB: {}", e));
                let _ = send_reply(chat_id, "❌ Gagal menyimpan agenda PI ke database internal.").await;
                logger.set_status(crate::tui::state::JobStatus::Failed);
            }
        }
    } else {
        logger.log("💬 AI menganggap pesan tidak memiliki action item/tugas spesifik.");
        let _ = send_reply(chat_id, "tidak tahu tempe").await;
        logger.set_status(crate::tui::state::JobStatus::Completed);
    }
}