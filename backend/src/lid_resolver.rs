// backend/src/lid_resolver.rs - In-memory WhatsApp LID (@lid) to Phone Number (@c.us) Resolver

use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::time::Duration;

static LID_CACHE: Lazy<DashMap<String, String>> = Lazy::new(DashMap::new);

/// Check if a JID is a WhatsApp LID identifier
pub fn is_lid(jid: &str) -> bool {
    jid.trim().ends_with("@lid")
}

/// Resolve a WhatsApp LID (e.g. 63436935458958@lid) to a standard Phone Number JID (e.g. 6281574844481@c.us).
/// Non-LID JIDs (such as @c.us or @g.us) are returned immediately with zero overhead.
pub async fn resolve_lid(jid: &str) -> String {
    let trimmed = jid.trim();
    if !is_lid(trimmed) {
        return trimmed.to_string();
    }

    // 1. Check in-memory cache
    if let Some(cached) = LID_CACHE.get(trimmed) {
        return cached.value().clone();
    }

    // 2. Query WAHA LIDs API
    let waha_url = std::env::var("WAHA_URL").unwrap_or_else(|_| "http://waha:3000".to_string());
    let api_key = std::env::var("WAHA_API_KEY").unwrap_or_else(|_| "devkey123".to_string());
    let base_url = waha_url.trim_end_matches('/');

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    // Primary endpoint: GET /api/default/lids/{lid}
    let lid_url = format!("{}/api/default/lids/{}", base_url, trimmed);
    if let Ok(res) = client.get(&lid_url).header("X-Api-Key", &api_key).send().await {
        if res.status().is_success() {
            if let Ok(json) = res.json::<serde_json::Value>().await {
                if let Some(phone_jid) = extract_phone_jid(&json) {
                    println!("🔄 [LID Resolver] Resolved {} -> {}", trimmed, phone_jid);
                    LID_CACHE.insert(trimmed.to_string(), phone_jid.clone());
                    return phone_jid;
                }
            }
        }
    }

    // Fallback endpoint: GET /api/default/contacts/{contactId}
    let contact_url = format!("{}/api/default/contacts/{}", base_url, trimmed);
    if let Ok(res) = client.get(&contact_url).header("X-Api-Key", &api_key).send().await {
        if res.status().is_success() {
            if let Ok(json) = res.json::<serde_json::Value>().await {
                if let Some(phone_jid) = extract_phone_jid(&json) {
                    println!("🔄 [LID Resolver] Resolved via contacts {} -> {}", trimmed, phone_jid);
                    LID_CACHE.insert(trimmed.to_string(), phone_jid.clone());
                    return phone_jid;
                }
            }
        }
    }

    // If resolution fails, return original JID as fallback
    trimmed.to_string()
}

/// Helper to extract phone JID (@c.us) from WAHA response JSON
fn extract_phone_jid(json: &serde_json::Value) -> Option<String> {
    // Check "pn" field (Phone Number JID)
    if let Some(pn) = json.get("pn").and_then(|v| v.as_str()) {
        let pn = pn.trim();
        if !pn.is_empty() && !pn.ends_with("@lid") {
            return Some(format_as_c_us(pn));
        }
    }

    // Check "id" field if it is not an @lid
    if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
        let id = id.trim();
        if !id.is_empty() && !id.ends_with("@lid") {
            return Some(format_as_c_us(id));
        }
    }

    // Check "number" / "phoneNumber" field
    if let Some(num) = json.get("number").or_else(|| json.get("phoneNumber")).and_then(|v| v.as_str()) {
        let num = num.trim();
        if !num.is_empty() {
            return Some(format_as_c_us(num));
        }
    }

    None
}

/// Ensure phone number is formatted with @c.us suffix
fn format_as_c_us(phone: &str) -> String {
    let clean = phone.trim();
    if clean.ends_with("@c.us") || clean.ends_with("@s.whatsapp.net") {
        clean.to_string()
    } else {
        let digits: String = clean.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            clean.to_string()
        } else {
            format!("{}@c.us", digits)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_lid() {
        assert!(is_lid("63436935458958@lid"));
        assert!(is_lid(" 12345678@lid "));
        assert!(!is_lid("6281574844481@c.us"));
        assert!(!is_lid("120363xxx@g.us"));
    }

    #[test]
    fn test_extract_phone_jid() {
        let json1 = serde_json::json!({
            "id": "6281574844481@c.us",
            "pn": "6281574844481@c.us",
            "lid": "63436935458958@lid"
        });
        assert_eq!(extract_phone_jid(&json1), Some("6281574844481@c.us".to_string()));

        let json2 = serde_json::json!({
            "number": "6281574844481",
            "lid": "63436935458958@lid"
        });
        assert_eq!(extract_phone_jid(&json2), Some("6281574844481@c.us".to_string()));

        let json_lid_only = serde_json::json!({
            "id": "63436935458958@lid",
            "lid": "63436935458958@lid"
        });
        assert_eq!(extract_phone_jid(&json_lid_only), None);
    }

    #[tokio::test]
    async fn test_resolve_non_lid_immediate() {
        let regular_user = "6281234567890@c.us";
        assert_eq!(resolve_lid(regular_user).await, regular_user);

        let group_chat = "120363123456789@g.us";
        assert_eq!(resolve_lid(group_chat).await, group_chat);
    }

    #[tokio::test]
    async fn test_cache_insertion() {
        let lid = "9999999999@lid";
        let pn = "6289999999999@c.us";
        LID_CACHE.insert(lid.to_string(), pn.to_string());

        assert_eq!(resolve_lid(lid).await, pn);
    }
}
