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

    /// Returns all the users votes for all the proposals. If user has not voted yet on any proposal empty vector will be returned.
    /// NOTE: the response may not be consistent with the registry. If user will do a soul_transfer, then technically votes should be associated
    /// with other user. Here we return votes from the original account that voted for the given user.
    pub fn user_votes(&self, user: AccountId) -> Vec<(u32, Option<Vec<usize>>)> {
        let mut to_return = Vec::new();

        for p in 0..=self.prop_counter {
            if let Some(proposal) = self.proposals.get(&p) {
                if let Some(user_vote_key) = proposal.user_sbt.get(&user) {
                    let user_vote = proposal.voters.get(&user_vote_key);
                    to_return.push((p, user_vote));
                } else {
                    to_return.push((p, None));
                }
            }
        }
        to_return
    }

    /// Returns the required policy
    pub fn policy(&self) -> String {
        hex::encode(self.policy)
    }
}
