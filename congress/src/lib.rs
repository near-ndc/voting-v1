use std::collections::HashMap;

use events::*;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap};
use near_sdk::json_types::U128;
use near_sdk::{env, near_bindgen, require, AccountId, Balance, PanicOnDefault};

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
    pub members: LazyOption<(Vec<AccountId>, Vec<PropPerm>)>,
    // minimum amount of members to approve the proposal
    pub threshold: u8,

    /// Map of accounts authorized to call hooks.
    pub hook_auth: LazyOption<HashMap<AccountId, Vec<HookPerm>>>,

    // all times below are in miliseconds
    pub start_time: u64,
    pub end_time: u64,
    pub cooldown: u64,

    pub prop_bond: Balance,
    pub balance_spent: Balance,
    pub balance_cap: Balance,
    pub big_budget_balance: Balance,
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
        member_perms: Vec<PropPerm>,
        hook_auth: HashMap<AccountId, Vec<HookPerm>>,
        prop_bond: U128,
        balance_cap: U128,
        big_budget_balance: U128,
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
            prop_bond: prop_bond.0,
            balance_spent: 0,
            balance_cap: balance_cap.0,
            big_budget_balance: big_budget_balance.0,
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
    pub fn create_proposal(&mut self, kind: ProposalKind, description: String) -> u32 {
        let user = env::predecessor_account_id();
        let (members, perms) = self.members.get().unwrap();
        require!(
            env::attached_deposit() == self.prop_bond,
            "must attach correct amount of bond"
        );
        require!(members.binary_search(&user).is_ok(), "not a member");
        require!(
            perms.contains(&kind.required_perm()),
            "proposal kind not allowed"
        );

        match kind {
            ProposalKind::Budget(b) => require!(self.balance_spent + b < self.balance_cap),
            ProposalKind::RecurrentBudget(_) => {
                // TODO: need to multiply by remaining months and check for overlap
            }
            _ => (),
        };

        self.prop_counter += 1;
        emit_prop_created(self.prop_counter, &kind);
        self.proposals.insert(
            &self.prop_counter,
            &Proposal {
                proposer: user,
                description,
                kind,
                status: ProposalStatus::InProgress,
                approve: 0,
                reject: 0,
                votes: HashMap::new(),
                submission_time: env::block_timestamp_ms().into(),
            },
        );

        self.prop_counter
    }

    /// Veto proposal hook
    /// Removes proposal
    /// * `id`: proposal id
    pub fn veto_hook(&mut self, id: u32) {
        self.assert_hook_perm(&env::predecessor_account_id(), &HookPerm::Veto);
        let proposal = self.assert_proposal(id);
        match proposal.status {
            ProposalStatus::InProgress | ProposalStatus::Failed => {
                self.proposals.remove(&id);
            }
            _ => {
                panic!("Proposal finalized");
            }
        }
        emit_veto(id);
    }

    /*****************
     * INTERNAL
     ****************/

    fn assert_hook_perm(&self, user: &AccountId, perm: &HookPerm) {
        let auth_hook = self.hook_auth.get().unwrap();
        let perms = auth_hook.get(user);
        require!(
            perms.is_some() && perms.unwrap().contains(perm),
            "not authorized"
        );
    }

    fn assert_proposal(&self, id: u32) -> Proposal {
        self.proposals.get(&id).expect("proposal does not exist")
    }
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
