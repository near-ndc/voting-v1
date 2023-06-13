use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::BorshStorageKey;

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    Nominations,
    Upvotes,
    Admins,
    UpvotesPerCandidate,
    Comments,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Campaign {
    pub name: String,
    pub link: String,
    /// start and end time for the nominations
    pub start_time: u64,
    pub end_time: u64,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum HouseType {
    Other,
    HouseOfMerit,
    CouncilOfAdvisors,
    TransparencyCommission,
}
