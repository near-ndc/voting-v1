use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{ext_contract, AccountId};

/// sbt_tokens_by owner interface for cross-contract calls
#[ext_contract(ext_sbtreg)]
pub trait ExtSbtRegistry {
    fn sbt_tokens_by_owner(
        &self,
        account: AccountId,
        issuer: Option<AccountId>,
        from_class: Option<u64>,
        limit: Option<u32>,
        with_expired: Option<bool>,
    ) -> Vec<(AccountId, Vec<OwnedToken>)>;

    fn is_human(&self, account: AccountId) -> bool;
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
#[derive(Deserialize, Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct TokenMetadata {
    pub class: u64,
    pub issued_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}
