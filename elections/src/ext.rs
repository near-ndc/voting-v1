use near_sdk::serde::Deserialize;
use near_sdk::{ext_contract, AccountId, Promise};
use near_sdk::json_types::U128;

// imports needed for conditional derive (required for tests)
#[allow(unused_imports)]
use near_sdk::serde::Serialize;

use crate::{Vote, VoteError};

#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_vote_verified(&mut self, prop_id: u32, voter: AccountId, vote: Vote) -> Result<(), VoteError>;
    fn on_community_verified(&mut self,
        sender: AccountId,
        policy: String,
        deposit_amount: U128) -> Promise;
    fn on_community_verified_bond(&mut self,
        sender: AccountId,
        deposit_amount: U128) -> Promise;
    fn on_community_verified_unbond(&mut self,
        sender: AccountId,
    ) -> Promise;
}

#[ext_contract(ext_sbtreg)]
pub trait ExtSbtRegistry {
    fn is_human(&self, account: AccountId) -> HumanSBTs;
    fn is_community_verified(&self, account: AccountId, is_human: bool) -> (HumanSBTs, HumanSBTs);
}

// TODO: use SBT crate once it is published

pub type TokenId = u64;
pub type HumanSBTs = Vec<(AccountId, Vec<TokenId>)>;

/// token data for sbt_tokens_by_owner response
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct OwnedToken {
    pub token: u64,
    pub metadata: TokenMetadata,
}

/// TokenMetadata defines attributes for each SBT token.

#[derive(Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Serialize, Debug))]
#[serde(crate = "near_sdk::serde")]
pub struct TokenMetadata {
    pub class: u64,
    pub issued_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}
