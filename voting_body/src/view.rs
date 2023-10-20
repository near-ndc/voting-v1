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

/// This is format of output via JSON for the config.
#[derive(Serialize)]
#[cfg_attr(test, derive(PartialEq, Debug))]
#[serde(crate = "near_sdk::serde")]
pub struct ConfigOutput {
    pub prop_counter: u32,
    pub pre_vote_bond: U128,
    pub active_queue_bond: U128,
    pub pre_vote_support: u32,
    pub simple_consent: Consent,
    pub super_consent: Consent,
    pub voting_duration: u64,
    pub pre_vote_duration: u64,
    pub accounts: Accounts,
}

#[near_bindgen]
impl Contract {
    /**********
     * QUERIES
     **********/

    /// Returns all proposals from the active queue, which were not marked as a spam. This
    /// includes proposals that are in progress, rejected, approved or failed.
    /// TODO: simplify this https://github.com/near-ndc/voting-v1/pull/102#discussion_r1365810686
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
                start = end_index - limit + 1;
            }

            return (start..=end_index)
                .rev()
                .filter_map(|id| {
                    self.proposals.get(&id).map(|mut proposal| {
                        proposal.recompute_status(self.voting_duration);
                        ProposalOutput { id, proposal }
                    })
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
        let mut p = self.proposals.get(&id);
        if p.is_none() {
            p = self.pre_vote_proposals.get(&id);
        }
        p.map(|mut proposal| {
            proposal.recompute_status(self.voting_duration);
            ProposalOutput { id, proposal }
        })
    }

    pub fn number_of_proposals(&self) -> u32 {
        self.prop_counter
    }

    pub fn config(&self) -> ConfigOutput {
        ConfigOutput {
            prop_counter: self.prop_counter,
            pre_vote_bond: U128(self.pre_vote_bond),
            active_queue_bond: U128(self.active_queue_bond),
            pre_vote_support: self.pre_vote_support,
            simple_consent: self.simple_consent,
            super_consent: self.super_consent,
            pre_vote_duration: self.pre_vote_duration,
            voting_duration: self.voting_duration,
            accounts: self.accounts.get().unwrap(),
        }
    }
}
