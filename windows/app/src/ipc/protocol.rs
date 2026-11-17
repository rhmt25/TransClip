//! Wire format of the daemon pipe (PLAN section 5.6): one JSON object per line.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A line longer than this is a protocol violation and closes the connection.
pub const MAX_LINE: usize = 1024 * 1024;

/// Client to daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Chosen by the client, echoed in the response.
    pub id: u64,
    /// Method name such as `get_status`.
    pub method: String,
    /// Arguments (`null` when none).
    #[serde(default)]
    pub params: Value,
}

/// Daemon to client, answer to a request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Id of the request.
    pub id: u64,
    /// Result on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error text on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Daemon to client, unsolicited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Event name such as `status_changed`.
    pub event: String,
    /// Payload.
    #[serde(default)]
    pub data: Value,
}

/// Anything the daemon can send.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    /// Answer to a request.
    Response(Response),
    /// Pushed event.
    Event(Event),
}

/// Methods the daemon understands (PLAN section 5.6).
pub const METHODS: [&str; 14] = [
    "get_status",
    "get_config",
    "set_config",
    "list_peers",
    "forget_peer",
    "set_bt_priority",
    "create_group",
    "get_pairing_uri",
    "join_group",
    "rotate_key",
    "set_paused",
    "open_log_dir",
    "show_settings",
    "quit",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_round_trip_and_default_params() {
        let r: Request = serde_json::from_str(r#"{"id":7,"method":"get_status"}"#).unwrap();
        assert_eq!(r.params, Value::Null);
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), r);
    }

    #[test]
    fn incoming_distinguishes_response_from_event() {
        let ok: Incoming = serde_json::from_str(r#"{"id":1,"result":{"a":1}}"#).unwrap();
        assert!(matches!(ok, Incoming::Response(Response { id: 1, .. })));
        let err: Incoming = serde_json::from_str(r#"{"id":2,"error":"nope"}"#).unwrap();
        assert!(matches!(
            err,
            Incoming::Response(Response { error: Some(_), .. })
        ));
        let ev: Incoming = serde_json::from_str(
            &json!({"event":"status_changed","data":{"online":2}}).to_string(),
        )
        .unwrap();
        assert!(matches!(ev, Incoming::Event(Event { .. })));
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(serde_json::from_str::<Incoming>("hello").is_err());
        assert!(serde_json::from_str::<Request>(r#"{"method":"x"}"#).is_err());
    }
}
