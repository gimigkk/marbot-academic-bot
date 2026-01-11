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

    pub participant: Option<String>,

    #[serde(default)]
    #[serde(flatten)]
    pub extra: Value,

    #[serde(rename = "hasMedia")]
    pub has_media: Option<bool>,
    #[serde(rename = "mediaUrl")]
    pub media_url: Option<String>,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    pub media: Option<MediaInfo>,

    #[serde(rename = "_data")]
    pub data: Option<MessageData>,

    // This is for backwards compatibility if quotedMsg exists
    #[serde(rename = "quotedMsg")]
    pub quoted_msg: Option<QuotedMessage>,
}

impl MessagePayload {
    /// Get quoted message from either quotedMsg field or extra.replyTo
    pub fn get_quoted_message(&self) -> Option<QuotedMessage> {
        // First try the quotedMsg field
        if let Some(ref quoted) = self.quoted_msg {
            return Some(quoted.clone());
        }
        
        // If not found, try extra.replyTo
        if let Some(reply_to) = self.extra.get("replyTo") {
            // Extract the body text from replyTo
            if let Some(body_str) = reply_to.get("body").and_then(|v| v.as_str()) {
                return Some(QuotedMessage {
                    id: String::new(), // replyTo doesn't have ID
                    text: body_str.to_string(),
                    from: None,
                });
            }
        }
        
        None
    }
}

#[derive(Debug, Deserialize)]
pub struct MessageData {
    #[serde(rename = "pushName")]
    pub push_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MediaInfo {
    pub url: Option<String>,
    pub mimetype: Option<String>,
    pub filename: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotedMessage {
    pub id: String,
    #[serde(rename = "body")]
    pub text: String,
    #[serde(default)]
    pub from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationRequest {
    pub assignment_id: uuid::Uuid,
    pub missing_fields: Vec<String>,
    pub message_id: String,
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
    Today,
    Week,
    Expand(u32),
    Todo,
    Done(u32),
    Undo,
    Help,
    Delete(u32),
    UnknownCommand(String),
    MissingArgument(String), // New variant for commands with missing arguments
}

// ===== AI EXTRACTION RESULTS =====

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AIClassification {
    /// Single assignment (backward compatible)
    AssignmentInfo {
        course_name: Option<String>,
        title: String,
        deadline: Option<String>,
        description: Option<String>,
        parallel_codes: Vec<String>,  
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        original_message: Option<String>,
    },
    
    /// NEW: Multiple assignments in one message
    MultipleAssignments {
        assignments: Vec<AssignmentData>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        original_message: Option<String>,
    },
    
    AssignmentUpdate {
        reference_keywords: Vec<String>,
        changes: String,
        new_title: Option<String>,
        new_deadline: Option<String>,
        new_description: Option<String>,
        parallel_codes: Vec<String>,  
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        original_message: Option<String>,
    },
    
    Unrecognized {
        #[serde(default)]
        reason: Option<String>,
    },
}

/// Individual assignment data for batch processing
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssignmentData {
    pub course_name: String,
    pub title: String,
    pub deadline: Option<String>,
    pub description: Option<String>,
    pub parallel_codes: Vec<String>,  
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
    pub course_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub deadline: Option<DateTime<Utc>>,
    pub parallel_codes: Vec<String>,
    pub sender_id: Option<String>,
    pub message_ids: Vec<String>,
    pub reminder_1h_sent: bool, 
    pub relating_messages: Vec<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AssignmentDisplay {
    pub course_name: String,
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub deadline: Option<DateTime<Utc>>,
    pub parallel_codes: Vec<String>,  
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewAssignment {
    pub course_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub deadline: Option<DateTime<Utc>>,
    pub parallel_codes: Vec<String>,
    pub sender_id: Option<String>,
    pub message_id: String,
    pub relating_messages: Vec<String>,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct AssignmentWithCourse {
    pub id: uuid::Uuid,
    pub course_name: String,
    pub parallel_codes: Vec<String>,
    pub title: String,
    pub first_alias: String,
    pub description: Option<String>,
    pub deadline: Option<DateTime<Utc>>,
    pub message_ids: Vec<String>,   
    pub sender_id: Option<String>, 
    pub is_completed: bool,
    pub relating_messages: Vec<String>,
}

impl AssignmentWithCourse {
    pub fn deadline_is_missing(&self) -> bool {
        self.deadline.is_none()
    }
    
    /// Format parallel codes for display
    pub fn format_parallel_display(&self) -> String {
        if self.parallel_codes.is_empty() {
            "N/A".to_string()  // No parallel set = N/A
        } else {
            format!("[{}]", self.parallel_codes
                .iter()
                .map(|c| c.to_uppercase())  
                .collect::<Vec<_>>()
                .join(", "))
        }
    }
}

impl Assignment {
    /// Format parallel codes for display
    pub fn format_parallel_display(&self) -> String {
        if self.parallel_codes.is_empty() {
            "N/A".to_string()
        } else {
            format!("[{}]", self.parallel_codes
                .iter()
                .map(|c| c.to_uppercase()) 
                .collect::<Vec<_>>()
                .join(", "))
        }
    }
    
    /// Check if this assignment targets a specific parallel
    pub fn targets_parallel(&self, parallel: &str) -> bool {
        if self.parallel_codes.is_empty() {
            return false;
        }
        
        // "all" targets everyone
        if self.parallel_codes.contains(&"all".to_string()) {
            return true;
        }
        
        // Check if the specific parallel is in the list
        self.parallel_codes.iter()
            .any(|p| p.eq_ignore_ascii_case(parallel))
    }
}

impl AssignmentData {
    /// Format parallel codes for display
    pub fn format_parallel_display(&self) -> String {
        if self.parallel_codes.is_empty() {
            "N/A".to_string()
        } else {
            format!("[{}]", self.parallel_codes.join(", "))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserCompletion {
    pub user_id: String,
    pub assignment_id: Uuid,
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
