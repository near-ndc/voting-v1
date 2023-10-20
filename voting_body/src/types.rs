use crate::Vote;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::AccountId;

#[derive(Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone, Serialize))]
#[serde(crate = "near_sdk::serde")]
pub struct VotePayload {
    pub prop_id: u32,
    pub vote: Vote,
}


pub type SBTs = Vec<(AccountId, Vec<u64>)>;
