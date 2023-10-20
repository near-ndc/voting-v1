use crate::{PropKind, Vote};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::AccountId;

#[derive(Deserialize, Serialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone))]
#[serde(crate = "near_sdk::serde")]
pub struct VotePayload {
    pub prop_id: u32,
    pub vote: Vote,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone))]
#[serde(crate = "near_sdk::serde")]
pub struct CreateProposalPayload {
    pub kind: PropKind,
    pub description: String,
}

pub type SBTs = Vec<(AccountId, Vec<u64>)>;
