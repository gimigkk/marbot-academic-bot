use crate::models::AIClassification;
use serde_json::json;

/// Extract structured info from message using Gemini AI
pub async fn extract_with_ai(text: &str) -> Result<AIClassification, Box<dyn std::error::Error>> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY not set")?;
    
    // Use Gemini 2.5 Flash
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );
    
    let prompt = format!(
        r#"Classify WhatsApp message. JSON ONLY. Be EXTREMELY brief.

Message: "{}"

Output ONE of these (keep ALL fields under 50 chars):

assignment_info:
{{"type":"assignment_info","title":"SHORT title","due_date":"2025-11-30","description":"SHORT desc"}}

course_info:
{{"type":"course_info","content":"SHORT summary"}}

assignment_reminder:
{{"type":"assignment_reminder","assignment_reference":"SHORT ref"}}

unrecognized:
{{"type":"unrecognized"}}

CRITICAL: Keep title/description/content UNDER 50 characters. Extract only key info."#,
        text
    );
    
    let request_body = json!({
        "contents": [{
            "parts": [{
                "text": prompt
            }]
        }],
        "generationConfig": {
            "temperature": 0.1,
            "maxOutputTokens": 256,
            "responseMimeType": "application/json"
        }
    });
    
    println!("🤖 Sending to Gemini AI...");
    
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await?;
        
        // Check if it's a rate limit error
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            eprintln!("⚠️  Rate limit hit, will retry later");
            // Parse retry delay from error if available
            if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(retry_delay) = error_json
                    .get("error")
                    .and_then(|e| e.get("details"))
                    .and_then(|d| d.as_array())
                    .and_then(|arr| arr.iter().find(|item| {
                        item.get("@type").and_then(|t| t.as_str()) == Some("type.googleapis.com/google.rpc.RetryInfo")
                    }))
                    .and_then(|retry| retry.get("retryDelay"))
                    .and_then(|d| d.as_str())
                {
                    eprintln!("   Suggested retry delay: {}", retry_delay);
                }
            }
        }
        
        return Err(format!("Gemini API error {}: {}", status, error_text).into());
    }
    
    let response_json: serde_json::Value = response.json().await?;
    
    // Extract text from Gemini response
    let ai_text = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or("Failed to parse Gemini response")?;
    
    println!("🤖 Gemini response: {}", ai_text);
    
    // Clean up response (remove markdown code blocks if present)
    let cleaned = ai_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    
    println!("🧹 Cleaned response: {}", cleaned);
    
    // Check if response looks truncated (missing closing brace or incomplete)
    if !cleaned.ends_with('}') || cleaned.matches('{').count() != cleaned.matches('}').count() {
        eprintln!("⚠️  Response appears truncated, classifying as course_info");
        // Fallback: treat truncated academic content as course_info
        return Ok(AIClassification::CourseInfo {
            content: "Academic announcement (full text in original message)".to_string(),
            original_message: None,
        });
    }
    
    // Parse JSON response
    let classification: AIClassification = serde_json::from_str(cleaned)
        .map_err(|e| {
            eprintln!("❌ JSON parse error: {}", e);
            eprintln!("   Raw response: {}", ai_text);
            eprintln!("   Cleaned: {}", cleaned);
            format!("Failed to parse AI response as JSON: {}", e)
        })?;
    
    Ok(classification)
}