use near_sdk::serde::Serialize;
use serde_json::json;

use crate::{proposal::PropKind, ExecError};

use common::{EventPayload, NearEvent};

fn emit_event<T: Serialize>(event: EventPayload<T>) {
    NearEvent {
        standard: "ndc-congress",
        version: "1.0.0",
        event,
    }
    .emit();
}

/// * `active`: set to true if the prosal was added to an active queue directly.
pub(crate) fn emit_prop_created(prop_id: u32, kind: &PropKind, active: bool) {
    emit_event(EventPayload {
        event: "new-proposal",
        data: json!({ "prop_id": prop_id, "kind": kind.to_name(),  "active": active}),
    });
}

pub(crate) fn emit_prop_active(prop_id: u32) {
    emit_event(EventPayload {
        event: "proposal-active",
        data: json!({ "prop_id": prop_id}),
    });
}

pub(crate) fn emit_vote(prop_id: u32) {
    emit_event(EventPayload {
        event: "vote",
        data: json!({ "prop_id": prop_id }),
    });
}

pub(crate) fn emit_vote_execute(prop_id: u32, err: ExecError) {
    emit_event(EventPayload {
        event: "vote-execute",
        data: json!({ "prop_id": prop_id, "status": "failed", "reason": err }),
    });
}

pub(crate) fn emit_executed(prop_id: u32) {
    emit_event(EventPayload {
        event: "execute",
        data: json!({ "prop_id": prop_id }),
    });
}

/// spam event is emitted when a proposal is marked as spam, removed and bond is slashed.
pub(crate) fn emit_spam(prop_id: u32) {
    emit_event(EventPayload {
        event: "spam",
        data: json!({ "prop_id": prop_id }),
    });
}
