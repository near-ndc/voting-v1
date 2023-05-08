use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::UnorderedMap;
// use near_sdk::json_types::U64;
// use near_sdk::serde::{Deserialize, Serialize};
// use near_sdk::CryptoHash;
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};

mod constants;
mod ext;
mod proposal;
mod storage;
mod view;
pub use crate::constants::*;
pub use crate::ext::*;
pub use crate::proposal::*;
use crate::storage::*;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub pause: bool,
    /// supermajority quorum
    pub sup_consent: Consent,
    pub consent: Consent,
    pub proposals: UnorderedMap<u32, Proposal>,
    prop_counter: u32,
    /// proposal voting duration in seconds
    pub prop_duration: u32,
    /// start_margin is a minimum duration in seconds before a proposal is submitted
    /// and proposal voting start.
    pub start_margin: u32,
    /// address which can pause the contract and make proposal.
    /// Should be a multisig / DAO;
    pub gwg: AccountId,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        gwg: AccountId,
        sup_consent: Consent,
        consent: Consent,
        prop_duration: u32,
        start_margin: u32,
    ) -> Self {
        Self {
            pause: false,
            gwg,
            sup_consent,
            consent,
            prop_duration,
            start_margin,
            proposals: UnorderedMap::new(StorageKey::Proposals),
            prop_counter: 0,
        }
    }

    /// creates new empty proposal
    /// returns proposal ID
    pub fn creat_proposal(&mut self, start: u64) -> u32 {
        // TODO: check permissions
        // - add fun parameters (description ...)
        let min_start = self.start_margin as u64 + env::block_timestamp() / SECOND;
        require!(
            start >= min_start,
            format!("proposal start after {} unix time", min_start)
        );
        self.prop_counter += 1;
        self.proposals.insert(
            &self.prop_counter,
            &Proposal::new(start, start + self.prop_duration as u64),
        );
        self.prop_counter
    }

    /// aggregated vote for a binary proposal
    #[payable]
    pub fn vote(&mut self, prop_id: u32, vote: Vote) -> Promise {
        let p = self._proposal(prop_id);
        p.assert_active();
        let user = env::predecessor_account_id();
        if !p.voted.contains_key(&user) {
            require!(
                env::attached_deposit() >= VOTE_COST,
                format!(
                    "requires {}yocto deposit for storage fees for every new vote",
                    VOTE_COST
                )
            );
        }
        require!(
            env::prepaid_gas() >= GAS_VOTE,
            format!("not enough gas, min: {:?}", GAS_VOTE)
        );

        // TODO: call staking contract and i-am-human
        ext_self::ext(env::current_account_id())
            .with_static_gas(GAS_VOTE_CALLBACK)
            .on_vote_verified(prop_id, user, vote)
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    pub fn on_vote_verified(&mut self, prop_id: u32, user: AccountId, vote: Vote) {
        let mut p = self._proposal(prop_id);
        p.vote_on_verified(&user, vote);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
