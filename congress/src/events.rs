use near_sdk::{serde::Serialize, Balance};
use serde_json::json;

use common::{EventPayload, NearEvent};

fn emit_event<T: Serialize>(event: EventPayload<T>) {
    NearEvent {
        standard: "ndc-congress",
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

pub(crate) fn emit_veto(prop_id: u32) {
    emit_event(EventPayload {
        event: "veto",
        data: json!({ "prop_id": prop_id }),
    });
}
