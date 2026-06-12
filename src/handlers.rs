use bytes::Bytes;
use std::sync::atomic::Ordering;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{error, info, warn};

use crate::state::{ListenerGuard, State};

pub(crate) async fn handle_source(mut stream: TcpStream, path: String, state: State, leftover: &[u8], metadata: crate::state::SourceMetadata, _burst_size: usize, real_ip: String) {
    info!("Source {} connected to mountpoint: {}", real_ip, path);

    // Create or get the mountpoint
    let ring = {
        let mount = state.mounts.entry(path.clone()).or_insert_with(|| crate::state::Mountpoint::new(metadata));
        mount.ring.clone()
    };

    if !leftover.is_empty() {
        let bytes = Bytes::copy_from_slice(leftover);
        ring.push(bytes);
    }

    // Read from source and push to ring
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                info!("Source {} disconnected from: {}", real_ip, path);
                break;
            }
            Ok(n) => {
                let bytes = Bytes::copy_from_slice(&buffer[..n]);
                ring.push(bytes);
            }
            Err(e) => {
                error!("Error reading from source {}: {}", path, e);
                break;
            }
        }
    }

    // Cleanup mountpoint when source disconnects
    state.mounts.remove(&path);
}

pub(crate) async fn handle_listener(mut stream: TcpStream, path: String, state: State, user_agent: Option<String>, real_ip: String, burst_size: usize) {
    info!("Listener {} connecting to mountpoint: {}", real_ip, path);

    let mount_info = {
        if let Some(mount) = state.mounts.get(&path) {
            Some((
                mount.ring.clone(),
                mount.listeners.clone(),
                mount.mobile_listeners.clone(),
                mount.desktop_listeners.clone(),
                mount.metadata.clone()
            ))
        } else {
            None
        }
    }; // <-- DashMap lock is officially dropped HERE!

    let (ring, listeners_arc, mobile_listeners_arc, desktop_listeners_arc, metadata) = match mount_info {
        Some(info) => info,
        None => {
            warn!("Mountpoint not found for listener: {}", path);
            let _ = stream.write_all(b"HTTP/1.0 404 Not Found\r\n\r\n").await;
            return;
        }
    };

    // Send HTTP 200 OK headers
    let mut headers = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {}\r\n\
         Server: Icecast 2.4.4-compatible (Rust)\r\n\
         Accept-Ranges: none\r\n\
         X-Accel-Buffering: no\r\n\
         icy-notice1: <BR>This stream requires a shoutcast/icecast compatible player<BR>\r\n\
         icy-notice2: Rust Micro-Icecast Server\r\n",
        metadata.content_type
    );

    if let Some(name) = &metadata.name {
        headers.push_str(&format!("icy-name: {}\r\n", name));
    }
    if let Some(genre) = &metadata.genre {
        headers.push_str(&format!("icy-genre: {}\r\n", genre));
    }
    if let Some(url) = &metadata.url {
        headers.push_str(&format!("icy-url: {}\r\n", url));
    }
    if let Some(br) = &metadata.bitrate {
        headers.push_str(&format!("icy-br: {}\r\n", br));
    }

    headers.push_str(
        "Connection: close\r\n\
         Cache-Control: no-cache, no-store, must-revalidate\r\n\
         Pragma: no-cache\r\n\
         Expires: 0\r\n\r\n",
    );

    if let Err(e) = stream.write_all(headers.as_bytes()).await {
        error!("Failed to write listener headers: {}", e);
        return;
    }

    info!("Listener {} fully connected to mountpoint: {}", real_ip, path);

    // Instantiate ListenerGuard AFTER fully connected
    let is_mobile = {
        let ua = user_agent.unwrap_or_default().to_lowercase();
        ua.contains("mobi") || ua.contains("android") || ua.contains("iphone") || ua.contains("ipad")
    };
    let _guard = ListenerGuard::new(listeners_arc, mobile_listeners_arc, desktop_listeners_arc, is_mobile);

    // Burst history flush
    let head = ring.head.load(Ordering::Acquire);
    let mut local_index = head;
    let mut accum_bytes = 0;

    for offset in 1..=crate::state::RING_SIZE {
        let i = head.saturating_sub(offset);
        let frame = ring.buffer[i % crate::state::RING_SIZE].load_full();
        if frame.is_empty() {
            break;
        }
        accum_bytes += frame.len();
        if accum_bytes > burst_size {
            break;
        }
        local_index = i;
    }

    for i in local_index..head {
        let frame = ring.buffer[i % crate::state::RING_SIZE].load_full();
        if let Err(e) = stream.write_all(&frame).await {
            info!("Listener {} disconnected during burst dump on {}: {}", real_ip, path, e);
            return;
        }
    }
    local_index = head;

    loop {
        let head = ring.head.load(Ordering::Acquire);
        
        // Caught-Up Yielding
        if local_index == head {
            ring.notify.notified().await;
            continue;
        }
        
        // Lagging Client Protection
        if head.saturating_sub(local_index) >= crate::state::RING_SIZE {
            warn!("Listener {} lagged behind on {}! Skipping lost frames...", real_ip, path);
            // Fast-forward listener to half a buffer behind the live edge to resume cleanly
            local_index = head - (crate::state::RING_SIZE / 2);
        }
        
        // Read frame
        let frame = ring.buffer[local_index % crate::state::RING_SIZE].load_full();
        if let Err(e) = stream.write_all(&frame).await {
            info!("Listener {} disconnected from {}: {}", real_ip, path, e);
            break;
        }
        local_index += 1;
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&#39;")
}

pub async fn handle_status_page(mut stream: TcpStream, state: State, host_header: Option<String>) {
    let host = host_header.unwrap_or_else(|| "localhost:8000".to_string());
    let base_url = format!("http://{}", host);

    let mut body = String::from(
        r#"<!DOCTYPE html>
<html lang="en" class="dark">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Rust Micro-Icecast Status</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <script>
        tailwind.config = {
            darkMode: 'class',
            theme: {
                extend: {
                    colors: {
                        brand: '#3b82f6',
                    }
                }
            }
        }
    </script>
</head>
<body class="bg-gray-900 text-gray-100 font-sans min-h-screen">
    <header class="bg-gray-800 border-b border-gray-700 shadow-lg">
        <div class="max-w-7xl mx-auto px-4 py-6 sm:px-6 lg:px-8 flex flex-col sm:flex-row justify-between items-center gap-4">
            <div>
                <h1 class="text-3xl font-bold text-white tracking-tight flex items-center gap-3">
                    <svg class="w-8 h-8 text-blue-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11a7 7 0 01-7 7m0 0a7 7 0 01-7-7m7 7v4m0 0H8m4 0h4m-4-8a3 3 0 01-3-3V5a3 3 0 116 0v6a3 3 0 01-3 3z"></path></svg>
                    Rust Micro-Icecast
                </h1>
                <p class="mt-1 text-sm text-gray-400">High-Concurrency Async Streaming Server</p>
            </div>
            <div class="text-right flex gap-4">
                <div class="bg-gray-700 rounded-lg px-4 py-2 text-center">
                    <span class="block text-xs text-gray-400 uppercase tracking-wider">Global Listeners</span>
                    <span class="block text-xl font-bold text-blue-400">"#
    );

    let mut total_listeners = 0;
    let mut active_streams = 0;
    for entry in state.mounts.iter() {
        total_listeners += entry.value().listeners.load(Ordering::SeqCst);
        active_streams += 1;
    }

    body.push_str(&format!(r#"{}</span>
                </div>
                <div class="bg-gray-700 rounded-lg px-4 py-2 text-center">
                    <span class="block text-xs text-gray-400 uppercase tracking-wider">Active Streams</span>
                    <span class="block text-xl font-bold text-green-400">{}</span>
                </div>
            </div>
        </div>
    </header>
    <main class="max-w-7xl mx-auto px-4 py-8 sm:px-6 lg:px-8">
"#, total_listeners, active_streams));

    if active_streams == 0 {
        body.push_str(r#"
        <div class="rounded-xl border-2 border-dashed border-gray-700 p-12 text-center bg-gray-800/50 mt-8">
            <svg class="mx-auto h-12 w-12 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path></svg>
            <h3 class="mt-4 text-xl font-medium text-gray-200">No Active Broadcasts</h3>
            <p class="mt-2 text-gray-400">Waiting for a Stream Source to connect...</p>
        </div>
        "#);
    } else {
        body.push_str("<div class=\"grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6\">\n");
        for entry in state.mounts.iter() {
            let path_raw = entry.key();
            let mount = entry.value();
            let listeners = mount.listeners.load(Ordering::SeqCst);
            let mobile = mount.mobile_listeners.load(Ordering::SeqCst);
            let desktop = mount.desktop_listeners.load(Ordering::SeqCst);
            let uptime = mount.created_at.elapsed().as_secs();

            let path = escape_html(path_raw);
            let name = escape_html(mount.metadata.name.as_deref().unwrap_or("Unnamed Station"));
            let genre = escape_html(mount.metadata.genre.as_deref().unwrap_or("Unknown Genre"));
            let desc = escape_html(mount.metadata.description.as_deref().unwrap_or("No description provided."));
            let ctype = escape_html(&mount.metadata.content_type);
            let bitrate = escape_html(mount.metadata.bitrate.as_deref().unwrap_or("Unknown"));

            body.push_str(&format!(r#"
            <div class="bg-gray-800 rounded-xl shadow-lg border border-gray-700 overflow-hidden flex flex-col transition-transform hover:-translate-y-1 hover:shadow-xl">
                <div class="p-6 flex-grow">
                    <div class="flex justify-between items-start mb-4">
                        <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium bg-green-500/10 text-green-400 border border-green-500/20">
                            <span class="w-1.5 h-1.5 bg-green-500 rounded-full animate-pulse"></span>
                            LIVE
                        </span>
                        <span class="text-xs text-gray-500 font-mono">{}</span>
                    </div>
                    <h2 class="text-xl font-bold text-white mb-1 truncate" title="{}">{}</h2>
                    <span class="inline-block px-2 py-0.5 rounded bg-gray-700 text-xs text-gray-300 mb-3">{}</span>
                    <p class="text-sm text-gray-400 mb-6 line-clamp-2" title="{}">{}</p>
                    
                    <div class="grid grid-cols-2 gap-4 mb-6">
                        <div class="bg-gray-900 rounded p-3 text-center">
                            <span class="block text-xs text-gray-500 mb-1">Codec</span>
                            <span class="block text-sm font-medium text-gray-200">{}</span>
                        </div>
                        <div class="bg-gray-900 rounded p-3 text-center">
                            <span class="block text-xs text-gray-500 mb-1">Bitrate</span>
                            <span class="block text-sm font-medium text-gray-200">{}</span>
                        </div>
                        <div class="bg-gray-900 rounded p-3 text-center col-span-2">
                            <span class="block text-xs text-gray-500 mb-1">Stream Uptime</span>
                            <span class="block text-sm font-medium text-gray-200 font-mono">{}s</span>
                        </div>
                    </div>
                    
                    <div class="space-y-3">
                        <div class="flex justify-between items-center text-sm">
                            <span class="text-gray-400 flex items-center gap-2">
                                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 18h.01M8 21h8a2 2 0 002-2V5a2 2 0 00-2-2H8a2 2 0 00-2 2v14a2 2 0 002 2z"></path></svg>
                                Mobile
                            </span>
                            <span class="font-medium text-white bg-gray-700 px-2 py-0.5 rounded-full min-w-[2.5rem] text-center">{}</span>
                        </div>
                        <div class="flex justify-between items-center text-sm">
                            <span class="text-gray-400 flex items-center gap-2">
                                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"></path></svg>
                                Desktop
                            </span>
                            <span class="font-medium text-white bg-gray-700 px-2 py-0.5 rounded-full min-w-[2.5rem] text-center">{}</span>
                        </div>
                        <div class="flex justify-between items-center text-sm border-t border-gray-700 pt-3 mt-3">
                            <span class="text-gray-400 font-medium">Total Audience</span>
                            <span class="font-bold text-white text-base">{}</span>
                        </div>
                    </div>
                </div>
                <div class="p-4 bg-gray-800 border-t border-gray-700">
                    <div class="mb-3">
                        <label class="block text-xs text-gray-500 mb-1">Direct Stream URL</label>
                        <div class="flex">
                            <input type="text" readonly value="{}{}" class="w-full bg-gray-900 text-gray-300 text-xs rounded px-3 py-2 border border-gray-700 focus:outline-none" onclick="this.select()">
                        </div>
                    </div>
                    <a href="{}{}" target="_blank" class="block w-full text-center bg-blue-600 hover:bg-blue-500 text-white font-medium py-2 px-4 rounded-lg transition-colors flex justify-center items-center gap-2">
                        <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM9.555 7.168A1 1 0 008 8v4a1 1 0 001.555.832l3-2a1 1 0 000-1.664l-3-2z" clip-rule="evenodd"></path></svg>
                        Listen Now
                    </a>
                </div>
            </div>
            "#, 
            path, name, name, genre, desc, desc, ctype, bitrate, uptime, mobile, desktop, listeners, base_url, path, base_url, path));
        }
        body.push_str("</div>\n");
    }

    body.push_str(r#"
    </main>
</body>
</html>"#);

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    if let Err(e) = stream.write_all(response.as_bytes()).await {
        error!("Failed to write status page: {}", e);
    }
}
