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
pub struct CreatePropPayload {
    pub kind: PropKind,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone))]
#[serde(crate = "near_sdk::serde")]
pub struct SupportPropPayload {
    pub prop_id: u32,
}

pub type SBTs = Vec<(AccountId, Vec<u64>)>;

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone, PartialEq))]
pub enum ExecResponse {
    Slashed,
    Rejected,
    Executed,
}
