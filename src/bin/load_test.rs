use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{error, info};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let addr = "127.0.0.1:8000";
    let mount = "/stream";

    info!("Spawning 1 source...");
    let source_handle = tokio::spawn(async move {
        match TcpStream::connect(addr).await {
            Ok(mut stream) => {
                // "source:hackme" in base64 is "c291cmNlOmhhY2ttZQ=="
                let req = format!(
                    "SOURCE {} HTTP/1.1\r\nAuthorization: Basic c291cmNlOmhhY2ttZQ==\r\n\r\n",
                    mount
                );
                if stream.write_all(req.as_bytes()).await.is_ok() {
                    let dummy_data = [0xAA; 4096];
                    for _ in 0..100 {
                        if stream.write_all(&dummy_data).await.is_err() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    info!("Source finished sending dummy data.");
                }
            }
            Err(e) => error!("Source failed to connect: {}", e),
        }
    });

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Spawn listeners
    let num_listeners = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1000".to_string())
        .parse::<usize>()
        .unwrap_or(1000);
    info!("Spawning {} listeners...", num_listeners);

    let mut join_handles = Vec::new();

    for i in 0..num_listeners {
        let m = mount.to_string();
        let h = tokio::spawn(async move {
            match TcpStream::connect(addr).await {
                Ok(mut stream) => {
                    let req = format!("GET {} HTTP/1.1\r\n\r\n", m);
                    if stream.write_all(req.as_bytes()).await.is_ok() {
                        let mut buf = [0u8; 1024];
                        loop {
                            match stream.read(&mut buf).await {
                                Ok(0) => break,
                                Ok(_) => {}
                                Err(_) => break,
                            }
                            tokio::task::yield_now().await;
                        }
                    }
                }
                Err(e) => error!("Listener {} failed to connect: {}", i, e),
            }
        });
        join_handles.push(h);
    }

    let _ = source_handle.await;

    info!("Load test complete. If the server is still running without panicking, the test passes!");

    Ok(())
}
