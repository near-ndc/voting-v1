use near_sdk::serde::Deserialize;
use near_sdk::{ext_contract, AccountId};

// imports needed for conditional derive (required for tests)
#[allow(unused_imports)]
use near_sdk::serde::Serialize;

use crate::storage::AccountFlag;
use crate::{RevokeVoteError, Vote, VoteError};

#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_vote_verified(
        &mut self,
        prop_id: u32,
        voter: AccountId,
        vote: Vote,
    ) -> Result<(), VoteError>;
    fn on_revoke_verified(&mut self, prop_id: u32, user: AccountId) -> Result<(), RevokeVoteError>;
}

#[ext_contract(ext_sbtreg)]
pub trait ExtSbtRegistry {
    fn is_human(&self, account: AccountId) -> HumanSBTs;
    fn account_flagged(&self, account: AccountId) -> Option<AccountFlag>;

    fn sbt_mint(&mut self, token_spec: Vec<(AccountId, Vec<TokenMetadata>)>) -> Vec<TokenId>;
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

#[derive(Deserialize, Serialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
#[serde(crate = "near_sdk::serde")]
pub struct TokenMetadata {
    pub class: u64,
    pub issued_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}
