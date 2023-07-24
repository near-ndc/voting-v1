use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Consent {
    /// min number of accounts voting
    pub quorum: u64,
    /// #yes votes threshold as percent value (eg 12 = 12%)
    pub threshold: u8,
}
