//! Test helper: a responder that completes the handshake and echoes every frame back.
//!
//! Usage: `echo_peer <group key as 64 hex chars>`. Prints `PORT <n>` on stdout once it is
//! listening, serves one connection, and exits when the peer closes it. Used by the Kotlin
//! live interop test (PLAN section 8).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use proto::group::GroupKey;
use proto::handshake::respond;
use proto::ids::decode_hex_exact;
use proto::msg::Message;
use proto::ProtoError;
use tokio::net::TcpListener;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let hex_key = std::env::args()
        .nth(1)
        .expect("usage: echo_peer <64 hex chars>");
    let gk = GroupKey(
        decode_hex_exact::<32>(&hex_key).expect("group key must be 64 lowercase hex chars"),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("PORT {}", listener.local_addr().unwrap().port());
    let (stream, _) = listener.accept().await.unwrap();
    stream.set_nodelay(true).unwrap();
    let mut conn = respond(stream, &gk).await.expect("handshake failed");
    loop {
        match conn.reader.read_frame().await {
            Ok(frame) => {
                if matches!(frame.message, Message::Unknown { .. }) {
                    continue;
                }
                conn.writer
                    .write_frame(&frame.message, &frame.payload)
                    .await
                    .unwrap();
            }
            Err(ProtoError::Closed) => break,
            Err(e) => {
                eprintln!("echo_peer: {e}");
                std::process::exit(1);
            }
        }
    }
}
