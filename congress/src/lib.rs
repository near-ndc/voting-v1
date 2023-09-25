use std::collections::HashMap;

use common::finalize_storage_check;
use events::*;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap};
use near_sdk::json_types::U128;
use near_sdk::{
    env, near_bindgen, AccountId, Balance, Gas, PanicOnDefault, Promise, PromiseOrValue,
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
    /// address of the community fund, where the excess of NEAR will be sent on dissolve and cleanup.
    pub community_fund: AccountId,

    pub dissolved: bool,
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,

    /// Map of accounts authorized create proposals and vote for proposals
    // We can use single object rather than LookupMap because the maximum amount of members
    // is 17 (for HoM: 15 + 2)
    pub members: LazyOption<(Vec<AccountId>, Vec<PropPerm>)>,
    /// minimum amount of members to approve the proposal
    pub threshold: u8,

    /// Map of accounts authorized to call hooks.
    pub hook_auth: LazyOption<HashMap<AccountId, Vec<HookPerm>>>,

    /// all times below are in miliseconds
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
        community_fund: AccountId,
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
        // we can support up to 255 with the limitation of the proposal type, but setting 100
        // here because this is more than enough for what we need to test for Congress.
        near_sdk::require!(members.len() <= 100, "max amount of members is 100");
        let threshold = (members.len() / 2) as u8 + 1;
        members.sort();
        Self {
            community_fund,
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
    /// Caller is required to attach enough deposit to cover the proposal storage as well as all
    /// possible votes (2*self.threshold - 1).
    /// NOTE: storage is paid from the account state
    #[payable]
    #[handle_result]
    pub fn create_proposal(
        &mut self,
        kind: PropKind,
        description: String,
    ) -> Result<u32, CreatePropError> {
        self.assert_active();
        let storage_start = env::storage_usage();
        let user = env::predecessor_account_id();
        let (members, perms) = self.members.get().unwrap();
        if members.binary_search(&user).is_err() {
            return Err(CreatePropError::NotAuthorized);
        }
        if !perms.contains(&kind.required_perm()) {
            return Err(CreatePropError::KindNotAllowed);
        }

        let now = env::block_timestamp_ms();
        let mut new_budget = 0;
        match kind {
            PropKind::FundingRequest(b) => {
                new_budget = self.budget_spent + b;
            }
            PropKind::RecurrentFundingRequest(b) => {
                new_budget = self.budget_spent + b * (self.remaining_months(now) as u128);
            }
            _ => (),
        };
        if new_budget > self.budget_cap {
            return Err(CreatePropError::BudgetOverflow);
        }

        self.prop_counter += 1;
        emit_prop_created(self.prop_counter, &kind);
        self.proposals.insert(
            &self.prop_counter,
            &Proposal {
                proposer: user.clone(),
                description,
                kind,
                status: ProposalStatus::InProgress,
                approve: 0,
                reject: 0,
                votes: HashMap::new(),
                submission_time: now,
            },
        );

        // max amount of votes is threshold + threshold-1.
        let extra_storage = VOTE_STORAGE * (2 * self.threshold - 1) as u64;
        if let Err(reason) = finalize_storage_check(storage_start, extra_storage, user) {
            return Err(CreatePropError::Storage(reason));
        }

        Ok(self.prop_counter)
    }

    // TODO: add immediate execution. Note can be automatically executed only when
    // contract.cooldown == 0
    #[handle_result]
    pub fn vote(&mut self, id: u32, vote: Vote) -> Result<(), VoteError> {
        self.assert_active();
        let user = env::predecessor_account_id();
        let (members, _) = self.members.get().unwrap();
        if members.binary_search(&user).is_err() {
            return Err(VoteError::NotAuthorized);
        }
        let mut prop = self.assert_proposal(id);

        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        if env::block_timestamp_ms() > prop.submission_time + self.voting_duration {
            return Err(VoteError::NotActive);
        }
        prop.add_vote(user, vote, self.threshold)?;
        self.proposals.insert(&id, &prop);
        emit_vote(id);
        Ok(())
    }

    /// Allows anyone to execute proposal.
    /// If `contract.cooldown` is set, then a proposal can be only executed after the cooldown:
    /// (submission_time + voting_duration + cooldown).
    #[handle_result]
    pub fn execute(&mut self, id: u32) -> Result<PromiseOrValue<()>, ExecError> {
        self.assert_active();
        let mut prop = self.assert_proposal(id);
        if !matches!(
            prop.status,
            // if the previous proposal execution failed, we should be able to re-execute it
            ProposalStatus::Approved | ProposalStatus::Failed
        ) {
            return Err(ExecError::NotApproved);
        }
        let now = env::block_timestamp_ms();
        if self.cooldown > 0 && now <= prop.submission_time + self.voting_duration + self.cooldown {
            return Err(ExecError::ExecTime);
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
            if self.budget_spent > self.budget_cap {
                return Err(ExecError::BudgetOverflow);
            }
        }
        self.proposals.insert(&id, &prop);

        let result = match result {
            PromiseOrValue::Promise(promise) => promise
                .then(
                    ext_self::ext(env::current_account_id())
                        .with_static_gas(EXECUTE_CALLBACK_GAS)
                        .on_execute(id, budget.into()),
                )
                .into(),
            _ => result,
        };
        Ok(result)
    }

    /// Veto proposal hook
    /// Removes proposal
    /// * `id`: proposal id
    #[handle_result]
    pub fn veto_hook(&mut self, id: u32) -> Result<(), HookError> {
        self.assert_active();
        self.assert_hook_perm(&env::predecessor_account_id(), &HookPerm::Veto)?;
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
        Ok(())
    }

    /// Dissolve and finalize the DAO. Will send the excess account funds back to the community
    /// fund. If the term is over can be called by anyone.
    #[handle_result]
    pub fn dissolve_hook(&mut self) -> Result<(), HookError> {
        // only check permission if the DAO term is not over.
        if env::block_timestamp_ms() <= self.end_time {
            self.assert_hook_perm(&env::predecessor_account_id(), &HookPerm::Dissolve)?;
        }
        self.dissolve_and_cleanup();
        Ok(())
    }

    #[handle_result]
    pub fn dismiss_hook(&mut self, member: AccountId) -> Result<(), HookError> {
        self.assert_active();
        self.assert_hook_perm(&env::predecessor_account_id(), &HookPerm::Dismiss)?;
        let (mut members, perms) = self.members.get().unwrap();
        let idx = members.binary_search(&member);
        if idx.is_err() {
            return Err(HookError::NoMember);
        }
        members[idx.unwrap()] = members.pop().unwrap();

        emit_dismiss(&member);
        // If DAO doesn't have required threshold, then we dissolve.
        if members.len() < self.threshold as usize {
            self.dissolve_and_cleanup();
        }

        self.members.set(&(members, perms));
        Ok(())
    }

    /*****************
     * INTERNAL
     ****************/

    fn assert_hook_perm(&self, user: &AccountId, perm: &HookPerm) -> Result<(), HookError> {
        let auth_hook = self.hook_auth.get().unwrap();
        let perms = auth_hook.get(user);
        if perms.is_none() || !perms.unwrap().contains(perm) {
            return Err(HookError::NotAuthorized);
        }
        Ok(())
    }

    fn assert_proposal(&self, id: u32) -> Proposal {
        self.proposals.get(&id).expect("proposal does not exist")
    }

    fn assert_active(&self) {
        near_sdk::require!(!self.dissolved, "dao is dissolved");
        near_sdk::require!(
            self.end_time > env::block_timestamp_ms(),
            "dao term is over, call dissolve_hook!"
        );
    }

    fn dissolve_and_cleanup(&mut self) {
        self.dissolved = true;
        emit_dissolve();
        // we leave 10B extra storage
        let required_deposit = (env::storage_usage() + 10) as u128 * env::storage_byte_cost();
        let diff = env::account_balance() - required_deposit;
        if diff > 0 {
            Promise::new(self.community_fund.clone()).transfer(diff);
        }
    }

    fn remaining_months(&self, now: u64) -> u64 {
        if self.end_time <= now {
            return 0;
        }
        // TODO: make correct calculation.
        // Need to check if recurrent budget can start immeidately or from the next month.
        (self.end_time - now) / 30 / 24 / 3600 / 1000
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
                emit_executed(prop_id);
            }
        };
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests {
    use near_sdk::{
        test_utils::{VMContextBuilder},
        testing_env, VMContext,
    };
    use near_units::parse_near;

    use crate::*;

    /// 1ms in nano seconds
    const MSECOND: u64 = 1_000_000;
    const START: u64 = 10;
    // 5 Min in milliseconds
    const FIVE_MIN: u64 = 60 * 5 * 1000;

    fn acc(idx: u8) -> AccountId {
        AccountId::new_unchecked(format!("user-{}.near", idx))
    }

    fn community_fund() -> AccountId {
        AccountId::new_unchecked(format!("community-fund.near"))
    }

    fn voting_body() -> AccountId {
        AccountId::new_unchecked(format!("voting-body.near"))
    }

    fn coa() -> AccountId {
        AccountId::new_unchecked(format!("coa.near"))
    }

    fn setup_ctr() -> (VMContext, Contract, u32) {
        let mut context = VMContextBuilder::new().build();
        let start_time = FIVE_MIN;
        let end_time = start_time + FIVE_MIN;
        let mut hash_map = HashMap::new();
        hash_map.insert(coa(), vec![HookPerm::Veto]);
        hash_map.insert(voting_body(), vec![HookPerm::Dismiss, HookPerm::Dissolve]);

        let mut contract = Contract::new(
            community_fund(),
            start_time,
            end_time,
            FIVE_MIN,
            FIVE_MIN,
            vec![acc(1), acc(2), acc(3), acc(4)],
            vec![PropPerm::Text, PropPerm::RecurrentFundingRequest, PropPerm::FundingRequest],
            hash_map,
            U128(10000),
            U128(100000)
        );
        context.block_timestamp = start_time * MSECOND;
        context.predecessor_account_id = acc(1);
        context.attached_deposit = parse_near!("1 N");
        testing_env!(context.clone());

        let id = contract.create_proposal(PropKind::Text, "Proposal unit test 1".to_string()).unwrap();
        (context, contract, id)
    }

    #[test]
    fn test_basics() {
        let (ctx, contract, id) = setup_ctr();
        let prop = contract.get_proposal(id);
        assert!(prop.is_some());
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::InProgress);

        
    }
}
