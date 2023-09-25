use std::cmp::min;

use near_sdk::serde::Serialize;

use crate::*;

/// This is format of output via JSON for the proposal.
#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalOutput {
    /// Id of the proposal.
    pub id: u32,
    #[serde(flatten)]
    pub proposal: Proposal,
}

#[near_bindgen]
impl Contract {
    /// Get proposals in paginated view.
    pub fn get_proposals(&self, from_index: u32, limit: u32) -> Vec<ProposalOutput> {
        (from_index..min(self.prop_counter, from_index + limit))
            .filter_map(|id| {
                self.proposals
                    .get(&id)
                    .map(|proposal| ProposalOutput { id, proposal })
            })
            .collect()
    }

    /// Get specific proposal.
    pub fn get_proposal(&self, id: u32) -> Option<ProposalOutput> {
        self.proposals
            .get(&id)
            .map(|proposal| ProposalOutput { id, proposal })
    }
}
