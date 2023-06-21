use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::BorshStorageKey;

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    Nominations,
    Upvotes,
    Admins,
}

/// nomination struct
#[derive(BorshDeserialize, BorshSerialize)]
pub struct Nomination {
    pub house: HouseType,
    /// timestamp in ms
    pub timestamp: u64,
    /// sum of received upvotes
    pub upvotes: u32,
}

/// house type struct
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, PartialEq)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(test, derive(Debug))]
pub enum HouseType {
    HouseOfMerit,
    CouncilOfAdvisors,
    TransparencyCommission,
}
