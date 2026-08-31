use crate::models::{MessageType, BotCommand};

// Check if message is a bot command
#[allow(non_snake_case)]
pub fn classify_message(text: &str) -> MessageType {
    let trimmed = text.trim();
    
    // Strip WhatsApp markdown wrappers (* _ ~ `) that users might apply to commands
    let stripped = trimmed.trim_matches(|c| c == '*' || c == '_' || c == '~' || c == '`');
    
    if stripped.starts_with('#') {
        match parse_command(stripped) {
            Some(cmd) => MessageType::Command(cmd),
            None => {
                let cmd_word = stripped.split_whitespace()
                    .next()
                    .unwrap_or(stripped);
                MessageType::Command(BotCommand::UnknownCommand(cmd_word.to_string()))
            }
        }
    } else {
        // pass original text to AI, not stripped
        MessageType::NeedsAI(text.to_string()) 
    }
}





fn parse_command(text: &str) -> Option<BotCommand> {
    let trimmed = text.trim();
    
    
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

                let id = parts[1].parse().ok()?;
                let message = parts[2..].join(" ");
                Some(BotCommand::Update(id, message))
            } else {
                Some(BotCommand::MissingArgument("update".to_string()))
            }
        },

       "daily" => {
            if parts.len() > 1 {       
                if let Ok(status) = parts[1].parse() {
                    Some(BotCommand::Daily(status))
                } else {
                    Some(BotCommand::MissingArgument("daily".to_string()))
                }
            } else {
                Some(BotCommand::MissingArgument("daily".to_string()))
            }
        },

        // --- Set Kelas ---
        "setkelas" | "set" => {
            if parts.len() >= 2 {
                let first_arg = parts[1].to_lowercase();
                
                // Check if user is using package shortcut (#setkelas paket1 .. paket5 or #setkelas paket 1 .. 5)
                if first_arg == "paket" {
                    if parts.len() == 2 {
                        // #setkelas paket -> show summary of packages
                        return Some(BotCommand::SetKelasPaket(0));
                    } else if let Ok(num) = parts[2].parse::<u8>() {
                        // #setkelas paket 1 .. 5
                        return Some(BotCommand::SetKelasPaket(num));
                    } else {
                        // #setkelas paket info / invalid -> show summary of packages
                        return Some(BotCommand::SetKelasPaket(0));
                    }
                } else if first_arg.starts_with("paket") {
                    let suffix = &first_arg["paket".len()..];
                    let trimmed_suffix = suffix.trim_start_matches(|c| c == '-' || c == '_');
                    if let Ok(num) = trimmed_suffix.parse::<u8>() {
                        // #setkelas paket1 .. paket5
                        return Some(BotCommand::SetKelasPaket(num));
                    }
                }
            }

            if parts.len() >= 3 {
                let mut split_idx = parts.len();
                
                for i in (2..parts.len()).rev() {
                    let token = parts[i];
                    let is_code = token.eq_ignore_ascii_case("all") || token.eq_ignore_ascii_case("none") || token.eq_ignore_ascii_case("non_asah") || token.len() <= 3;
                    
                    if is_code {
                        split_idx = i;
                    } else {
                        break;
                    }
                }
                
                let matkul = parts[1..split_idx].join(" ");
                let codes: Vec<String> = parts[split_idx..]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

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
       

        "tugas" | "tygas" | "tgs" => {

            if parts.len() > 1 {
                if let Ok(id) = parts[1].parse() {
                    return Some(BotCommand::Expand(id));
                }
            }
            Some(BotCommand::Tugas)
        },
        "mykelas"      => Some(BotCommand::MyKelas),
        "today"        => Some(BotCommand::Today),
        "week"         => Some(BotCommand::Week),
        "todo" | "td"  => Some(BotCommand::Todo),  
        "help" | "hlp" => Some(BotCommand::Help),
        "undo" | "und" => Some(BotCommand::Undo),
        "done" | "dn"  => {
            if parts.len() > 1 {
                let id = parts[1].parse().ok()?;
                Some(BotCommand::Done(id))
            } else {
               
                Some(BotCommand::MissingArgument("done".to_string()))
            }
        },
        "delete" | "hapus" => {
            if parts.len() > 1 {
                let id = parts[1].parse().ok()?;
                Some(BotCommand::Delete(id))
            } else {
              
                Some(BotCommand::MissingArgument("delete".to_string()))
            }
        },
        "expand" => {
            if parts.len() > 1 {
                let id = parts[1].parse().ok()?;
                Some(BotCommand::Expand(id))
            } else {
              
                Some(BotCommand::MissingArgument("expand".to_string()))
            }
        },
        "announcement" | "announce" | "ann" => {
            let after_cmd = without_hash[parts[0].len()..]
                .trim_start_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r');
            Some(BotCommand::Announcement(after_cmd.to_string()))
        }
        "apikey" | "api" => {
            let after_cmd = without_hash[parts[0].len()..]
                .trim_start_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r');
            Some(BotCommand::ApiKey(after_cmd.to_string()))
        }
        "apidocs" | "apidoc" => Some(BotCommand::ApiDocs),
        _ if command.chars().all(|c| c.is_numeric()) => {
            let id = command.parse().ok()?;
            Some(BotCommand::Expand(id))
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_setkelas_paket() {
        // Test #setkelas paket1 .. paket5
        for i in 1..=5 {
            let msg = format!("#setkelas paket{}", i);
            match classify_message(&msg) {
                MessageType::Command(BotCommand::SetKelasPaket(num)) => assert_eq!(num, i),
                other => panic!("Expected SetKelasPaket({}), got {:?}", i, other),
            }
        }

        // Test #setkelas paket 1 .. 5
        for i in 1..=5 {
            let msg = format!("#setkelas paket {}", i);
            match classify_message(&msg) {
                MessageType::Command(BotCommand::SetKelasPaket(num)) => assert_eq!(num, i),
                other => panic!("Expected SetKelasPaket({}), got {:?}", i, other),
            }
        }

        // Test #setkelas paket-1 / #setkelas paket_1
        match classify_message("#setkelas paket-1") {
            MessageType::Command(BotCommand::SetKelasPaket(num)) => assert_eq!(num, 1),
            other => panic!("Expected SetKelasPaket(1), got {:?}", other),
        }

        // Test #setkelas paket (shows summary, num = 0)
        match classify_message("#setkelas paket") {
            MessageType::Command(BotCommand::SetKelasPaket(num)) => assert_eq!(num, 0),
            other => panic!("Expected SetKelasPaket(0), got {:?}", other),
        }

        // Test #setkelas without args returns MissingArgument
        match classify_message("#setkelas") {
            MessageType::Command(BotCommand::MissingArgument(cmd)) => assert_eq!(cmd, "setkelas"),
            other => panic!("Expected MissingArgument, got {:?}", other),
        }

        // Test standard individual setkelas
        match classify_message("#setkelas analgor k1 r1") {
            MessageType::Command(BotCommand::SetKelas(matkul, codes)) => {
                assert_eq!(matkul, "analgor");
                assert_eq!(codes, vec!["k1", "r1"]);
            }
            other => panic!("Expected SetKelas, got {:?}", other),
        }

        // Verify #paket1 is NOT parsed as command (user requested no top-level #paket1)
        match classify_message("#paket1") {
            MessageType::Command(BotCommand::UnknownCommand(cmd)) => assert_eq!(cmd, "#paket1"),
            other => panic!("Expected UnknownCommand for #paket1, got {:?}", other),
        }
    }
}