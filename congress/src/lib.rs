use std::collections::HashMap;

//use events::{emit_bond, emit_revoke_vote, emit_vote};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap};
use near_sdk::json_types::U128;
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise, PromiseOrValue};

mod constants;
mod errors;
mod events;
pub mod proposal;
mod storage;
// mod ext;
// mod view;

pub use crate::constants::*;
pub use crate::errors::*;
pub use crate::proposal::*;
use crate::storage::*;
// pub use crate::ext::*;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub dissolved: bool,
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,

    /// Map of accounts authorized create proposals and vote for proposals
    // We can use single object rather than LookupMap because the maximum amount of members
    // is 17 (for HoM: 15 + 2)
    pub members: LazyOption<(Vec<AccountId>, Vec<PropPerms>)>,
    // minimum amount of members to approve the proposal
    pub threshold: u8,

    /// Map of accounts authorized to call hooks.
    pub hook_auth: LazyOption<HashMap<AccountId, Vec<HookPerms>>>,

    // all times below are in miliseconds
    pub start_time: u64,
    pub end_time: u64,
    pub cooldown: u64,
}

#[near_bindgen]
impl Contract {
    #[init]
    /// * hook_auth : map of accounts authorized to call hooks
    pub fn new(
        start_time: u64,
        end_time: u64,
        cooldown: u64,
        #[allow(unused_mut)] mut members: Vec<AccountId>,
        member_perms: Vec<PropPerms>,
        hook_auth: HashMap<AccountId, Vec<HookPerms>>,
    ) -> Self {
        require!(members.len() < 100);
        let threshold = (members.len() / 2) as u8 + 1;
        members.sort();
        Self {
            dissolved: false,
            prop_counter: 0,
            proposals: LookupMap::new(StorageKey::Proposals),
            members: LazyOption::new(StorageKey::Members, Some(&(members, member_perms))),
            threshold,
            hook_auth: LazyOption::new(StorageKey::HookAuth, Some(&hook_auth)),
            start_time,
            end_time,
            cooldown,
        }
    }

    /*
     * Queries are in view.rs
     */

    /**********
     * TRANSACTIONS
     **********/

    /// Creates a new empty proposal. `start` and `end`are timestamps in milliseconds.
    /// Returns the new proposal ID.
    /// NOTE: storage is paid from the account state
    #[payable]
    pub fn create_proposal(&mut self, typ: ProposalKind) -> u32 {
        let user = &env::predecessor_account_id();
        let (members, perms) = self.members.get().unwrap();
        require!(members.binary_search(user).is_ok(), "not a member");

        self.prop_counter
    }

    /*****************
     * INTERNAL
     ****************/
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests {
    use near_sdk::{
        test_utils::{self, VMContextBuilder},
        testing_env, Gas, VMContext,
    };
    use serde_json::Value;

    use crate::*;

    /// 1ms in nano seconds
    const MSECOND: u64 = 1_000_000;
    const START: u64 = 10;

    fn acc(idx: u8) -> AccountId {
        AccountId::new_unchecked(format!("user-{}.near", idx))
    }
}
