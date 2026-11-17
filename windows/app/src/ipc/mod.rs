//! Local control channel between the daemon and other TransClip processes.

pub mod client;
pub mod protocol;
pub mod server;

/// Pipe name for the current user (and data-dir override, if any).
pub fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
    let user: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let mut name = format!("\\\\.\\pipe\\transclip-ipc-{user}");
    if let Some(dir) = std::env::var_os(crate::paths::DATA_DIR_ENV) {
        let mut h: u32 = 0x811c_9dc5;
        for b in dir.to_string_lossy().bytes() {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
        name.push_str(&format!("-{h:08x}"));
    }
    name
}

#[cfg(test)]
mod tests {
    use super::client::{one_shot, IpcClient};
    use super::protocol::Event;
    use super::server::{Handler, IpcServer};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use std::time::Duration;

    fn unique(tag: &str) -> String {
        format!("\\\\.\\pipe\\transclip-test-{tag}-{}", std::process::id())
    }

    fn handler() -> Handler {
        Arc::new(|method, params| {
            Box::pin(async move {
                match method.as_str() {
                    "echo" => Ok(params),
                    "slow" => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        Ok(json!("slow"))
                    }
                    other => Err(format!("unknown method: {other}")),
                }
            })
        })
    }

    #[tokio::test]
    async fn request_response_and_errors() {
        let name = unique("rr");
        let _server = IpcServer::start(&name, handler()).unwrap();
        let (client, _ev) = IpcClient::connect(&name).await.unwrap();
        assert_eq!(
            client.call("echo", json!({"a": 1})).await.unwrap(),
            json!({"a": 1})
        );
        assert_eq!(
            client.call("nope", Value::Null).await.unwrap_err(),
            "unknown method: nope"
        );
        // Concurrent calls are matched to their own answers.
        let (a, b) = tokio::join!(
            client.call("slow", Value::Null),
            client.call("echo", json!(2))
        );
        assert_eq!(a.unwrap(), json!("slow"));
        assert_eq!(b.unwrap(), json!(2));
    }

    #[tokio::test]
    async fn events_reach_connected_clients() {
        let name = unique("ev");
        let server = IpcServer::start(&name, handler()).unwrap();
        let (client, mut events) = IpcClient::connect(&name).await.unwrap();
        // Make sure the server side has registered the subscription.
        client.call("echo", Value::Null).await.unwrap();
        server.publish(Event {
            event: "status_changed".into(),
            data: json!({"online": 3}),
        });
        let ev = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ev.event, "status_changed");
        assert_eq!(ev.data["online"], 3);
    }

    #[tokio::test]
    async fn second_server_on_the_same_name_is_refused() {
        let name = unique("dup");
        let _first = IpcServer::start(&name, handler()).unwrap();
        assert!(IpcServer::start(&name, handler()).is_err());
    }

    #[tokio::test]
    async fn one_shot_and_many_sequential_clients() {
        let name = unique("one");
        let _server = IpcServer::start(&name, handler()).unwrap();
        for i in 0..5 {
            let v = one_shot(&name, "echo", json!(i)).await.unwrap();
            assert_eq!(v, json!(i));
        }
    }

    #[tokio::test]
    async fn oversized_line_closes_the_connection() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = unique("big");
        let _server = IpcServer::start(&name, handler()).unwrap();
        let mut raw = ClientOptions::new().open(&name).unwrap();
        let junk = vec![b'x'; super::protocol::MAX_LINE + 10];
        let _ = raw.write_all(&junk).await;
        // The server must still serve other clients afterwards.
        assert_eq!(one_shot(&name, "echo", json!(1)).await.unwrap(), json!(1));
    }

    #[test]
    fn pipe_name_is_per_user() {
        assert!(super::pipe_name().starts_with("\\\\.\\pipe\\transclip-ipc-"));
    }
}
