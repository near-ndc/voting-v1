use near_sdk::env;
use near_sdk::serde::Serialize;

/// Helper struct to create Standard NEAR Event JSON.
/// Arguments:
/// * `standard`: name of standard e.g. nep171
/// * `version`: e.g. 1.0.0
/// * `event`: associate event data
#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct NearEvent<T: Serialize> {
    pub standard: &'static str,
    pub version: &'static str,

    // `flatten` to not have "event": {<EventVariant>} in the JSON, just have the contents of {<EventVariant>}.
    #[serde(flatten)]
    pub event: T,
}

impl<T: Serialize> NearEvent<T> {
    pub fn to_json_event_string(&self) -> String {
        let s = serde_json::to_string(&self)
            .ok()
            .unwrap_or_else(|| env::abort());
        format!("EVENT_JSON:{}", s)
    }

    pub fn emit(self) {
        env::log_str(&self.to_json_event_string());
    }
}

/// Helper struct to be used in `NearEvent.event` to construct NEAR Event compatible payload
#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct EventPayload<T: Serialize> {
    /// event name
    pub event: &'static str,
    /// event payload
    pub data: T,
}

impl<T: Serialize> EventPayload<T> {
    pub fn emit(self, standard: &'static str, version: &'static str) {
        NearEvent {
            standard,
            version,
            event: self,
        }
        .emit()
    }
}

#[cfg(test)]
mod tests {
    use near_sdk::{test_utils, AccountId};

    use super::*;

    fn alice() -> AccountId {
        AccountId::new_unchecked("alice.near".to_string())
    }

    #[test]
    fn emit_event_payload() {
        let expected = r#"EVENT_JSON:{"standard":"nepXYZ","version":"1.0.1","event":"mint","data":["alice.near",[821,10,44]]}"#;
        let tokens = vec![821, 10, 44];
        let event = EventPayload {
            event: "mint",
            data: (alice(), tokens),
        };
        event.emit("nepXYZ", "1.0.1");
        assert_eq!(vec![expected], test_utils::get_logs());
    }
}
