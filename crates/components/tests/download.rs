//! Pruebas de la descarga contra un servidor HTTP local que simula los fallos reales: cortes a
//! mitad, servidores que ignoran `Range`, archivos corruptos y errores del servidor.

use std::io::{self, Cursor, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use components::{ComponentError, Progress, RetryPolicy, fetch_with};
use sha2::{Digest, Sha256};
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    /// La primera respuesta se corta al 40 % (conexión caída); las siguientes son normales.
    DropFirst,
    /// Siempre responde 200 con el archivo entero, aunque se pida un rango.
    IgnoreRange,
    /// Siempre 503.
    Unavailable,
}

struct TestServer {
    url: String,
    /// Cabecera `Range` de cada petición recibida (`None` si no la traía).
    ranges: Arc<Mutex<Vec<Option<String>>>>,
}

/// Lee `data` hasta `limit` bytes y luego falla, como una conexión que se cae.
struct Dying {
    inner: Cursor<Vec<u8>>,
    limit: usize,
    served: usize,
}

impl Read for Dying {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.served >= self.limit {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "caída simulada",
            ));
        }
        let room = (self.limit - self.served).min(buf.len());
        let n = self.inner.read(&mut buf[..room])?;
        self.served += n;
        Ok(n)
    }
}

fn serve(data: Vec<u8>, mode: Mode) -> TestServer {
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&ranges);
    let served = AtomicUsize::new(0);
    thread::spawn(move || {
        for request in server.incoming_requests() {
            if *request.method() != Method::Get {
                continue;
            }
            let range = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Range"))
                .map(|h| h.value.to_string());
            seen.lock().unwrap().push(range.clone());
            let number = served.fetch_add(1, Ordering::SeqCst);

            if mode == Mode::Unavailable {
                let _ = request.respond(Response::empty(StatusCode(503)));
                continue;
            }
            let start = match (&range, mode) {
                (Some(r), m) if m != Mode::IgnoreRange => r
                    .strip_prefix("bytes=")
                    .and_then(|v| v.strip_suffix('-'))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0),
                _ => 0,
            };
            let body = data[start..].to_vec();
            let length = body.len();
            let (status, headers) = if start > 0 {
                let content_range = format!("bytes {start}-{}/{}", data.len() - 1, data.len());
                (
                    StatusCode(206),
                    vec![Header::from_bytes("Content-Range", content_range).unwrap()],
                )
            } else {
                (StatusCode(200), Vec::new())
            };
            if mode == Mode::DropFirst && number == 0 {
                let dying = Dying {
                    inner: Cursor::new(body),
                    limit: length * 4 / 10,
                    served: 0,
                };
                let _ = request.respond(Response::new(status, headers, dying, Some(length), None));
            } else {
                let _ = request.respond(Response::new(
                    status,
                    headers,
                    Cursor::new(body),
                    Some(length),
                    None,
                ));
            }
        }
    });
    TestServer {
        url: format!("http://127.0.0.1:{port}/archivo.bin"),
        ranges,
    }
}

fn data(len: usize) -> Vec<u8> {
    // Determinista y poco comprimible.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

const FAST: RetryPolicy = RetryPolicy {
    max_failures: 4,
    backoff_ms: 1,
};

fn run(
    server: &TestServer,
    part: &std::path::Path,
    bytes: &[u8],
    sha: &str,
) -> (Result<(), ComponentError>, Vec<Progress>) {
    let mut events = Vec::new();
    let result = fetch_with(&server.url, part, bytes.len() as u64, sha, FAST, &mut |p| {
        events.push(p)
    });
    (result, events)
}

#[test]
fn downloads_verifies_and_reports_progress() {
    let bytes = data(3 * 1024 * 1024);
    let server = serve(bytes.clone(), Mode::Normal);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.part");

    let (result, events) = run(&server, &part, &bytes, &sha256(&bytes));

    result.unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), bytes);
    let total = bytes.len() as u64;
    assert!(events.contains(&Progress::Downloading { done: total, total }));
    assert_eq!(events.last(), Some(&Progress::Verifying));
    let dones: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Progress::Downloading { done, .. } => Some(*done),
            _ => None,
        })
        .collect();
    assert!(
        dones.windows(2).all(|w| w[0] <= w[1]),
        "el avance no retrocede: {dones:?}"
    );
}

#[test]
fn a_dropped_connection_is_resumed_with_a_range_request() {
    let bytes = data(2 * 1024 * 1024);
    let server = serve(bytes.clone(), Mode::DropFirst);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.part");

    let (result, _) = run(&server, &part, &bytes, &sha256(&bytes));

    result.unwrap();
    assert_eq!(
        std::fs::read(&part).unwrap(),
        bytes,
        "el archivo final está completo y sin huecos"
    );
    let ranges = server.ranges.lock().unwrap();
    assert!(ranges.len() >= 2, "hubo reintento: {ranges:?}");
    assert_eq!(ranges[0], None);
    let second = ranges[1].as_deref().unwrap();
    assert!(
        second.starts_with("bytes=") && second != "bytes=0-",
        "reanuda donde quedó: {second}"
    );
}

#[test]
fn an_existing_partial_file_is_continued_not_restarted() {
    let bytes = data(1024 * 1024);
    let server = serve(bytes.clone(), Mode::Normal);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.part");
    std::fs::write(&part, &bytes[..300_000]).unwrap();

    let (result, _) = run(&server, &part, &bytes, &sha256(&bytes));

    result.unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), bytes);
    assert_eq!(
        server.ranges.lock().unwrap()[0].as_deref(),
        Some("bytes=300000-")
    );
}

#[test]
fn a_server_that_ignores_range_makes_the_download_restart_cleanly() {
    let bytes = data(1024 * 1024);
    let server = serve(bytes.clone(), Mode::IgnoreRange);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.part");
    std::fs::write(&part, &bytes[..100_000]).unwrap();

    let (result, _) = run(&server, &part, &bytes, &sha256(&bytes));

    result.unwrap();
    assert_eq!(
        std::fs::read(&part).unwrap(),
        bytes,
        "sin duplicar el principio"
    );
}

#[test]
fn an_oversized_partial_file_is_discarded() {
    let bytes = data(500_000);
    let server = serve(bytes.clone(), Mode::Normal);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.part");
    std::fs::write(&part, vec![7u8; 900_000]).unwrap();

    let (result, _) = run(&server, &part, &bytes, &sha256(&bytes));

    result.unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), bytes);
}

#[test]
fn a_corrupt_download_is_rejected_and_deleted() {
    let bytes = data(400_000);
    let server = serve(bytes.clone(), Mode::Normal);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.part");
    let wrong = sha256(b"otra cosa");

    let (result, _) = run(&server, &part, &bytes, &wrong);

    match result {
        Err(ComponentError::HashMismatch { expected, actual }) => {
            assert_eq!(expected, wrong);
            assert_eq!(actual, sha256(&bytes));
        }
        other => panic!("esperaba HashMismatch, salió {other:?}"),
    }
    assert!(!part.exists(), "el archivo dañado no se conserva");
}

#[test]
fn a_server_that_keeps_failing_gives_up_with_a_network_error() {
    let server = serve(data(10), Mode::Unavailable);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.part");

    let (result, _) = run(&server, &part, &data(10), &sha256(&data(10)));

    assert!(matches!(result, Err(ComponentError::Network(m)) if m.contains("503")));
    assert_eq!(
        server.ranges.lock().unwrap().len(),
        4,
        "reintentó exactamente max_failures veces"
    );
}
