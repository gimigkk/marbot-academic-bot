mod core;
mod prompts;
mod parsing;

pub mod schedule_oracle;
mod context_builder;

// ===== MODEL CONFIGURATION =====

// Groq reasoning models (PRIORITY - best for complex logic)
pub const GROQ_REASONING_MODELS: &[&str; 3] = &[
    "openai/gpt-oss-120b",            // Rank 1: Flagship reasoning (Replace DeepSeek R1)
    "llama-3.3-70b-versatile",        // Rank 2: The most reliable 70B (300k TPM limit)
    "qwen/qwen3-32b",                 // Rank 3: Dense logic model, punches above weight, limited to 32k context which is really bad
];

// Groq vision models (multimodal - for image processing)
pub const GROQ_VISION_MODELS: &[&str; 2] = &[
    "meta-llama/llama-4-maverick-17b-128e-instruct",  // Rank 1: High fidelity (New architecture)
    "meta-llama/llama-4-scout-17b-16e-instruct",      // Rank 2: High speed multimodal
];

// Groq standard text models (fallback - non-reasoning)
pub const GROQ_TEXT_MODELS: &[&str; 2] = &[
    "openai/gpt-oss-20b",       // Primary text cruncher
    "llama-3.1-8b-instant",     // Ultimate fallback (Fastest/Cheapest)
];

// Gemini models (final fallback - reliable, 1M context window)
pub const GEMINI_MODELS: &[&str; 4] = &[
    "gemini-3-flash-preview",     // Preview - latest balanced model
    "gemini-3-pro-preview",       // Preview - most intelligent
    "gemini-2.5-flash",           // Stable - best price-performance (RECOMMENDED)
    "gemini-2.5-pro",             // Stable - advanced thinking model
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
