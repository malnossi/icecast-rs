# Rust Micro-Icecast Server 📻⚡

![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)
![Runtime: Tokio](https://img.shields.io/badge/Runtime-Tokio-blue.svg)
![Scale: 10K+ Concurrent](https://img.shields.io/badge/Scale-10K%2B_Concurrent-brightgreen.svg)
![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)

An ultra-lightweight, zero-allocation alternative to legacy Icecast streaming servers. Built from the ground up in modern Rust, this project is relentlessly optimized for digital sovereignty, massive concurrency, and O(1) performance scale. 

Whether you are hosting a global digital radio station, an enterprise ambient audio feed, or an independent podcast stream, Rust Micro-Icecast delivers your audio instantaneously and flawlessly to tens of thousands of concurrent listeners on a fraction of the hardware footprint.

---

## 🏛 Core Architectural Pillars

Rust Micro-Icecast achieves unparalleled throughput through three strict engineering paradigms:

1. **Asynchronous I/O (The C10K Solution)**
   Powered by the `Tokio` runtime, the server utilizes OS-native event loops (`epoll` on Linux, `kqueue` on macOS) to handle tens of thousands of simultaneous TCP connections using asynchronous, non-blocking green-threads.
2. **Zero-Allocation Fanout ($O(1)$ Payload)**
   A classic streaming bottleneck is memory allocation per listener. We eliminate this entirely by utilizing `bytes::Bytes` structures. Incoming audio chunks are loaded into memory exactly once. Every connecting listener simply receives a shallow-copy atomic pointer reference to the original chunk, keeping the memory footprint flat regardless of whether you have 10 or 10,000 listeners.
3. **Total Fault Isolation**
   Listener tasks operate in strictly isolated, lock-free tokio tasks. If a mobile listener encounters high latency, drops packets, or suffers an ungraceful disconnect, it cannot trigger a deadlock or interrupt the global broadcast channel. The source ingest and neighboring listeners remain utterly unaffected.

---

## 📊 Key Features Matrix

| Feature | Description |
| :--- | :--- |
| **Burst-on-Connect Cache** | Maintains a 128 KB (131,072 bytes) sliding history window. New listeners are instantly fed this burst buffer for true zero-latency, gapless stream initialization. |
| **Thread-Safe Multi-Mountpoint** | Safely hosts and isolates multiple parallel broadcast streams on dynamic paths (e.g., `/live`, `/ambient`) without cross-talk. |
| **Source Authentication** | Strict ingestion boundary parsing `SOURCE` and `PUT` HTTP methods against secure Basic Authentication parameters. |
| **Tailwind Web Dashboard** | A beautiful, native HTML dashboard served at `GET /` visualizing realtime telemetry, mountpoint status, codecs, and bitrates. |
| **Mobile vs Desktop Analytics** | Dynamically parses listener `User-Agent` strings. Lock-free `ListenerGuard` trackers provide realtime atomic device analytics on the dashboard. |
| **Strict ICY Compliance** | Natively extracts, caches, and reflects classic Shoutcast/Icecast metadata headers (`ice-name`, `ice-bitrate`, `Content-Type`) perfectly to downstream media players. |

---

## ⚙️ Production Configuration

The server configuration is loaded natively from a minimal `config.toml` file in the root directory.

```toml
# config.toml

[server]
host = "0.0.0.0"
port = 8000
burst_size = 131072 # 128KB gapless history cache

[source]
username = "source"
password = "super_secure_password"
```

---

## 🚀 Quick Start & Execution Guide

This project utilizes `just` (a modern `make` alternative) to orchestrate local development and testing.

Ensure you have Rust installed via `rustup`, then use the following `justfile` recipes:

```bash
# Build the highly optimized release binary
just build

# Run the server locally
just run

# Launch a mock broadcaster source (requires ffmpeg or ezstream)
just source

# Execute the local memory and connection stress-testing suite
just stress-test
```

---

## 🔧 Kernel Tuning & Systems Engineering

To truly unleash the asynchronous power of this server and scale past 10,000+ concurrent connections, you must instruct your Operating System Kernel to raise its default File Descriptor (Socket) limitations.

By default, UNIX systems cap open files/sockets to `1024` per process. Before launching the server in a production environment, execute the following explicit `ulimit` override:

```bash
# Raise the file descriptor boundary to allow 50,000 concurrent listeners
ulimit -n 50000

# Launch the optimized release binary
./target/release/icecast-rs
```

Failure to configure the system file descriptors will result in the kernel prematurely terminating incoming TCP handshakes under heavy listener load.

---

## License

This project is open-source and dual-licensed under both the MIT License and the Apache License (Version 2.0). See the [LICENSE](LICENSE) file for details.
