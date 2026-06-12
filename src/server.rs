use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::handlers::{handle_listener, handle_source, handle_status_page};
use crate::protocol::{self, Method};
use crate::state::State;

pub async fn run_server(addr: &str, state: State, config: Arc<Config>) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("Icecast-RS Studio listening on {}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                info!("New connection from: {}", peer_addr);
                let state = state.clone();
                let cfg = config.clone();
                tokio::spawn(async move {
                    handle_connection(stream, state, cfg).await;
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

async fn handle_connection(mut stream: TcpStream, state: State, config: Arc<Config>) {
    let peer_addr = match stream.peer_addr() {
        Ok(addr) => addr,
        Err(e) => {
            error!("Failed to get peer address: {}", e);
            return;
        }
    };

    let mut buffer = [0u8; 4096];
    let mut request_str = String::new();
    let mut total_read = 0;

    loop {
        match stream.read(&mut buffer[total_read..]).await {
            Ok(0) => return,
            Ok(n) => {
                let chunk = &buffer[total_read..total_read + n];
                total_read += n;
                request_str.push_str(&String::from_utf8_lossy(chunk));
                if request_str.contains("\r\n\r\n") || request_str.contains("\n\n") {
                    break;
                }
                if total_read >= buffer.len() {
                    warn!("Request headers too large");
                    return;
                }
            }
            Err(e) => {
                error!("Failed to read request: {}", e);
                return;
            }
        }
    }

    if let Some(req) = protocol::parse_request(&request_str) {
        let real_ip = peer_addr.ip().to_string();

        match (&req.method, req.path.as_str()) {
            (Method::Get, "/") => {
                handle_status_page(stream, state, req.host).await;
            }
            (Method::Get, path) => {
                handle_listener(stream, path.to_string(), state, req.user_agent, real_ip, config.server.burst_size).await;
            }
            (Method::Source | Method::Put, path) => {
                if !protocol::check_auth(&request_str, &config) {
                    warn!("Unauthorized source attempt for {}", path);
                    let resp = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Icecast\"\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }

                if let Err(e) = stream.write_all(b"HTTP/1.0 200 OK\r\n\r\n").await {
                    error!("Failed to write source OK response: {}", e);
                    return;
                }

                let leftover = &buffer[req.body_start..total_read];
                handle_source(stream, path.to_string(), state, leftover, req.metadata, config.server.burst_size, real_ip).await;
            }
            (Method::Unknown(m), _) => {
                info!("Unsupported method: {}", m);
                let _ = stream.write_all(b"HTTP/1.0 405 Method Not Allowed\r\n\r\n").await;
            }
        }
    } else {
        error!("Invalid request line, closing socket with 400 Bad Request");
        let _ = stream.write_all(b"HTTP/1.0 400 Bad Request\r\n\r\n").await;
    }
}
