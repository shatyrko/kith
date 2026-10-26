//! Test helpers shared by the google modules.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// `(request line, headers, body)` of every request the mock received.
pub type RequestLog = Arc<Mutex<Vec<(String, String, String)>>>;

/// A one-thread HTTP server on 127.0.0.1 that answers the n-th request with the n-th canned `(status, extra headers, body)`.
/// Returns its base URL (`http://127.0.0.1:port/v1`) and the log of received requests.
pub fn mock(responses: Vec<(u16, &'static str, String)>) -> (String, RequestLog) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let log: RequestLog = Arc::new(Mutex::new(vec![]));
    let log2 = log.clone();
    std::thread::spawn(move || {
        for (status, headers, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut head = String::new();
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
                head.push_str(&h);
            }
            let mut b = vec![0u8; len];
            reader.read_exact(&mut b).unwrap();
            log2.lock().unwrap().push((line.trim().to_string(), head, String::from_utf8_lossy(&b).to_string()));
            let resp = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len());
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });
    (format!("http://{addr}/v1"), log)
}
