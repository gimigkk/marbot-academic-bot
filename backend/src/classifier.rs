use crate::models::{MessageType, BotCommand};

// Check if message is a bot command
#[allow(non_snake_case)]
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
        "test" | "tes" | "ping" => Some(BotCommand::Ping),
        
        // --- Set Kelas (DIUBAH) ---
        "setkelas" | "set" => {
            if parts.len() >= 3 {
                let matkul = parts[1].to_string();
                
                // Pecah kode-kode kelas menjadi Vector
                // Gabungkan semua argumen setelah nama mata kuliah
                let kode_string = parts[2..].join(" ");
                
                // Proses pemecahan kode (mendukung spasi atau koma)
                let kode: Vec<String> = kode_string
                    .replace(',', ' ')        // Ganti koma dengan spasi
                    .split_whitespace()       // Pisah berdasarkan spasi
                    .map(|s| s.to_string())   // Ubah ke String
                    .collect();
                
                Some(BotCommand::SetKelas(matkul, kode))
            } else {
                Some(BotCommand::MissingArgument("setkelas".to_string()))
            }
        },
        // -----------------------------

        "tugas" => {
            // Handle both "#tugas" alone and "#tugas 123"
            if parts.len() > 1 {
                if let Ok(id) = parts[1].parse() {
                    return Some(BotCommand::Expand(id));
                }
            }
            Some(BotCommand::Tugas)
        },
        "todo" => Some(BotCommand::Todo),  
        "today" => Some(BotCommand::Today),
        "week" => Some(BotCommand::Week),
        "help" => Some(BotCommand::Help),
        "undo" => Some(BotCommand::Undo),
        "done" => {
            if parts.len() > 1 {
                let id = parts[1].parse().ok()?;
                Some(BotCommand::Done(id))
            } else {
                // Missing argument - return special error variant
                Some(BotCommand::MissingArgument("done".to_string()))
            }
        },
        "delete" | "hapus" => {
            if parts.len() > 1 {
                let id = parts[1].parse().ok()?;
                Some(BotCommand::Delete(id))
            } else {
                // Missing argument - return special error variant
                Some(BotCommand::MissingArgument("delete".to_string()))
            }
        },
        "expand" => {
            if parts.len() > 1 {
                let id = parts[1].parse().ok()?;
                Some(BotCommand::Expand(id))
            } else {
                // Missing argument - return special error variant
                Some(BotCommand::MissingArgument("expand".to_string()))
            }
        },
        // Handle numeric-only commands like "# 123" or "#123"
        _ if command.chars().all(|c| c.is_numeric()) => {
            let id = command.parse().ok()?;
            Some(BotCommand::Expand(id))
        },
        _ => None,
    }
}