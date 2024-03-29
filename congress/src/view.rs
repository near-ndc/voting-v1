use std::cmp::{max, min};

use itertools::Either;
#[allow(unused_imports)]
use near_sdk::serde::{Deserialize, Serialize};

use crate::*;

/// This is format of output via JSON for the proposal.
#[derive(Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[cfg_attr(any(test, not(target_arch = "wasm32")), derive(Deserialize))]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalOutput {
    /// Id of the proposal.
    pub id: u32,
    #[serde(flatten)]
    pub proposal: Proposal,
}

/// This is format of output via JSON for the config.
#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ConfigOutput {
    pub threshold: u8,
    pub start_time: u64,
    pub end_time: u64,
    pub cooldown: u64,
    pub vote_duration: u64,
    pub budget_spent: U128,
    pub budget_cap: U128,
    pub big_funding_threshold: U128,
    pub min_vote_duration: u64,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[cfg_attr(any(test, not(target_arch = "wasm32")), derive(Deserialize))]
#[serde(crate = "near_sdk::serde")]
pub struct MembersOutput {
    /// Id of the proposal.
    pub members: Vec<AccountId>,
    pub permissions: Vec<PropPerm>,
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
        let iter = if reverse.unwrap_or(false) {
            let end = if from_index == 0 {
                self.prop_counter
            } else {
                min(from_index, self.prop_counter)
            };
            let start = if end <= limit { 1 } else { end - (limit - 1) };
            Either::Left((start..=end).rev())
        } else {
            let from_index = max(from_index, 1);
            Either::Right(from_index..=min(self.prop_counter, from_index + limit - 1))
        };

        let (members, _) = self.members.get().unwrap();
        let ml = members.len();

        iter.filter_map(|id| {
            self.proposals.get(&id).map(|mut proposal| {
                proposal.finalize_status(
                    ml,
                    self.threshold,
                    self.min_vote_duration,
                    self.vote_duration,
                );
                ProposalOutput { id, proposal }
            })
        })
        .collect()
    }

    /// Get specific proposal.
    pub fn get_proposal(&self, id: u32) -> Option<ProposalOutput> {
        let (members, _) = self.members.get().unwrap();
        let ml = members.len();
        self.proposals.get(&id).map(|mut proposal| {
            proposal.finalize_status(
                ml,
                self.threshold,
                self.min_vote_duration,
                self.vote_duration,
            );
            ProposalOutput { id, proposal }
        })
    }

    pub fn number_of_proposals(&self) -> u32 {
        self.prop_counter
    }

    pub fn is_dissolved(&self) -> bool {
        self.dissolved
    }

    pub fn members_len(&self) -> u8 {
        self.members_len
    }

    /// Returns all members with permissions
    pub fn get_members(&self) -> MembersOutput {
        let (members, permissions) = self.members.get().unwrap();
        MembersOutput {
            members,
            permissions,
        }
    }

    /// Returns all members with permissions
    pub fn is_member(&self, account: AccountId) -> bool {
        let (members, _) = self.members.get().unwrap();
        members.contains(&account)
    }

    /// Returns permissions of a given member.
    /// Returns empty vector (`[]`) if not a member.
    pub fn member_permissions(&self, member: AccountId) -> Vec<PropPerm> {
        let (members, perms) = self.members.get().unwrap();
        if members.binary_search(&member).is_ok() {
            return perms;
        }
        vec![]
    }

    /// Returns hook permissions for given account
    /// Returns empty vector `[]` if not a hook.
    pub fn hook_permissions(&self, user: AccountId) -> Vec<HookPerm> {
        let mut hooks = self.hook_auth.get().unwrap();
        hooks.remove(&user).unwrap_or(vec![])
    }

    /// Returns all hook permissions
    pub fn all_hook_permissions(&self) -> HashMap<AccountId, Vec<HookPerm>> {
        self.hook_auth.get().unwrap()
    }

    pub fn config(&self) -> ConfigOutput {
        ConfigOutput {
            threshold: self.threshold,
            start_time: self.start_time,
            end_time: self.end_time,
            cooldown: self.cooldown,
            vote_duration: self.vote_duration,
            budget_spent: U128(self.budget_spent),
            budget_cap: U128(self.budget_cap),
            big_funding_threshold: U128(self.big_funding_threshold),
            min_vote_duration: self.min_vote_duration,
        }
    }
}
