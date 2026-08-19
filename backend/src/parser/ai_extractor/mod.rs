mod core;
mod prompts;
mod parsing;

pub mod schedule_oracle;
mod context_builder;

// ===== MODEL CONFIGURATION =====

// Groq reasoning models (PRIORITY - best for complex logic)
pub const GROQ_REASONING_MODELS: &[&str; 4] = &[
    "openai/gpt-oss-120b",            // Rank 1: Flagship reasoning
    "qwen/qwen3.6-27b",               // Rank 2: High capacity reasoning model
    "groq/compound",                  // Rank 3: Compound logic engine
    "openai/gpt-oss-20b",             // Rank 4: Dense text fallback
];

// Groq vision models (multimodal - for image processing)
pub const GROQ_VISION_MODELS: &[&str; 2] = &[
    "qwen/qwen3.6-27b",               // Rank 1: Multimodal vision model
    "groq/compound",                  // Rank 2: Compound engine fallback
];

// Groq standard text models (fallback - non-reasoning)
pub const GROQ_TEXT_MODELS: &[&str; 4] = &[
    "openai/gpt-oss-20b",             // Rank 1: Primary fast text model
    "groq/compound-mini",             // Rank 2: Fast compound mini
    "allam-2-7b",                     // Rank 3: Lightweight text model
    "openai/gpt-oss-safeguard-20b",   // Rank 4: Safeguard fallback
];

// Gemini models (final fallback - 100% verified working live)
pub const GEMINI_MODELS: &[&str; 5] = &[
    "gemini-3.7-flash",               // Rank 1: Flagship 3.7 Flash model
    "gemini-3.5-flash",               // Rank 2: 3.5 Flash engine
    "gemini-3.1-flash-lite",          // Rank 3: Lightweight 3.1 Flash
    "gemini-2.5-flash",               // Rank 4: 2.5 Flash model
    "gemini-2.5-flash-lite",          // Rank 5: 2.5 Flash Lite fallback
];

// ===== PUBLIC API =====

pub use core::{extract_with_ai, match_update_to_assignment, check_duplicate_assignment};
pub use schedule_oracle::ScheduleOracle;
pub use context_builder::build_context;

pub use parsing::{
    extract_numbers,
    GeminiResponse,
    GroqResponse,
    extract_ai_text,
    extract_groq_text,
};

// ===== HELPER =====

pub fn build_course_map_from_db_results(
    courses: &[(uuid::Uuid, String)]
) -> std::collections::HashMap<uuid::Uuid, String> {
    courses.iter()
        .map(|(id, name)| (*id, name.clone()))
        .collect()
}
