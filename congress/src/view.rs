use std::cmp::min;

use near_sdk::serde::Serialize;

use crate::*;

/// This is format of output via JSON for the proposal.
#[derive(Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalOutput {
    /// Id of the proposal.
    pub id: u32,
    #[serde(flatten)]
    pub proposal: Proposal,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq))]
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
        self.proposals.get(&id).map(|mut proposal| {
            proposal.recompute_status(self.voting_duration);
            ProposalOutput { id, proposal }
        })
    }

    pub fn number_of_proposals(&self) -> u32 {
        self.prop_counter
    }

    pub fn is_dissolved(&self) -> bool {
        self.dissolved
    }

    /// Returns all members with permissions
    pub fn get_members(&self) -> MembersOutput {
        let (members, permissions) = self.members.get().unwrap();
        MembersOutput {
            members,
            permissions,
        }
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
        let hooks = self.hook_auth.get().unwrap();
        let res = hooks.get(&user).cloned();
        if res.is_none() {
            return vec![];
        }
        res.unwrap()
    }
}
