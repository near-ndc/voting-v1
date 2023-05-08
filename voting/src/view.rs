use near_sdk::near_bindgen;

use crate::proposal::*;
use crate::{Contract, ContractExt};

#[near_bindgen]
impl Contract {
    pub(crate) fn _proposal(&self, prop_id: u32) -> Proposal {
        self.proposals.get(&prop_id).expect("proposal not found")
    }

    pub fn get_proposal(&self, prop_id: u32) -> ProposalView {
        self._proposal(prop_id).into()
    }
}
