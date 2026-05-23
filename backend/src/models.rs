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

    #[serde(rename = "quotedMsg")]
    pub quoted_msg: Option<QuotedMessage>,
}

impl MessagePayload {
    pub fn get_quoted_message(&self) -> Option<QuotedMessage> {

        if let Some(ref quoted) = self.quoted_msg {
            return Some(quoted.clone());
        }
     
        if let Some(reply_to) = self.extra.get("replyTo") {
            if let Some(body_str) = reply_to.get("body").and_then(|v| v.as_str()) {
                return Some(QuotedMessage {
                    id: String::new(), 
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
    // For WEBJS/NOWEB
    #[serde(rename = "pushName")]
    pub push_name: Option<String>,

    // For GOWS - nested structure
    #[serde(rename = "Info")]
    pub info: Option<MessageInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MessageInfo {
    #[serde(rename = "PushName")]
    pub push_name: Option<String>,
    
    #[serde(rename = "Chat")]
    pub chat: Option<String>,
    
    #[serde(rename = "Sender")]
    pub sender: Option<String>,
    
    #[serde(flatten)]
    pub extra: serde_json::Value,
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
    #[serde(rename = "reply_to", skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,  
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentions: Option<Vec<String>>,
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
    SetKelas(String, Vec<String>),
    MyKelas,
    Daily(i32),
    UnknownCommand(String),
    MissingArgument(String),
    Update(u32, String),      
    Announcement(String), // yeayy fitur baru
    ApiKey(String),
    ApiDocs,
}


#[derive(Debug, sqlx::FromRow, Clone)]
pub struct ApiKeyRecord {
    pub key_name: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}


#[derive(Debug, sqlx::FromRow)]
pub struct UserCourseSetting {
    pub course_id: uuid::Uuid,
    pub parallel_code: String,
}

// Struct untuk hasil query MyKelas (gabungan nama matkul & status setting)
#[derive(Debug, sqlx::FromRow)]
pub struct UserCourseStatus {
    pub course_name: String,
    pub parallel_code: Option<String>, 
}

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
    
    /// Multiple assignments in one message
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
        new_course_name: Option<String>,
        parallel_codes: Vec<String>,  
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        original_message: Option<String>,
    },
    
    Unrecognized {
        #[serde(default)]
        reason: Option<String>,
        category: UnrecognizedCategory,
    },
}

impl AIClassification {
    /// Clean up parallel codes - if "all" is present, remove all other codes
    pub fn clean_parallel_codes(self) -> Self {
        match self {
            AIClassification::AssignmentInfo { 
                course_name, 
                title, 
                deadline, 
                description, 
                parallel_codes,
                original_message 
            } => {
                let cleaned_codes = Self::clean_codes_array(parallel_codes);
                
                AIClassification::AssignmentInfo {
                    course_name,
                    title,
                    deadline,
                    description,
                    parallel_codes: cleaned_codes,
                    original_message,
                }
            }
            
            AIClassification::MultipleAssignments { assignments, original_message } => {
                let cleaned_assignments = assignments
                    .into_iter()
                    .map(|mut assignment| {
                        assignment.parallel_codes = Self::clean_codes_array(assignment.parallel_codes);
                        assignment
                    })
                    .collect();
                
                AIClassification::MultipleAssignments {
                    assignments: cleaned_assignments,
                    original_message,
                }
            }
            
            AIClassification::AssignmentUpdate {
                reference_keywords,
                changes,
                new_deadline,
                new_title,
                new_description,
                new_course_name,
                parallel_codes,
                original_message,
            } => {
                let cleaned_codes = Self::clean_codes_array(parallel_codes);
                
                AIClassification::AssignmentUpdate {
                    reference_keywords,
                    changes,
                    new_deadline,
                    new_title,
                    new_description,
                    new_course_name,
                    parallel_codes: cleaned_codes,
                    original_message,
                }
            }
            other => other,
        }
    }
  
    fn clean_codes_array(codes: Vec<String>) -> Vec<String> {
        if codes.iter().any(|c| c.eq_ignore_ascii_case("all")) {
            vec!["all".to_string()]
        } else {
            codes
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub enum UnrecognizedCategory {
    #[default]
    Informal,          
    AcademicRelated,   
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
    pub personal_reminder_sent: bool, 
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

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
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
   
    pub fn targets_parallel(&self, parallel: &str) -> bool {
        if self.parallel_codes.is_empty() {
            return false;
        }
      
        if self.parallel_codes.contains(&"all".to_string()) {
            return true;
        }
       
        self.parallel_codes.iter().any(|p| {
            let p_str = p.to_lowercase();
            let target_str = parallel.to_lowercase();
            
            if p_str == target_str {
                return true;
            }

            if p_str.starts_with('r') && target_str.starts_with('p') {
                return p_str[1..] == target_str[1..];
            }
            if p_str.starts_with('p') && target_str.starts_with('r') {
                return p_str[1..] == target_str[1..];
            }

            false
        })
    }
}

impl AssignmentData {
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
