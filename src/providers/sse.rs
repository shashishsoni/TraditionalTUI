//! SSE/NDJSON parsing helpers shared by provider adapters.

use serde_json::Value;

/// Decode one `eventsource-stream` event's JSON data payload.
pub fn json_event(data: &str) -> Option<Value> {
    let trimmed = data.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

/// Split a byte buffer into complete NDJSON lines, retaining the remainder.
pub fn take_lines(buffer: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    for index in 0..buffer.len() {
        if buffer[index] == b'\n' {
            let line = String::from_utf8_lossy(&buffer[start..index])
                .trim_end_matches('\r')
                .to_string();
            if !line.is_empty() {
                lines.push(line);
            }
            start = index + 1;
        }
    }
    if start > 0 {
        buffer.drain(..start);
    }
    lines
}

/// Reject plaintext HTTP for non-loopback hosts.
pub fn validate_endpoint(endpoint: &str) -> Result<(), super::ProviderError> {
    let url = url::Url::parse(endpoint)
        .map_err(|error| super::ProviderError::InvalidEndpoint(format!("{endpoint}: {error}")))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = url.host_str().unwrap_or_default();
            let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]");
            if loopback {
                Ok(())
            } else {
                Err(super::ProviderError::InvalidEndpoint(format!(
                    "plaintext HTTP is only allowed for loopback endpoints: {endpoint}"
                )))
            }
        }
        other => Err(super::ProviderError::InvalidEndpoint(format!(
            "unsupported scheme {other}: {endpoint}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndjson_lines_survive_partial_frames() {
        let mut buffer = b"{\"a\":1}\n{\"b\"".to_vec();
        let lines = take_lines(&mut buffer);
        assert_eq!(lines, vec!["{\"a\":1}".to_string()]);
        assert_eq!(buffer, b"{\"b\"");
        buffer.extend_from_slice(b":2}\n");
        assert_eq!(take_lines(&mut buffer), vec!["{\"b\":2}".to_string()]);
    }

    #[test]
    fn done_marker_is_not_json() {
        assert!(json_event("[DONE]").is_none());
        assert_eq!(
            json_event("{\"x\":1}").and_then(|value| value.get("x").cloned()),
            Some(serde_json::json!(1))
        );
    }

    #[test]
    fn endpoint_validation_enforces_tls() {
        assert!(validate_endpoint("https://api.openai.com/v1").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:11434").is_ok());
        assert!(validate_endpoint("http://example.com").is_err());
        assert!(validate_endpoint("ftp://example.com").is_err());
    }
}
