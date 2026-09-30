//! The message envelope exchanged between a hosted web page and its Rust host.
//!
//! One shape covers everything: the page sends a `Request` (via `POST /__msg`),
//! the host answers with a `Response`, and the host may push an `Event` at any
//! time. The page-side API (`window.<namespace>.call(...)` / `.on(...)`) hides
//! the transport, so the same page works whether it is rendered by WebView2 or
//! by a borrowed browser.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Request,
    Response,
    Event,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Envelope {
    pub id: u64,
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub data: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_with_lowercase_kind() {
        let env = Envelope {
            id: 7,
            kind: Kind::Request,
            name: "hello".into(),
            data: json!({ "a": 1 }),
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(s.contains("\"kind\":\"request\""), "got: {s}");
        let back: Envelope = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, 7);
        assert_eq!(back.kind, Kind::Request);
        assert_eq!(back.data["a"], 1);
    }

    #[test]
    fn data_defaults_to_null() {
        let env: Envelope =
            serde_json::from_str(r#"{"id":1,"kind":"event","name":"x"}"#).unwrap();
        assert_eq!(env.kind, Kind::Event);
        assert!(env.data.is_null());
    }
}
