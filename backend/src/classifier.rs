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
    let text = text.trim();
    
    match text {
        "#ping" => Some(BotCommand::Ping),
        "#tugas" => Some(BotCommand::Tugas),
        "#help" => Some(BotCommand::Help),
        _ if text.starts_with("#done ") => {
            // Parse "#done 1"
            let id = text.strip_prefix("#done ")?.parse().ok()?;
            Some(BotCommand::Done(id))
        }
        _ if text.starts_with("#expand ") => {
            let id = text.strip_prefix("#expand ")?.parse().ok()?;
            Some(BotCommand::Expand(id))
        }
        _ => None, // Unrecognized command starting with #
    }
}