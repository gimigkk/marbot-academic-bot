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
    let text_lower = text.trim().to_lowercase();
    
    match text_lower.as_str() {
        "#ping" => Some(BotCommand::Ping),
        "#tugas" => Some(BotCommand::Tugas),
        "#today" => Some(BotCommand::Today),
        "#week" => Some(BotCommand::Week),
        "#help" => Some(BotCommand::Help),
        _ if text_lower.starts_with("#done ") => {
            let id = text_lower.strip_prefix("#done ")?.trim().parse().ok()?;
            Some(BotCommand::Done(id))
        }
        _ if text_lower.starts_with("#expand ") => {
            let id = text_lower.strip_prefix("#expand ")?.trim().parse().ok()?;
            Some(BotCommand::Expand(id))
        }
        _ if text_lower.starts_with("#tugas ") => {
            let id = text_lower.strip_prefix("#tugas ")?.trim().parse().ok()?;
            Some(BotCommand::Expand(id))
        }
        _ if text_lower.len() > 1 && text_lower.chars().skip(1).all(|c| c.is_numeric()) => {
            let id = text_lower.strip_prefix('#')?.trim().parse().ok()?;
            Some(BotCommand::Expand(id))
        }
        _ => None,
    }
}