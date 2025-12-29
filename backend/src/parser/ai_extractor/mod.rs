mod core;
mod prompts;
mod parsing;

// ===== MODEL CONFIGURATION =====

// Groq reasoning models (PRIORITY - best for complex logic)
pub const GROQ_REASONING_MODELS: &[&str] = &[
    "openai/gpt-oss-120b",            // OpenAI's flagship 120B reasoning model (recommended replacement)
    "deepseek-r1-distill-qwen-32b",   // DeepSeek R1 distilled into Qwen 32B (available, fast)
    "openai/gpt-oss-20b",             // Lighter GPT-OSS variant
];

// Groq vision models (multimodal - for image processing)
pub const GROQ_VISION_MODELS: &[&str] = &[
    "meta-llama/llama-4-scout-17b-16e-instruct",      // 128K context, fast multimodal
    "meta-llama/llama-4-maverick-17b-128e-instruct",  // 128K context, powerful
];

// Groq standard text models (fallback - non-reasoning)
pub const GROQ_TEXT_MODELS: &[&str] = &[
    "llama-3.3-70b-versatile",  // 128K context (recommended by Groq as replacement)
    "llama-3.1-8b-instant",     // Fast 8B model
];

// Gemini models (final fallback - reliable, 1M context window)
pub const GEMINI_MODELS: &[&str] = &[
    "gemini-3-flash-preview",     // Preview - latest balanced model
    "gemini-3-pro-preview",       // Preview - most intelligent
    "gemini-2.5-flash",           // Stable - best price-performance (RECOMMENDED)
    "gemini-2.5-pro",             // Stable - advanced thinking model
    "gemini-2.5-flash-lite",      // Stable - ultra fast, cost-efficient
];

// ===== PUBLIC API =====

pub use core::{extract_with_ai, match_update_to_assignment};

// ===== HELPER =====

pub fn build_course_map_from_db_results(
    courses: &[(uuid::Uuid, String)]
) -> std::collections::HashMap<uuid::Uuid, String> {
    courses.iter()
        .map(|(id, name)| (*id, name.clone()))
        .collect()
}
