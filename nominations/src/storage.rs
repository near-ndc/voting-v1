use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId, BorshStorageKey};

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    Nominations,
    Upvotes,
    Campaigns,
    Admins,
    NumUpvotes,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(crate = "near_sdk::serde")]
pub struct NominationKey {
    pub campaign: u32,
    pub nominee: AccountId,
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

impl Campaign {
    pub fn assert_active(self) {
        let current_timestamp = env::block_timestamp() / crate::constants::SECOND;
        require!(
            current_timestamp <= self.end_time && current_timestamp >= self.start_time,
            format!(
                "Campaign not active. start_time: {}, end_time: {}",
                self.start_time, self.end_time
            )
        );
    }
}
