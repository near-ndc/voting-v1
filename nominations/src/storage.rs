use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{env, require, AccountId, BorshStorageKey};

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    Nominations,
    NominationsPerUser,
    Campaigns,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct NominationKey {
    pub campaign: u32,
    pub nominator: AccountId,
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

impl Campaign {
    pub fn assert_active(self) {
        let current_timestamp = env::block_timestamp();
        require!(
            current_timestamp <= self.end_time && current_timestamp >= self.start_time,
            format!(
                "Campaign not active. start_time: {}, end_time: {}",
                self.start_time, self.end_time
            )
        );
    }
}
