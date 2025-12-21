use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    NeedsAI(String), // Raw message text to send to AI
}

#[derive(Debug)]
pub enum BotCommand {
    Ping,
    Tugas,
    Expand(u32),  // #expand 1
    Done(u32),    // #done 1
    Help,
    UnknownCommand(String), // Any # command we don't recognize
}

// ===== AI EXTRACTION RESULTS =====

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AIClassification {
    AssignmentInfo {
        title: String,
        due_date: Option<String>,  // e.g., "2025-01-15"
        description: String,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        original_message: Option<String>,  // Ignore if AI returns it
    },
    CourseInfo {
        content: String,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        original_message: Option<String>,  // Ignore if AI returns it
    },
    AssignmentReminder {
        assignment_reference: String,
    },
    FailedCommand {
        attempted_command: String,
        suggestion: String,
    },
    Unrecognized,
}