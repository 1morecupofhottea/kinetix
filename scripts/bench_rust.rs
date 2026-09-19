//! Dependency-free capacity/overhead rig for Kinetix (NFR-1.4/NFR-1.8).
//!
//! The Python harness (scripts/bench.sh) is fine for correctness but is
//! GIL-bound: a Python client + Python synthetic upstream saturate on one
//! machine well before Kinetix does, so it cannot demonstrate the NFR-1.4
//! capacity gate (200 concurrent streams / 50 rps). This rig is std-only Rust,
//! so both the synthetic upstream and the load generator scale.
//!
//! Build:  rustc -O scripts/bench_rust.rs -o /tmp/bench_rust
//! Upstream: /tmp/bench_rust upstream 9099 [tokens] [delay_us]
//! Load:     /tmp/bench_rust load <url> <concurrency> <total_requests> [tokens]
//!
//! The upstream speaks OpenAI-compatible SSE (data: frames + [DONE]) and closes
//! the connection to delimit the body.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("upstream") => {
            let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(9099);
            let tokens: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(40);
            let delay_us: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
            upstream(port, tokens, delay_us);
        }
        Some("load") => {
            let url = args.get(2).cloned().unwrap_or_default();
            let concurrency: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
            let total: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(100);
            let tokens: usize = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(40);
            load(&url, concurrency, total, tokens);
        }
        _ => {
            eprintln!("usage: bench_rust upstream <port> [tokens] [delay_us]");
            eprintln!("       bench_rust load <url> <concurrency> <total> [tokens]");
            std::process::exit(2);
        }
    }
}

// ---------------------------------------------------------------------------
// Synthetic upstream: thread-per-connection OpenAI SSE server.
// ---------------------------------------------------------------------------
fn upstream(port: u16, tokens: usize, delay_us: u64) {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    eprintln!("synthetic upstream on 127.0.0.1:{port} (tokens={tokens}, delay_us={delay_us})");
    for stream in listener.incoming() {
        if let Ok(s) = stream {
            let tokens = tokens;
            std::thread::spawn(move || {
                let _ = handle_upstream(s, tokens, delay_us);
            });
        }
    }
}

fn handle_upstream(mut s: TcpStream, tokens: usize, delay_us: u64) -> std::io::Result<()> {
    s.set_nodelay(true).ok();
    // Read headers.
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = s.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
    }
    // Read body by content-length.
    let head = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
    let clen: usize = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    while buf.len() < header_end + clen {
        let n = s.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }

    let _ = s.write_all(
        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
    );
    s.flush().ok();

    let mut out = Vec::with_capacity(256 * tokens);
    let role = r#"data: {"id":"syn-1","object":"chat.completion.chunk","model":"syn","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#;
    out.extend_from_slice(role.as_bytes());
    out.extend_from_slice(b"\n\n");
    let delta = r#"data: {"id":"syn-1","object":"chat.completion.chunk","model":"syn","choices":[{"index":0,"delta":{"content":"lorem"},"finish_reason":null}]}"#;
    for _ in 0..tokens {
        out.extend_from_slice(delta.as_bytes());
        out.extend_from_slice(b"\n\n");
        if delay_us > 0 {
            let _ = s.write_all(&out);
            out.clear();
            s.flush().ok();
            std::thread::sleep(Duration::from_micros(delay_us));
        }
    }
    out.extend_from_slice(
        b"data: {\"id\":\"syn-1\",\"object\":\"chat.completion.chunk\",\"model\":\"syn\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    );
    out.extend_from_slice(b"data: [DONE]\n\n");
    let _ = s.write_all(&out);
    let _ = s.flush();
    Ok(())
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// Load generator: N threads, each issuing streaming requests until the shared
// budget is exhausted. Records TTFT and total latency per request.
// ---------------------------------------------------------------------------
struct Stats {
    ttft_us: Vec<u64>,
    total_us: Vec<u64>,
    errors: usize,
}

fn load(url: &str, concurrency: usize, total: usize, tokens: usize) {
    // Parse http://host:port/path
    let rest = url.strip_prefix("http://").unwrap_or(url);
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };

    let issued = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    let start = Instant::now();
    for _ in 0..concurrency {
        let hostport = hostport.to_string();
        let path = path.to_string();
        let issued = issued.clone();
        let done = done.clone();
        handles.push(std::thread::spawn(move || {
            let mut st = Stats {
                ttft_us: Vec::new(),
                total_us: Vec::new(),
                errors: 0,
            };
            loop {
                let n = issued.fetch_add(1, Ordering::Relaxed);
                if n >= total {
                    break;
                }
                match one_request(&hostport, &path, tokens) {
                    Ok((ttft, total)) => {
                        st.ttft_us.push(ttft);
                        st.total_us.push(total);
                    }
                    Err(_) => st.errors += 1,
                }
                done.fetch_add(1, Ordering::Relaxed);
            }
            st
        }));
    }

    let mut all = Stats {
        ttft_us: Vec::new(),
        total_us: Vec::new(),
        errors: 0,
    };
    for h in handles {
        let st = h.join().unwrap();
        all.ttft_us.extend(st.ttft_us);
        all.total_us.extend(st.total_us);
        all.errors += st.errors;
    }
    let elapsed = start.elapsed();

    all.ttft_us.sort_unstable();
    all.total_us.sort_unstable();
    let n = all.total_us.len();
    let rps = if elapsed.as_secs_f64() > 0.0 {
        n as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };
    println!(
        "{{\"concurrency\":{concurrency},\"requests\":{n},\"errors\":{},\"elapsed_ms\":{},\"rps\":{:.1},\"ttft_p50_ms\":{:.2},\"ttft_p95_ms\":{:.2},\"ttft_p99_ms\":{:.2},\"total_p50_ms\":{:.2},\"total_p95_ms\":{:.2},\"total_p99_ms\":{:.2}}}",
        all.errors,
        elapsed.as_millis(),
        rps,
        pct(&all.ttft_us, 0.50),
        pct(&all.ttft_us, 0.95),
        pct(&all.ttft_us, 0.99),
        pct(&all.total_us, 0.50),
        pct(&all.total_us, 0.95),
        pct(&all.total_us, 0.99),
    );
    let _ = done;
}

fn pct(sorted_us: &[u64], p: f64) -> f64 {
    if sorted_us.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_us.len() as f64 * p) as usize).min(sorted_us.len() - 1);
    sorted_us[idx] as f64 / 1000.0
}

fn one_request(hostport: &str, path: &str, tokens: usize) -> std::io::Result<(u64, u64)> {
    let body = format!(
        "{{\"model\":\"syn\",\"stream\":true,\"max_tokens\":{},\"messages\":[{{\"role\":\"user\",\"content\":\"hi\"}}]}}",
        tokens
    );
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {hostport}\r\ncontent-type: application/json\r\naccept: text/event-stream\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let started = Instant::now();
    let mut s = TcpStream::connect(hostport)?;
    s.set_nodelay(true).ok();
    s.set_read_timeout(Some(Duration::from_secs(60))).ok();
    s.write_all(req.as_bytes())?;
    s.flush().ok();

    let mut tmp = [0u8; 16384];
    let mut ttft: Option<u64> = None;
    loop {
        let n = s.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        if ttft.is_none() {
            ttft = Some(started.elapsed().as_micros() as u64);
        }
    }
    let total = started.elapsed().as_micros() as u64;
    Ok((ttft.unwrap_or(total), total))
}
