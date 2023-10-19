use std::cmp::min;

use near_sdk::serde::Serialize;

use crate::*;

/// This is format of output via JSON for the proposal.
#[derive(Serialize)]
#[cfg_attr(test, derive(PartialEq))]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone))]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalOutput {
    /// Id of the proposal.
    pub id: u32,
    #[serde(flatten)]
    pub proposal: Proposal,
}

#[near_bindgen]
impl Contract {
    /**********
     * QUERIES
     **********/

    /// Returns all proposals
    /// Get proposals in paginated view.
    pub fn get_proposals(
        &self,
        from_index: u32,
        limit: u32,
        reverse: Option<bool>,
    ) -> Vec<ProposalOutput> {
        if reverse.unwrap_or(false) {
            let mut start = 1;
            let end_index = min(from_index, self.prop_counter);
            if end_index > limit {
                start = end_index - limit;
            }

            return (start..=end_index)
                .rev()
                .filter_map(|id| {
                    self.proposals
                        .get(&id)
                        .map(|proposal| ProposalOutput { id, proposal })
                })
                .collect();
        }

        (from_index..=min(self.prop_counter, from_index + limit))
            .filter_map(|id| {
                self.proposals.get(&id).map(|mut proposal| {
                    proposal.recompute_status(self.voting_duration);
                    ProposalOutput { id, proposal }
                })
            })
            .collect()
    }

    /// Get specific proposal.
    pub fn get_proposal(&self, id: u32) -> Option<ProposalOutput> {
        self.proposals.get(&id).map(|mut proposal| {
            proposal.recompute_status(self.voting_duration);
            ProposalOutput { id, proposal }
        })
    }

    pub fn number_of_proposals(&self) -> u32 {
        self.prop_counter
    }
}
