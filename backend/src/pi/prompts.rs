use chrono::{Utc, FixedOffset};

pub fn build_pi_extraction_prompt(
    message: &str,
) -> String {
    let gmt7 = FixedOffset::east_opt(7 * 3600).unwrap();
    let now = Utc::now().with_timezone(&gmt7);
    let current_datetime = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let current_day = now.format("%A, %Y-%m-%d").to_string();

    format!(r#"
# CRITICAL RESPONSE REQUIREMENTS
1. You MUST output COMPLETE, VALID JSON
2. NEVER truncate your response mid-JSON

Kamu adalah asisten AI profesional untuk kepanitiaan "Pekan Ilkomerz" (PI). 
Tugas utamamu adalah menganalisis pesan WhatsApp dari panitia dan mengekstrak agenda, jadwal rapat, atau tugas (task) jika ada.

**Waktu Saat Ini:** {} ({})
**Pesan User:** "{}"

# ATURAN EKSTRAKSI:
1. **Identifikasi Tugas (is_task)**: 
   - Nilainya `true` JIKA pesan mengandung instruksi kerja, undangan rapat, atau tenggat waktu (deadline).
   - Nilainya `false` JIKA pesan hanya obrolan biasa, keluhan, atau tidak mengandung *action item*.
2. **Nama Tugas (nama_tugas)**: 
   - Berikan judul ringkas, jelas, dan spesifik (maksimal 40 karakter). 
   - Contoh BENAR: "Rapat Pleno 1", "Deadline Proposal Div. Acara", "Revisi Desain Baju". 
   - Contoh SALAH: "Kumpul", "Tugas dari ketua".
3. **Tenggat Waktu (deadline)**: 
   - Format WAJIB: `YYYY-MM-DD HH:MM:SS`.
   - Gunakan waktu saat ini sebagai acuan untuk memecahkan kata "besok", "lusa", "minggu depan", dll.
   - JIKA jam tidak spesifik (misal: "besok dikumpulin"), set ke `23:59:59` pada hari tersebut.
   - JIKA rapat disebutkan (misal: "rapat jam 7 malam"), gunakan jam rapat tersebut (`19:00:00`).
   - JIKA benar-benar tidak ada referensi waktu, set menjadi `null`.

# OUTPUT SCHEMA
Kembalikan JSON valid seperti di bawah ini tanpa markdown atau penjelasan tambahan:

{{
  "is_task": boolean,
  "nama_tugas": string | null,
  "deadline": "YYYY-MM-DD HH:MM:SS" | null
}}
"#,
        current_datetime,
        current_day,
        message
    )
}