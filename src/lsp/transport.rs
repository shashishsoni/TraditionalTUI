//! LSP `Content-Length` framing codec.

use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// Newline/header framing for LSP JSON-RPC over stdio.
#[derive(Debug, Default)]
pub struct LspFrameCodec;

impl LspFrameCodec {
    pub fn new() -> Self {
        Self
    }
}

impl Decoder for LspFrameCodec {
    type Item = serde_json::Value;
    type Error = std::io::Error;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let Some(header_end) = find_header_end(source) else {
            return Ok(None);
        };
        let header = &source[..header_end];
        let content_length = parse_content_length(header)?;
        let total = header_end + 4 + content_length;
        if source.len() < total {
            source.reserve(total - source.len());
            return Ok(None);
        }
        let mut frame = source.split_to(total);
        frame.advance(header_end + 4);
        let value = serde_json::from_slice(&frame).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid LSP JSON payload: {error}"),
            )
        })?;
        Ok(Some(value))
    }
}

impl Encoder<serde_json::Value> for LspFrameCodec {
    type Error = std::io::Error;

    fn encode(
        &mut self,
        item: serde_json::Value,
        destination: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        let body = serde_json::to_vec(&item).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        destination.extend_from_slice(header.as_bytes());
        destination.extend_from_slice(&body);
        Ok(())
    }
}

fn find_header_end(source: &[u8]) -> Option<usize> {
    source.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_content_length(header: &[u8]) -> Result<usize, std::io::Error> {
    for line in header.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        let name = String::from_utf8_lossy(&line[..colon]);
        if name.eq_ignore_ascii_case("content-length") {
            let value = String::from_utf8_lossy(&line[colon + 1..]);
            return value.trim().parse::<usize>().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid Content-Length header",
                )
            });
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "missing Content-Length header",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(value: serde_json::Value) -> BytesMut {
        let mut codec = LspFrameCodec::new();
        let mut buffer = BytesMut::new();
        codec.encode(value, &mut buffer).expect("encode");
        buffer
    }

    #[test]
    fn round_trips_a_single_message() {
        let value = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        let mut buffer = encode(value.clone());
        let mut codec = LspFrameCodec::new();
        assert_eq!(codec.decode(&mut buffer).expect("decode"), Some(value));
        assert!(codec.decode(&mut buffer).expect("decode").is_none());
    }

    #[test]
    fn waits_for_fragmented_payloads() {
        let value = serde_json::json!({"jsonrpc": "2.0", "id": 2, "result": {"ok": true}});
        let full = encode(value.clone());
        let mut codec = LspFrameCodec::new();
        let mut buffer = BytesMut::new();
        buffer.extend_from_slice(&full[..7]);
        assert!(codec.decode(&mut buffer).expect("partial").is_none());
        buffer.extend_from_slice(&full[7..]);
        assert_eq!(codec.decode(&mut buffer).expect("complete"), Some(value));
    }

    #[test]
    fn decodes_back_to_back_messages() {
        let first = serde_json::json!({"id": 1});
        let second = serde_json::json!({"id": 2});
        let mut buffer = encode(first.clone());
        buffer.extend_from_slice(&encode(second.clone()));
        let mut codec = LspFrameCodec::new();
        assert_eq!(codec.decode(&mut buffer).expect("first"), Some(first));
        assert_eq!(codec.decode(&mut buffer).expect("second"), Some(second));
    }

    #[test]
    fn missing_content_length_is_an_error() {
        let mut buffer = BytesMut::from(&b"X-Test: 1\r\n\r\n{}"[..]);
        let mut codec = LspFrameCodec::new();
        assert!(codec.decode(&mut buffer).is_err());
    }

    #[test]
    fn header_names_are_case_insensitive() {
        let body = br#"{"id":1}"#;
        let mut buffer =
            BytesMut::from(format!("content-length: {}\r\n\r\n", body.len()).as_bytes());
        buffer.extend_from_slice(body);
        let mut codec = LspFrameCodec::new();
        assert!(codec.decode(&mut buffer).expect("decode").is_some());
    }
}
