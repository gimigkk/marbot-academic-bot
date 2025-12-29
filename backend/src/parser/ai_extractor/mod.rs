mod core;
mod prompts;
mod parsing;

// ===== MODEL CONFIGURATION =====

// Groq models (frontline - fast, vision-capable, high rate limits)
// Context windows: Scout 128K, Maverick 128K, Llama 3.3 128K, Mixtral 32K
pub const GROQ_VISION_MODELS: &[&str] = &[
    "meta-llama/llama-4-scout-17b-16e-instruct",      // 128K context, fast multimodal
    "meta-llama/llama-4-maverick-17b-128e-instruct",  // 128K context, powerful
];

pub const GROQ_TEXT_MODELS: &[&str] = &[
    "llama-3.3-70b-versatile",  // 128K context
    "mixtral-8x7b-32768",       // 32K context
];

// Gemini models (fallback - reliable, 1M context window for complex reasoning)
pub const GEMINI_MODELS: &[&str] = &[
    "gemini-3-flash-preview",   // 1M context
    "gemini-2.5-flash",         // 1M context
    "gemini-2.5-flash-lite",    // 1M context
];

// ===== PUBLIC API =====

pub use core::{extract_with_ai, match_update_to_assignment};

// ===== HELPER (for external use) =====

/// Helper to build course_map from assignments and a course lookup
/// This is a convenience function - you can also pass the map directly
pub fn build_course_map_from_db_results(
    courses: &[(uuid::Uuid, String)]  // List of (course_id, course_name) tuples
) -> std::collections::HashMap<uuid::Uuid, String> {
    courses.iter()
        .map(|(id, name)| (*id, name.clone()))
        .collect()
}