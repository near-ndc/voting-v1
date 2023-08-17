use near_sdk::{serde::Serialize, AccountId};
use serde_json::json;

use common::{EventPayload, NearEvent};

fn emit_event<T: Serialize>(event: EventPayload<T>) {
    NearEvent {
        standard: "ndc-elections",
        version: "1.0.0",
        event,
    }
    .emit();
}

pub(crate) fn emit_vote(prop_id: u32) {
    emit_event(EventPayload {
        event: "vote",
        data: json!({ "prop_id": prop_id }),
    });
}

#[cfg(test)]
mod unit_tests {
    use near_sdk::test_utils;

    use super::*;

    fn _acc(idx: u8) -> AccountId {
        AccountId::new_unchecked(format!("user-{}.near", idx))
    }

    #[test]
    fn log_vote() {
        let expected1 = r#"EVENT_JSON:{"standard":"ndc-elections","version":"1.0.0","event":"vote","data":{"prop_id":21}}"#;
        emit_vote(21);
        assert_eq!(vec![expected1], test_utils::get_logs());
    }
}
