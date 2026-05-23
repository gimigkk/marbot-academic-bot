use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct PiTask {
    pub id: i32,
    pub nama_tugas: String,
    pub deadline: NaiveDateTime, // Menggunakan NaiveDateTime karena tipe datanya TIMESTAMP (tanpa timezone)
    pub reminder_1h_sent: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewPiTask {
    pub nama_tugas: String,
    pub deadline: NaiveDateTime,
}

// Model untuk hasil ekstraksi AI khusus PI
#[derive(Debug, Serialize, Deserialize)]
pub struct PiAIExtraction {
    pub is_task: bool,
    pub nama_tugas: Option<String>,
    pub deadline: Option<String>, // Format: YYYY-MM-DD HH:MM:SS
    #[serde(default)]
    pub tasks: Vec<PiAITaskData>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PiAITaskData {
    pub nama_tugas: String,
    pub deadline: Option<String>,
}

