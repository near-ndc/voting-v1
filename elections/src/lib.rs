use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::UnorderedMap;
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};

mod constants;
mod proposal;
mod storage;
mod view;
// mod ext;

pub use crate::constants::*;
pub use crate::proposal::*;
use crate::storage::*;
//pub use crate::ext::*;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub pause: bool,
    prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,

    /// address which can pause the contract and make proposal.
    /// Should be a multisig / DAO;
    pub authority: AccountId,
    pub sbt_registry: AccountId,
    /// issuer account for proof of humanity
    pub iah_issuer: AccountId,
    /// SBT class ID used for Facetech verification
    pub iah_class_id: u64,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        authority: AccountId,
        sbt_registry: AccountId,
        iah_issuer: AccountId,
        iah_class_id: u64,
    ) -> Self {
        Self {
            pause: false,
            authority,
            sbt_registry,
            iah_issuer,
            iah_class_id,
            proposals: UnorderedMap::new(StorageKey::Proposals),
            prop_counter: 0,
        }
    }

    /// creates new empty proposal
    /// returns proposal ID
    pub fn creat_proposal(
        &mut self,
        typ: PropType,
        start: u64,
        title: String,
        ref_link: String,
        ref_hash: String,
    ) -> u32 {
        // TODO: discuss other options to allow other parties to submit a proposal

        let min_start = self.start_margin as u64 + env::block_timestamp() / SECOND;
        require!(
            start >= min_start,
            format!("proposal start after {} unix time", min_start)
        );
        self.prop_counter += 1;
        self.proposals.insert(
            &self.prop_counter,
            &Proposal::new(
                typ,
                self.prop_counter,
                start,
                start + self.prop_duration as u64,
                title,
                ref_link,
                ref_hash,
            ),
        );
        self.prop_counter
    }

    /// aggregated vote for a binary proposal
    #[payable]
    pub fn elect(&mut self, prop_id: u32, vote: Vote) -> Promise {
        let p = self._proposal(prop_id);
        p.assert_active();
        let user = env::predecessor_account_id();
        if !p.votes.contains_key(&user) {
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

        // call SBT registry to verify G$ SBT
        ext_sbtreg::ext(self.sbt_registry.clone())
            .sbt_tokens_by_owner(
                user.clone(),
                Some(self.sbt_gd_issuer.clone()),
                Some(self.sbt_gd_class_id.clone()),
                Some(1),
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_VOTE_CALLBACK)
                    .on_vote_verified(prop_id, user, vote),
            )
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    pub fn on_vote_verified(&mut self, prop_id: u32, user: AccountId, vote: Vote) {
        let mut p = self._proposal(prop_id);
        p.vote_on_verified(&user, vote);
    }

    /*****************
     * PRIVATE
     ****************/

    fn assert_admin(&self) {
        // TODO
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
