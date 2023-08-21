use near_sdk::{env, near_bindgen, AccountId};
use uint::hex;

use crate::proposal::*;
use crate::{Contract, ContractExt};

#[near_bindgen]
impl Contract {
    pub(crate) fn _proposal(&self, prop_id: u32) -> Proposal {
        self.proposals.get(&prop_id).expect("proposal not found")
    }

    /**********
     * QUERIES
     **********/

    pub fn proposals(&self) -> Vec<ProposalView> {
        let mut proposals = Vec::with_capacity(self.prop_counter as usize);
        for i in 1..=self.prop_counter {
            proposals.push(self.proposals.get(&i).unwrap().to_view(i));
        }
        proposals
    }

    pub fn proposal(&self, prop_id: u32) -> ProposalView {
        self._proposal(prop_id).to_view(prop_id)
    }

    /// Returns the proposal status
    pub fn proposal_status(&self, prop_id: u32) -> Option<ProposalStatus> {
        let now = env::block_timestamp_ms();
        return self.proposals.get(&prop_id).map(|p| p.status(now));
    }

    /// Returns the policy if user has accepted it otherwise returns None
    pub fn accepted_policy(&self, user: AccountId) -> Option<String> {
        self.accepted_policy
            .get(&user)
            .map(|policy| hex::encode(policy))
    }

    pub fn policy(&self) -> String {
        hex::encode(self.policy)
    }
}
