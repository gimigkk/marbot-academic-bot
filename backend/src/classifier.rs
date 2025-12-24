use crate::models::{MessageType, BotCommand};

// Check if message is a bot command
pub fn classify_message(text: &str) -> MessageType {
    let trimmed = text.trim();
    
    // Check if it starts with # - if so, it's either a known command or unknown command
    if trimmed.starts_with('#') {
        // Try to parse as known command
        match parse_command(trimmed) {
            Some(cmd) => MessageType::Command(cmd),
            // If starts with # but not recognized, still treat as command attempt
            // This prevents unrecognized commands from being sent to AI
            None => {
                // Extract the attempted command
                let cmd_word = trimmed.split_whitespace()
                    .next()
                    .unwrap_or(trimmed);
                
                MessageType::Command(BotCommand::UnknownCommand(cmd_word.to_string()))
            }
        }
    } else {
        // No #, so it's a regular message that needs AI processing
        MessageType::NeedsAI(text.to_string())
    }
}

fn parse_command(text: &str) -> Option<BotCommand> {
    let trimmed = text.trim();
    
    // Remove # and any spaces after it, then lowercase
    let without_hash = trimmed.strip_prefix('#')?.trim();
    let parts: Vec<&str> = without_hash.split_whitespace().collect();
    
    if parts.is_empty() {
        return None;
    }
    
    let command = parts[0].to_lowercase();
    
    match command.as_str() {
        "ping" => Some(BotCommand::Ping),
        "tugas" => {
            // Handle both "#tugas" alone and "#tugas 123"
            if parts.len() > 1 {
                if let Ok(id) = parts[1].parse() {
                    return Some(BotCommand::Expand(id));
                }
            }
            Some(BotCommand::Tugas)
        }
        "today" => Some(BotCommand::Today),
        "week" => Some(BotCommand::Week),
        "help" => Some(BotCommand::Help),
        "done" => {
            if parts.len() > 1 {
                let id = parts[1].parse().ok()?;
                Some(BotCommand::Done(id))
            } else {
                None
            }
        }
        "expand" => {
            if parts.len() > 1 {
                let id = parts[1].parse().ok()?;
                Some(BotCommand::Expand(id))
            } else {
                None
            }
        }
        // Handle numeric-only commands like "# 123" or "#123"
        _ if command.chars().all(|c| c.is_numeric()) => {
            let id = command.parse().ok()?;
            Some(BotCommand::Expand(id))
        }
        _ => None,
    }
}