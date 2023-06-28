use near_sdk::json_types::Base64VecU8;
use near_sdk::serde::Deserialize;
use near_sdk::{ext_contract, AccountId, PromiseOrValue};

// imports needed for conditional derive (required for tests)
#[allow(unused_imports)]
use near_sdk::serde::Serialize;

use crate::Vote;

#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_vote_verified(&mut self, prop_id: u32, user: AccountId, vote: Vote);
}

#[ext_contract(ext_sbtreg)]
pub trait ExtSbtRegistry {
    fn is_human(&self, account: AccountId) -> Vec<(AccountId, Vec<TokenId>)>;
    fn is_human_call(
        &mut self,
        account: AccountId,
        ctr: AccountId,
        function: String,
        args: Base64VecU8,
    ) -> PromiseOrValue<bool>;
}

// TODO: use SBT crate once it is published

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

pub type TokenId = u64;
