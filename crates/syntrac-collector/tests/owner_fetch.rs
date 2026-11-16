//! The WinHTTP fetcher, against a tiny HTTP server on the loopback interface (no internet needed). The production URL is https and
//! is compiled in; the loopback URLs here are only for these tests.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use syntrac_collector::owner_fetch::{parse_url, FetchLimits, WinHttpFetcher, OWNER_DATA_URL};
use syntrac_core::DatasetFetcher;

/// Answers ONE request with exactly these bytes, then closes the connection. Returns the URL to ask.
fn serve_once(response: Vec<u8>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/data/ip2asn-combined.tsv.gz", listener.local_addr().unwrap().port());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut seen = Vec::new();
        let mut chunk = [0u8; 1024];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => seen.extend_from_slice(&chunk[..n]),
            }
        }
        let _ = stream.write_all(&response);
        let _ = stream.flush();
    });
    (url, handle)
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn ok_response(body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn small_limits() -> FetchLimits {
    FetchLimits { max_download_bytes: 64 * 1024, max_text_bytes: 256 * 1024, receive_timeout_ms: 5_000 }
}

const TSV: &str = "1.0.0.0\t1.0.0.255\t13335\tUS\tCLOUDFLARENET - Cloudflare, Inc.\n";

#[test]
fn a_gzip_file_is_downloaded_decompressed_and_identified() {
    let body = gzip(TSV.as_bytes());
    let (url, server) = serve_once(ok_response(&body));

    let file = WinHttpFetcher::new(&url, small_limits()).unwrap().fetch().unwrap();
    server.join().unwrap();

    assert_eq!(file.text, TSV);
    assert_eq!(file.source, "127.0.0.1");
    assert_eq!(
        file.sha256,
        Sha256::digest(&body).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "the hash is of the file as downloaded"
    );
}

#[test]
fn an_error_status_is_an_error_naming_the_status() {
    let (url, server) =
        serve_once(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec());

    let error = WinHttpFetcher::new(&url, small_limits()).unwrap().fetch().unwrap_err();
    server.join().unwrap();

    assert!(error.0.contains("404"), "{error}");
}

#[test]
fn a_redirect_is_refused_not_followed() {
    let (url, server) = serve_once(
        b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.2/elsewhere.gz\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
    );

    let error = WinHttpFetcher::new(&url, small_limits()).unwrap().fetch().unwrap_err();
    server.join().unwrap();

    assert!(error.0.contains("302") && error.0.contains("redirect"), "{error}");
}

#[test]
fn a_download_that_says_it_is_too_big_is_refused_before_it_is_read() {
    let mut response = b"HTTP/1.1 200 OK\r\nContent-Length: 999999999\r\nConnection: close\r\n\r\n".to_vec();
    response.extend_from_slice(&[0u8; 100]);
    let (url, server) = serve_once(response);

    let error = WinHttpFetcher::new(&url, small_limits()).unwrap().fetch().unwrap_err();
    server.join().unwrap();

    assert!(error.0.contains("larger than"), "{error}");
}

#[test]
fn a_download_that_turns_out_bigger_than_the_cap_is_stopped() {
    // No Content-Length: the size is only known by reading.
    let mut response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    response.extend_from_slice(&vec![7u8; 200 * 1024]);
    let (url, server) = serve_once(response);

    let error = WinHttpFetcher::new(&url, small_limits()).unwrap().fetch().unwrap_err();
    let _ = server.join();

    assert!(error.0.contains("larger than"), "{error}");
}

#[test]
fn a_file_that_inflates_beyond_the_cap_is_refused() {
    let bomb = gzip(&vec![b'a'; 4 * 1024 * 1024]);
    assert!(bomb.len() < 64 * 1024, "the bomb itself is small: {}", bomb.len());
    let (url, server) = serve_once(ok_response(&bomb));

    let error = WinHttpFetcher::new(&url, small_limits()).unwrap().fetch().unwrap_err();
    server.join().unwrap();

    assert!(error.0.contains("unpack") || error.0.contains("larger than"), "{error}");
}

#[test]
fn a_truncated_or_plain_file_is_not_accepted_as_the_dataset() {
    let whole = gzip(TSV.repeat(200).as_bytes());
    let truncated = &whole[..whole.len() / 2];
    for body in [truncated.to_vec(), TSV.as_bytes().to_vec()] {
        let (url, server) = serve_once(ok_response(&body));

        let error = WinHttpFetcher::new(&url, small_limits()).unwrap().fetch().unwrap_err();
        server.join().unwrap();

        assert!(error.0.to_lowercase().contains("gzip") || error.0.contains("unpack"), "{error}");
    }
}

#[test]
fn nothing_listening_is_an_error_not_a_hang() {
    // A port that was just free; nobody answers.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();

    let error =
        WinHttpFetcher::new(&format!("http://127.0.0.1:{port}/x"), small_limits()).unwrap().fetch().unwrap_err();

    assert!(!error.0.is_empty());
}

#[test]
fn only_plain_http_and_https_urls_with_a_host_and_no_login_are_accepted() {
    for bad in ["ftp://example.com/x", "file:///c:/x", "https://", "https://user:pw@example.com/x", "example.com/x", ""]
    {
        assert!(parse_url(bad).is_err(), "{bad} should be refused");
    }
    let parsed = parse_url("https://iptoasn.com/data/ip2asn-combined.tsv.gz").unwrap();
    assert_eq!(
        (parsed.secure, parsed.host.as_str(), parsed.port, parsed.path.as_str()),
        (true, "iptoasn.com", 443, "/data/ip2asn-combined.tsv.gz")
    );
    let parsed = parse_url("http://127.0.0.1:8080").unwrap();
    assert_eq!((parsed.secure, parsed.port, parsed.path.as_str()), (false, 8080, "/"));
}

#[test]
fn the_compiled_in_address_is_the_one_the_plan_approved() {
    assert_eq!(OWNER_DATA_URL, "https://iptoasn.com/data/ip2asn-combined.tsv.gz");
    assert!(parse_url(OWNER_DATA_URL).unwrap().secure);
}
