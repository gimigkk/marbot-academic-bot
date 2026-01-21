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
        
        // Admin only #update command
        "update" => {
            if parts.len() > 2 {
                // parts[1] = id, parts[2..] = update message
                let id = parts[1].parse().ok()?;
                let message = parts[2..].join(" ");
                Some(BotCommand::Update(id, message))
            } else {
                Some(BotCommand::MissingArgument("update".to_string()))
            }
        },
        

        // --- Set Kelas ---
        "setkelas" | "set" => {
            if parts.len() >= 3 {
                
                let mut split_idx = parts.len();
                
                // Cek dari token terakhir mundur sampai index 2
                for i in (2..parts.len()).rev() {
                    let token = parts[i];
                    let is_code = token.eq_ignore_ascii_case("all") || token.len() <= 3;
                    
                    if is_code {
                        split_idx = i;
                    } else {
                        // Ketemu token panjang (bagian dari nama matkul), berhenti scan
                        break;
                    }
                }
                
                let matkul = parts[1..split_idx].join(" ");
                let codes: Vec<String> = parts[split_idx..]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                // Fallback: Jika logic di atas gagal menemukan kode (misal user typo atau kode panjang),
                if codes.is_empty() && parts.len() >= 3 {
                     let fallback_split = parts.len() - 1;
                     let matkul = parts[1..fallback_split].join(" ");
                     let codes = vec![parts[fallback_split].to_string()];
                     Some(BotCommand::SetKelas(matkul, codes))
                } else {
                     Some(BotCommand::SetKelas(matkul, codes))
                }
            } else {
                Some(BotCommand::MissingArgument("setkelas".to_string()))
            }
        },
        // -----------------------------

        "tugas" | "tygas" => {
            // Handle both "#tugas" alone and "#tugas 123"
            if parts.len() > 1 {
                if let Ok(id) = parts[1].parse() {
                    return Some(BotCommand::Expand(id));
                }
            }
            Some(BotCommand::Tugas)
        },
        "mykelas" => Some(BotCommand::MyKelas),
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