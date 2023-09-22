use std::collections::HashMap;

use events::*;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap};
use near_sdk::json_types::U128;
use near_sdk::{
    env, near_bindgen, require, AccountId, Balance, Gas, PanicOnDefault, Promise, PromiseOrValue,
    PromiseResult,
};

mod constants;
mod errors;
mod events;
mod ext;
pub mod proposal;
mod storage;
mod view;

pub use crate::constants::*;
pub use crate::errors::*;
pub use crate::ext::*;
pub use crate::proposal::*;
use crate::storage::*;

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
    pub voting_duration: u64,

    pub budget_spent: Balance,
    pub budget_cap: Balance,
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
        voting_duration: u64,
        #[allow(unused_mut)] mut members: Vec<AccountId>,
        member_perms: Vec<PropPerm>,
        hook_auth: HashMap<AccountId, Vec<HookPerm>>,
        budget_cap: U128,
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
            voting_duration,
            budget_spent: 0,
            budget_cap: budget_cap.0,
            big_budget_balance: big_budget_balance.0,
        }
    }

    /*
     * Queries are in view.rs
     */

    /**********
     * TRANSACTIONS
     **********/

    /// Creates a new proposal. `start` and `end` is Unix Time in milliseconds.
    /// Returns the new proposal ID.
    /// NOTE: storage is paid from the account state
    #[payable]
    pub fn create_proposal(&mut self, kind: PropKind, description: String) -> u32 {
        self.assert_not_dissolved();
        let user = env::predecessor_account_id();
        let (members, perms) = self.members.get().unwrap();
        // TODO: add storage usage checks and return excess of deposit
        require!(members.binary_search(&user).is_ok(), "not a member");
        require!(
            perms.contains(&kind.required_perm()),
            "proposal kind not allowed"
        );

        let now = env::block_timestamp_ms();
        match kind {
            PropKind::FundingRequest(b) => {
                require!(
                    self.budget_spent + b < self.budget_cap,
                    "budget cap overflow"
                )
            }
            PropKind::RecurrentFundingRequest(b) => {
                require!(
                    self.budget_spent + b * (self.remaining_months(now) as u128) < self.budget_cap,
                    "budget cap overflow"
                )
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
                submission_time: now,
            },
        );

        self.prop_counter
    }

    // TODO: add immediate execution
    pub fn vote(&mut self, id: u32, vote: Vote) {
        self.assert_not_dissolved();
        let user = env::predecessor_account_id();
        let (members, _) = self.members.get().unwrap();
        require!(members.binary_search(&user).is_ok(), "not a member");
        let mut prop = self.assert_proposal(id);
        require!(matches!(prop.status, ProposalStatus::InProgress));
        require!(
            prop.submission_time + self.voting_duration < env::block_timestamp_ms(),
            "voting time is over"
        );
        prop.add_vote(user, vote, self.threshold);
        self.proposals.insert(&id, &prop);
        emit_vote(id);
    }

    pub fn execute(&mut self, id: u32) -> PromiseOrValue<()> {
        self.assert_not_dissolved();
        let mut prop = self.assert_proposal(id);
        require!(matches!(
            prop.status,
            // if the previous proposal execution failed, we should be able to re-execute it
            ProposalStatus::Approved | ProposalStatus::Failed
        ));
        let now = env::block_timestamp_ms();
        if self.cooldown > 0 {
            require!(
                prop.submission_time + self.voting_duration + self.cooldown > now,
                "can be executed only after cooldown"
            );
        }

        prop.status = ProposalStatus::Executed;
        let mut result = PromiseOrValue::Value(());
        let mut budget = 0;
        match &prop.kind {
            PropKind::FunctionCall {
                receiver_id,
                actions,
            } => {
                let mut promise = Promise::new(receiver_id.clone());
                for action in actions {
                    promise = promise.function_call(
                        action.method_name.clone(),
                        action.args.clone().into(),
                        action.deposit.0,
                        Gas(action.gas.0),
                    );
                }
                result = promise.into();
            }
            PropKind::FundingRequest(b) => budget = *b,
            PropKind::RecurrentFundingRequest(b) => {
                budget = *b * self.remaining_months(now) as u128
            }
            PropKind::Text => (),
        };
        if budget != 0 {
            self.budget_spent += budget;
            require!(self.budget_spent <= self.budget_cap, "budget cap overflow")
        }
        self.proposals.insert(&id, &prop);

        // TODO:
        // + return bond
        match result {
            PromiseOrValue::Promise(promise) => promise
                .then(
                    ext_self::ext(env::current_account_id())
                        .with_static_gas(EXECUTE_CALLBACK_GAS)
                        .on_execute(id, budget.into()),
                )
                .into(),
            _ => result,
        }
    }

    /// Veto proposal hook
    /// Removes proposal
    /// * `id`: proposal id
    pub fn veto_hook(&mut self, id: u32) {
        self.assert_not_dissolved();
        self.assert_hook_perm(&env::predecessor_account_id(), &HookPerm::Veto);
        let proposal = self.assert_proposal(id);
        // TODO: check cooldown. Cooldown finishes at
        // min(proposal.start+self.voting_duration, time when proposal passed) + self.cooldown

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

    pub fn dissolve_hook(&mut self) {
        self.assert_hook_perm(&env::predecessor_account_id(), &HookPerm::Dissolve);
        self.dissolved = true;
        emit_dissolve();
    }

    pub fn dismiss_hook(&mut self, member: AccountId) {
        self.assert_not_dissolved();
        self.assert_hook_perm(&env::predecessor_account_id(), &HookPerm::Dismiss);
        let (mut members, perms) = self.members.get().unwrap();
        let idx = members.binary_search(&member);
        require!(idx.is_ok(), "not found");
        members[idx.unwrap()] = members.pop().unwrap();

        emit_dismiss(&member);
        // If DAO doesn't have required threshold, then we dissolve.
        if members.len() < self.threshold as usize {
            self.dissolved = true;
            emit_dissolve();
        }

        self.members.set(&(members, perms));
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

    fn assert_not_dissolved(&self) {
        require!(!self.dissolved, "dao is dissolved");
    }

    fn remaining_months(&self, now: u64) -> u64 {
        if self.end_time <= now {
            return 0;
        }
        // TODO: make correct calculation.
        // Need to check if recurrent budget can start immeidately or from the next month.
        (now - self.end_time) / 30 / 24 / 3600 / 1000
    }

    #[private]
    pub fn on_execute(&mut self, prop_id: u32, budget: U128) {
        assert_eq!(
            env::promise_results_count(),
            1,
            "ERR_UNEXPECTED_CALLBACK_PROMISES"
        );
        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(_) => (),
            PromiseResult::Failed => {
                let mut prop = self.assert_proposal(prop_id);
                self.budget_spent -= budget.0;
                prop.status = ProposalStatus::Failed;
                self.proposals.insert(&prop_id, &prop);
            }
        };
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
