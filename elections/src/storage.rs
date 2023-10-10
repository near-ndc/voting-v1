use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::Base64VecU8;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::BorshStorageKey;

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    Proposals,
    ProposalVoters(u32),
    AcceptedPolicy,
    BondedAmount,
    UserSBT(u32),
    DisqualifiedCandidates,
}

#[derive(PartialEq, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Clone))]
pub enum AccountFlag {
    /// Account is "blacklisted" when it was marked as a scam or breaking the IAH rules.
    Blacklisted,
    Verified,
}

/// ClassMetadata describes an issuer class.
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone)]
#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
#[serde(crate = "near_sdk::serde")]
pub struct ClassMetadata {
    /// Issuer class name. Required.
    pub name: String,
    /// If defined, should be used instead of `contract_metadata.symbol`.
    pub symbol: Option<String>,
    /// Icon content (SVG) or a link to an Icon. If it doesn't start with a scheme (eg: https://)
    /// then `contract_metadata.base_uri` should be prepended.
    pub icon: Option<String>,
    /// JSON or an URL to a JSON file with more info. If it doesn't start with a scheme
    /// (eg: https://) then base_uri should be prepended.
    pub reference: Option<String>,
    /// Base64-encoded sha256 hash of JSON from reference field. Required if `reference` is included.
    pub reference_hash: Option<Base64VecU8>,
}
