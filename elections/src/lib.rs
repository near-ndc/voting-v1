use std::collections::HashSet;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
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
            proposals: LookupMap::new(StorageKey::Proposals),
            prop_counter: 0,
        }
    }

    /// creates new empty proposal
    /// returns the new proposal ID
    pub fn creat_proposal(
        &mut self,
        typ: HouseType,
        start: u64,
        end: u64,
        ref_link: String,
        credits: u16,
        #[allow(unused_mut)] mut candidates: Vec<AccountId>,
    ) -> u32 {
        self.assert_admin();
        let min_start = env::block_timestamp() / SECOND;
        require!(min_start < start, "proposal start must be in the future");
        require!(start < end, "proposal start must be before end");
        require!(
            6 <= ref_link.len() && ref_link.len() <= 120,
            "ref_link length must be between 6 and 120 bytesx"
        );
        let cs: HashSet<&AccountId> = HashSet::from_iter(candidates.iter());
        require!(cs.len() == candidates.len(), "duplicated candidates");
        candidates.sort();

        self.prop_counter += 1;
        let l = candidates.len();
        let p = Proposal {
            typ,
            start,
            end,
            ref_link,
            credits,
            candidates,
            result: vec![0; l],
            voters: LookupSet::new(StorageKey::ProposalVoters(self.prop_counter)),
        };

        self.proposals.insert(&self.prop_counter, &p);
        self.prop_counter
    }

    /// election vote using quadratic mechanism
    #[payable]
    pub fn vote(&mut self, prop_id: u32, vote: Vote) -> Promise {
        let p = self._proposal(prop_id);
        p.assert_active();
        let user = env::predecessor_account_id();
        require!(!p.voters.contains(&user), "caller already voted",);
        require!(
            env::attached_deposit() >= VOTE_COST,
            format!(
                "requires {}yocto deposit for storage fees for every new vote",
                VOTE_COST
            )
        );
        require!(
            env::prepaid_gas() >= GAS_VOTE,
            format!("not enough gas, min: {:?}", GAS_VOTE)
        );
        validate_vote(&vote, p.credits, &p.candidates);

        // TODO
        // call SBT registry to verify  SBT
        // ext_sbtreg::ext(self.sbt_registry.clone())
        //     .sbt_tokens_by_owner(
        //         user.clone(),
        //         Some(self.iah_issuer.clone()),
        //         Some(self.iah_class_id.clone()),
        //         Some(1),
        //     )
        //     .then(
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

    /*****************
     * INTERNAL
     ****************/

    #[inline]
    fn assert_admin(&self) {
        require!(
            self.authority == env::predecessor_account_id(),
            "not an admin"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
