//! A tiny one-shot HTTP server that records the request it received and
//! replies with canned responses, so the login flow and the API client can be
//! exercised without touching the real platform.
//!
//! Not every integration test uses every helper here, so dead-code warnings
//! are silenced for this module.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread::JoinHandle;

/// A planned response: status line, extra headers, and body.
#[derive(Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Response {
    pub fn ok(body: &str) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: body.to_string(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
}

/// A server that answers one connection per planned response, in order.
pub struct MockServer {
    pub url: String,
    requests: mpsc::Receiver<String>,
    handle: JoinHandle<()>,
}

impl MockServer {
    /// Start a server answering each connection with the next planned response.
    pub fn start(responses: Vec<Response>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();

        let handle = std::thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let request = read_request(&mut stream);
                let _ = tx.send(request);

                let mut raw = format!(
                    "HTTP/1.1 {} {}\r\n",
                    response.status,
                    reason(response.status)
                );
                for (name, value) in &response.headers {
                    raw.push_str(&format!("{}: {}\r\n", name, value));
                }
                raw.push_str(&format!("content-length: {}\r\n", response.body.len()));
                raw.push_str("connection: close\r\n\r\n");
                raw.push_str(&response.body);
                let _ = stream.write_all(raw.as_bytes());
                let _ = stream.flush();
            }
        });

        Self {
            url,
            requests: rx,
            handle,
        }
    }

    /// Block until the next request arrives and return it verbatim.
    pub fn next_request(&self) -> String {
        self.requests
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("mock server received no request")
    }

    /// Stop the server thread.
    pub fn finish(self) {
        drop(self.requests);
        let _ = self.handle.join();
    }
}

/// Read one HTTP request in full: ureq may write the headers and the body in
/// separate packets, so keep reading until `content-length` bytes have arrived.
fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    while buf.len() < 1 << 20 {
        let read = stream.read(&mut chunk).unwrap_or(0);
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let content_length = content_length_of(&buf[..end]);
            if buf.len() >= end + 4 + content_length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn content_length_of(head: &[u8]) -> usize {
    let head = String::from_utf8_lossy(head).to_lowercase();
    head.lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

/// Responses for the two login calls: device registration and sign-in.
pub fn login_responses(token: &str, device_id: &str) -> Vec<Response> {
    vec![
        Response::ok(&format!(r#"{{"device":{{"deviceID":"{device_id}"}}}}"#)).with_header(
            "set-cookie",
            "Oasis-Token=anon.jwt.device; Path=/; HttpOnly",
        ),
        Response::ok("{}").with_header("oasis-token", token),
    ]
}

/// Response for the balance call that validates a freshly logged-in token.
pub fn balance_response(balance: &str) -> Response {
    Response::ok(&format!(r#"{{"balance":"{balance}","cost_total":"12.5"}}"#))
}
