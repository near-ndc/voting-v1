use near_sdk::{near_bindgen, AccountId};

use crate::{proposal::*};
use crate::{Contract, ContractExt};

#[near_bindgen]
impl Contract {
    pub(crate) fn _proposal(&self, prop_id: u32) -> Proposal {
        self.proposals.get(&prop_id).expect("proposal not found")
    }

    /**********
     * QUERIES
     **********/

    /// Returns all proposals
    pub fn proposals(&self) -> Vec<Proposal> {
        let mut proposals = Vec::with_capacity(self.prop_counter as usize);
        for i in 1..=self.prop_counter {
            proposals.push(self.proposals.get(&i).unwrap());
        }
        proposals
    }

    /// Returns given proposal
    pub fn proposal(&self, prop_id: u32) -> Proposal {
        self._proposal(prop_id)
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
