use crate::Vote;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::AccountId;

#[derive(Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone))]
#[serde(crate = "near_sdk::serde")]
pub struct VotePayload {
    pub id: u32,
    pub vote: Vote,
}

pub type TokenId = u64;

pub type SBTs = Vec<(AccountId, Vec<TokenId>)>;
