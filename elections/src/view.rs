use near_sdk::near_bindgen;

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
}
