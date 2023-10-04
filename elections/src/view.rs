use near_sdk::{env, near_bindgen, AccountId, Balance};
use uint::hex;

use crate::{proposal::*, TokenId};
use crate::{Contract, ContractExt};

#[near_bindgen]
impl Contract {
    pub(crate) fn _proposal(&self, prop_id: u32) -> Proposal {
        self.proposals.get(&prop_id).expect("proposal not found")
    }

    /**********
     * QUERIES
     **********/

    pub fn finish_time(&self) -> u64 {
        self.finish_time
    }

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
        self.proposals
            .get(&prop_id)
            .map(|p| p.status(now, self.finish_time))
    }

    /// Returns the policy if user has accepted it otherwise returns None
    pub fn accepted_policy(&self, user: AccountId) -> Option<String> {
        self.accepted_policy.get(&user).map(hex::encode)
    }

    /// Returns all the users votes for all the proposals. If user has not voted yet a vector with None values will be returned.
    /// Eg. if we have 3 porposals and user only voted on first one then the return value will look like [Some([1,2]), None, None]
    /// NOTE: the response may not be consistent with the registry. If user will do a soul_transfer, then technically votes should be associated
    /// with other user. Here we return votes from the original account that voted for the given user.
    pub fn user_votes(&self, user: AccountId) -> Vec<Option<Vec<usize>>> {
        let mut to_return = Vec::new();

        for p in 1..=self.prop_counter {
            if let Some(proposal) = self.proposals.get(&p) {
                if let Some(user_vote_key) = proposal.user_sbt.get(&user) {
                    let user_vote = proposal.voters.get(&user_vote_key);
                    to_return.push(user_vote);
                } else {
                    to_return.push(None);
                }
            }
        }
        to_return
    }

    /// Returns bond amount by SBT TokenID.
    pub fn bond_by_sbt(&self, sbt: TokenId) -> Balance {
        self.bonded_amounts.get(&sbt).unwrap_or(0)
    }

    /// Returns true if user has voted on all proposals, otherwise false.
    pub fn has_voted_on_all_proposals(&self, user: AccountId) -> bool {
        self.user_votes(user).iter().all(|vote| vote.is_some())
    }

    /// Returns the required policy
    pub fn policy(&self) -> String {
        hex::encode(self.policy)
    }

    /// Returns a list of winners of the proposal if the elections is over and the quorum has been reached, otherwise returns empty list.
    /// A candidate is considered the winner only if he reached the `min_candidate_support`
    /// and is not listed as disqualified.
    /// If the number of returned winners is smaller than the number of seats it means some of the candidates
    /// did not reach the required minimum support.
    /// If there is a tie break at the tail and it exceeds the number of seats, the accounts
    /// in tie at the tail are not considered winners.
    pub fn winners_by_proposal(&self, prop_id: u32) -> Vec<AccountId> {
        let proposal = self._proposal(prop_id);

        if !(proposal.is_past_cooldown()
            && env::block_timestamp_ms() > self.finish_time
            && proposal.voters_num >= proposal.quorum)
        {
            return Vec::new();
        }

        let disqualified_candidates_indices = self.disqualifed_candidates_indices(prop_id);

        // Filter and sort the candidates in one step
        let mut indexed_results: Vec<(usize, u64)> = proposal
            .result
            .iter()
            .enumerate()
            .filter(|(idx, _)| !disqualified_candidates_indices.contains(idx))
            .map(|(idx, &votes)| (idx, votes))
            .collect();

        indexed_results.sort_by_key(|&(_, value)| std::cmp::Reverse(value));

        let mut winners = Vec::new();
        let last_out_idx = proposal.seats as usize;
        let last_out_votes = indexed_results
            .get(last_out_idx)
            .map(|&(_, votes)| votes)
            .unwrap_or(indexed_results[0].1 + 1);

        for (idx, votes) in indexed_results.into_iter().take(last_out_idx) {
            // Filter out tie in the tail if it could exceed the seats
            if proposal.min_candidate_support <= votes && last_out_votes < votes {
                let candidate = proposal.candidates.get(idx).unwrap();
                winners.push(candidate.clone());
            }
        }

        winners
    }

    /// Returns the list of disqualified candidates
    pub fn disqualified_candidates(&self) -> Vec<AccountId> {
        let mut res = Vec::new();
        for c in self.disqualified_candidates.iter() {
            res.push(c.clone());
        }
        res
    }
}
