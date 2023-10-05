use near_sdk::{serde::Serialize, AccountId};
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

pub(crate) fn emit_prop_created(prop_id: u32, kind: &PropKind) {
    emit_event(EventPayload {
        event: "new-proposal",
        data: json!({ "prop_id": prop_id, "kind": kind.to_name() }),
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

/// spam event is emitted when a proopsal is marked as spam, removed and bond is slashed.
pub(crate) fn emit_spam(prop_id: u32) {
    emit_event(EventPayload {
        event: "spam",
        data: json!({ "prop_id": prop_id }),
    });
}
