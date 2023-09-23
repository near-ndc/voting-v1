use std::cmp::min;

use near_sdk::{near_bindgen, AccountId};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

use crate::{proposal::*};
use crate::{Contract, ContractExt};

/// This is format of output via JSON for the proposal.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalOutput {
    /// Id of the proposal.
    pub id: u32,
    #[serde(flatten)]
    pub proposal: Proposal,
}


#[near_bindgen]
impl Contract {
    pub(crate) fn _proposal(&self, prop_id: u32) -> Proposal {
        self.proposals.get(&prop_id).expect("proposal not found")
    }

    /**********
     * QUERIES
     **********/

    /// Returns all proposals
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

    /// Returns the proposal status
    pub fn proposal_status(&self, prop_id: u32) -> Option<ProposalStatus> {
        self.proposals.get(&prop_id).map(|p| p.status)
    }

    pub fn dissolve_status(&self) -> bool {
        self.dissolved
    }

    /// Returns permissions of any member
    /// None if not a member
    pub fn member_permissions(&self, member: AccountId) -> Option<Vec<PropPerm>> {
        let (members, perms) = self.members.get().unwrap();
        if members.binary_search(&member).is_ok() {
            return Some(perms);
        }
        None
    }

    /// Returns hook permissions for given account
    /// None if doesn't exist in hook
    pub fn hook_permissions(&self, user: AccountId) -> Option<Vec<HookPerm>> {
        let hooks = self.hook_auth.get().unwrap();
        hooks.get(&user).cloned()
    }
}