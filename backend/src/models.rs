use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};

// ===== WEBHOOK PAYLOAD TYPES (from WAHA) =====

#[derive(Debug, Deserialize)]
pub struct WebhookPayload {
    pub event: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub session: String,
    pub payload: MessagePayload,
}

#[derive(Debug, Deserialize)]
pub struct MessagePayload {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub body: String,
    pub from: String,
    #[serde(default)]
    #[serde(rename = "fromMe")]
    pub from_me: bool,
    #[serde(default)]
    #[serde(rename = "chatId")]
    #[serde(flatten)]
    #[allow(dead_code)]
    pub extra: Value,
}

// ===== WAHA API TYPES =====

#[derive(Debug, Serialize)]
pub struct SendTextRequest {
    #[serde(rename = "chatId")]
    pub chat_id: String,
    pub text: String,
    pub session: String,
}

#[derive(Debug, Serialize)]
pub struct ForwardMessageRequest {
    #[serde(rename = "chatId")]
    pub chat_id: String,
    #[serde(rename = "messageId")]
    pub message_id: String,
    pub session: String,
}

// ===== MESSAGE CLASSIFICATION =====

#[derive(Debug)]
pub enum MessageType {
    Command(BotCommand),
    NeedsAI(String),
}

#[derive(Debug)]
pub enum BotCommand {
    Ping,
    Tugas,
    Expand(u32),
    Done(u32),
    Help,
    UnknownCommand(String),
}

// ===== AI EXTRACTION RESULTS =====

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AIClassification {
    AssignmentInfo {
        course_name: Option<String>,  // Added: Course name from AI
        title: String,
        deadline: Option<String>,  // "2025-01-15"
        description: Option<String>, // kayaknya nanti gw bikin harus diisi
        parallel_code: Option<String>,  // Added: parallel code (k1, k2, etc.)
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        original_message: Option<String>,
    },
    
    AssignmentUpdate {
        reference_keywords: Vec<String>,
        changes: String,
        new_deadline: Option<String>,
        new_description: Option<String>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        original_message: Option<String>,
    },
    
    Unrecognized,
}

// ===== DATABASE MODELS =====

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Course {
    pub id: Uuid,
    pub name: String,
    pub aliases: Option<Vec<String>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewCourse {
    pub name: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Assignment {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub course_id: Option<Uuid>,  // Foreign key to courses table
    pub title: String,
    pub description: String,
    pub deadline: Option<DateTime<Utc>>,
    pub parallel_code: Option<String>,  // k1, k2, k3, p1, p2, p3
    pub sender_id: Option<String>,      // WhatsApp sender number
    pub message_id: String,             // WhatsApp message ID
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AssignmentDisplay {
    pub course_name: String,
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub deadline: Option<DateTime<Utc>>,
    pub parallel_code: Option<String>,  // k1, k2, k3, p1, p2, p3
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewAssignment {
    pub course_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub deadline: Option<DateTime<Utc>>,
    pub parallel_code: Option<String>,
    pub sender_id: Option<String>,
    pub message_id: String,
}

/// Struct to hold assignment data with course name for display
#[derive(Debug)]
pub struct AssignmentWithCourse {
    pub id: uuid::Uuid,
    pub course_name: String,
    pub parallel_code: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub deadline: DateTime<Utc>,
    pub message_id: String,
    pub sender_id: String,
}


#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct WaLog {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub event_type: Option<String>,
    pub payload: Option<Value>,
    pub processed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewWaLog {
    pub event_type: Option<String>,
    pub payload: Option<Value>,
}