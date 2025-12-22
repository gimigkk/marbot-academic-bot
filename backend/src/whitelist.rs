use std::collections::HashSet;

/// Whitelist configuration for academic channels/groups
pub struct Whitelist {
    /// Chat IDs that are allowed to send academic info
    /// Format: "6281234567890@c.us" for DMs or "123456789@g.us" for groups or "123@newsletter" for channels
    academic_channels: HashSet<String>,
}

impl Whitelist {
    pub fn new() -> Self {
        let mut academic_channels = HashSet::new();
        
        // Load from environment or config file
        if let Ok(channels) = std::env::var("ACADEMIC_CHANNELS") {
            for channel in channels.split(',') {
                let trimmed = channel.trim();
                if !trimmed.is_empty() {
                    academic_channels.insert(trimmed.to_string());
                    println!("📝 Whitelisted academic channel: {}", trimmed);
                }
            }
        }
        
        // Default fallback if no env var is set
        if academic_channels.is_empty() {
            println!("⚠️  No ACADEMIC_CHANNELS configured. Add to .env file:");
            println!("   ACADEMIC_CHANNELS=120363423034679598@newsletter");
        }
        
        Self { academic_channels }
    }
    
    /// Check if a chat is whitelisted for academic info
    pub fn is_academic_channel(&self, chat_id: &str) -> bool {
        self.academic_channels.contains(chat_id)
    }
    
    /// Check if we should process this message
    /// Returns (should_process, reason)
    pub fn should_process(&self, chat_id: &str, is_command: bool) -> (bool, &'static str) {
        // Commands can come from ANYWHERE (DMs, groups, channels)
        if is_command {
            return (true, "command");
        }
        
        // Non-command messages ONLY from academic channels
        if self.is_academic_channel(chat_id) {
            (true, "academic_channel")
        } else {
            (false, "not_whitelisted")
        }
    }
    
    /// Add a channel to whitelist (useful for testing)
    #[allow(dead_code)]
    pub fn add_channel(&mut self, chat_id: String) {
        self.academic_channels.insert(chat_id);
    }
}

impl Default for Whitelist {
    fn default() -> Self {
        Self::new()
    }
}