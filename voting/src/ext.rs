use near_sdk::{ext_contract, AccountId};

use crate::Vote;

#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_vote_verified(&mut self, prop_id: u32, user: AccountId, vote: Vote);
}
