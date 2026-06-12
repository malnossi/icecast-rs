use crate::config::Config;
use crate::state::SourceMetadata;
use base64::prelude::*;

pub enum Method {
    Get,
    Source,
    Put,
    Unknown(String),
}

pub struct HttpRequest {
    pub method: Method,
    pub path: String,
    pub body_start: usize,
    pub(crate) metadata: SourceMetadata,
    pub user_agent: Option<String>,
    pub host: Option<String>,
}

pub fn check_auth(request_str: &str, config: &Config) -> bool {
    let expected_user = config.source.username.as_deref().unwrap_or("");
    let expected_pass = config.source.password.as_deref().unwrap_or("");

    if expected_user.is_empty() && expected_pass.is_empty() {
        return true;
    }

    for line in request_str.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("authorization: basic ") {
            let b64 = line[21..].trim();
            if let Ok(decoded) = BASE64_STANDARD.decode(b64) {
                let creds = String::from_utf8(decoded).unwrap_or_default();
                let mut parts = creds.splitn(2, ':');
                let user = parts.next().unwrap_or("");
                let pass = parts.next().unwrap_or("");
                if user == expected_user && pass == expected_pass {
                    return true;
                }
            }
        }
    }
    false
}

pub fn parse_request(request_str: &str) -> Option<HttpRequest> {
    let mut lines = request_str.lines();
    if let Some(request_line) = lines.next() {
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 3 {
            return None;
        }

        let method_str = parts[0];
        let path = parts[1].to_string();

        let method = match method_str {
            "GET" => Method::Get,
            "SOURCE" => Method::Source,
            "PUT" => Method::Put,
            _ => Method::Unknown(method_str.to_string()),
        };

        let body_start = request_str
            .find("\r\n\r\n")
            .map(|i| i + 4)
            .or_else(|| request_str.find("\n\n").map(|i| i + 2))
            .unwrap_or(request_str.len());

        let mut metadata = SourceMetadata::default();
        let mut user_agent = None;
        let mut host = None;
        for line in lines {
            if line.is_empty() {
                continue; // Can't break if there are empty lines before \r\n\r\n unless we know it's the end, but actually the string is up to the body.
            }
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_lowercase();
                let val = parts[1].trim().to_string();
                match key.as_str() {
                    "content-type" => {
                        metadata.content_type = if val == "application/ogg" {
                            "audio/ogg".to_string()
                        } else {
                            val
                        };
                    }
                    "ice-name" | "icy-name" => metadata.name = Some(val),
                    "ice-description" | "icy-description" => metadata.description = Some(val),
                    "ice-genre" | "icy-genre" => metadata.genre = Some(val),
                    "ice-url" | "icy-url" => metadata.url = Some(val),
                    "ice-bitrate" | "icy-br" => metadata.bitrate = Some(val),
                    "ice-public" | "icy-pub" => metadata.is_public = Some(val),
                    "ice-audio-info" => metadata.audio_info = Some(val),
                    "user-agent" => user_agent = Some(val),
                    "host" => host = Some(val),
                    _ => {}
                }
            }
        }

        Some(HttpRequest {
            method,
            path,
            body_start,
            metadata,
            user_agent,
            host,
        })
    } else {
        None
    }
}
