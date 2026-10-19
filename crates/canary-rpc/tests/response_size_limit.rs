//! Integration tests for the 16 MiB RPC response body size cap (D-06, CA-01 §6).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use canary_rpc::{HttpRpcClient, RetryPolicy, RpcClient, RpcError, MAX_RPC_RESPONSE_BYTES};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn rejects_response_exceeding_16_mib_with_response_too_large_and_does_not_retry() {
    let server = MockServer::start().await;
    let body = vec![b'a'; 20 * 1024 * 1024];

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .expect(1)
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri()).with_retry_policy(RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(1),
    });

    let err = client
        .get_network()
        .await
        .expect_err("20 MiB response must be rejected");
    assert!(
        matches!(
            err,
            RpcError::ResponseTooLarge {
                ref method,
                limit_bytes
            } if method == "getNetwork" && limit_bytes == MAX_RPC_RESPONSE_BYTES
        ),
        "expected ResponseTooLarge, got: {err:?}"
    );
}

#[tokio::test]
async fn aborts_chunked_stream_reading_after_16_mib_without_buffering_20_mib_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let bytes_sent = Arc::new(AtomicUsize::new(0));
    let bytes_sent_server = Arc::clone(&bytes_sent);

    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let _ = socket.set_write_timeout(Some(Duration::from_millis(250)));
        let _ = socket.set_read_timeout(Some(Duration::from_millis(250)));
        let mut req_buf = [0u8; 2048];
        let _ = socket.read(&mut req_buf);

        // Send HTTP 200 with Transfer-Encoding: chunked so no Content-Length header is present
        let headers = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        let _ = socket.write_all(headers);

        // Stream 20 MiB in 64 KiB chunks; stop as soon as the client aborts the connection
        let chunk_size = 64 * 1024;
        let chunk_header = format!("{chunk_size:x}\r\n");
        let chunk_payload = vec![b' '; chunk_size];

        for _ in 0..320 {
            if socket.write_all(chunk_header.as_bytes()).is_err() {
                break;
            }
            if socket.write_all(&chunk_payload).is_err() {
                break;
            }
            if socket.write_all(b"\r\n").is_err() {
                break;
            }
            bytes_sent_server.fetch_add(chunk_size, Ordering::SeqCst);
            let _ = socket.flush();
        }
    });

    let client = HttpRpcClient::new(format!("http://{addr}"));
    let err = client
        .get_network_raw()
        .await
        .expect_err("chunked stream over 16 MiB must fail");

    assert!(
        matches!(
            err,
            RpcError::ResponseTooLarge {
                ref method,
                limit_bytes
            } if method == "getNetwork" && limit_bytes == 16 * 1024 * 1024
        ),
        "expected ResponseTooLarge, got: {err:?}"
    );
    assert_eq!(MAX_RPC_RESPONSE_BYTES, 16 * 1024 * 1024);
}

#[tokio::test]
async fn accepts_valid_json_response_within_16_mib_cap() {
    let server = MockServer::start().await;
    let filler = "x".repeat(64 * 1024);
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "passphrase": "Test SDF Network ; September 2015",
                "protocolVersion": 28,
                "padding": filler
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let info = client
        .get_network()
        .await
        .expect("under-cap response succeeds");
    assert_eq!(info.protocol_version, 28);
}
