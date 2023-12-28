use std::cmp::min;
use std::collections::HashMap;

use common::errors::HookError;
use common::finalize_storage_check;
use events::*;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap};
use near_sdk::json_types::{Base64VecU8, U128};
use near_sdk::{
    env, near_bindgen, require, AccountId, Balance, Gas, PanicOnDefault, Promise, PromiseError,
    PromiseOrValue, PromiseResult,
};
use serde_json::json;

mod constants;
mod errors;
mod events;
mod ext;
mod migrate;
pub mod proposal;
mod storage;
pub mod view;

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
    /// I Am Human registry
    pub registry: AccountId,

    pub dissolved: bool,
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,

    /// Map of accounts authorized create proposals and vote for proposals
    // We can use single object rather than LookupMap because the maximum amount of members
    // is 17 (for HoM: 15 + 2)
    pub members: LazyOption<(Vec<AccountId>, Vec<PropPerm>)>,
    /// length of members
    pub members_len: u8,
    /// minimum amount of members to approve the proposal
    pub threshold: u8,

    /// Map of accounts authorized to call hooks.
    pub hook_auth: LazyOption<HashMap<AccountId, Vec<HookPerm>>>,

    /// all times below are in miliseconds
    pub start_time: u64,
    pub end_time: u64,
    pub cooldown: u64,
    pub vote_duration: u64,
    pub min_vote_duration: u64,

    pub budget_spent: Balance,
    pub budget_cap: Balance,
    /// size (in yocto NEAR) of the big funding request
    pub big_funding_threshold: Balance,
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
        vote_duration: u64,
        min_vote_duration: u64,
        #[allow(unused_mut)] mut members: Vec<AccountId>,
        member_perms: Vec<PropPerm>,
        hook_auth: HashMap<AccountId, Vec<HookPerm>>,
        budget_cap: U128,
        big_funding_threshold: U128,
        registry: AccountId,
    ) -> Self {
        // we can support up to 255 with the limitation of the proposal type, but setting 100
        // here because this is more than enough for what we need to test for Congress.
        let members_len = members.len() as u8;
        near_sdk::require!(members_len <= 100, "max amount of members is 100");
        let threshold = (members_len / 2) + 1;
        members.sort();
        Self {
            community_fund,
            dissolved: false,
            prop_counter: 0,
            proposals: LookupMap::new(StorageKey::Proposals),
            members: LazyOption::new(StorageKey::Members, Some(&(members, member_perms))),
            members_len,
            threshold,
            hook_auth: LazyOption::new(StorageKey::HookAuth, Some(&hook_auth)),
            start_time,
            end_time,
            cooldown,
            vote_duration,
            min_vote_duration,
            budget_spent: 0,
            budget_cap: budget_cap.0,
            big_funding_threshold: big_funding_threshold.0,
            registry,
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
        match &kind {
            PropKind::FundingRequest(b) => {
                new_budget = self.budget_spent + b.0;
            }
            PropKind::RecurrentFundingRequest(b) => {
                new_budget = self.budget_spent + b.0 * (self.remaining_months(now) as u128);
            }
            PropKind::FunctionCall { actions, .. } => {
                let mut sum_gas = 0;
                for a in actions {
                    if a.gas.0 < EXEC_CTR_CALL_GAS.0 || a.gas.0 > MAX_EXEC_FUN_CALL_GAS.0 {
                        return Err(CreatePropError::Gas(
                            "action gas must be between 8tgas and 280tgas".to_owned(),
                        ));
                    }
                    sum_gas += a.gas.0;
                }
                if sum_gas > MAX_EXEC_FUN_CALL_GAS.0 {
                    return Err(CreatePropError::Gas(
                        "sum of action gas can't exceed 276tgas".to_owned(),
                    ));
                }
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
                abstain: 0,
                votes: HashMap::new(),
                submission_time: now,
                approved_at: None,
            },
        );

        // max amount of votes is threshold + threshold-1.
        let extra_storage = VOTE_STORAGE * (2 * self.threshold - 1) as u64;
        if let Err(reason) = finalize_storage_check(storage_start, extra_storage, user) {
            return Err(CreatePropError::Storage(reason));
        }

        Ok(self.prop_counter)
    }

    #[handle_result]
    pub fn vote(&mut self, id: u32, vote: Vote) -> Result<(), VoteError> {
        self.assert_active();
        let user = env::predecessor_account_id();
        let (members, _) = self.members.get().unwrap();
        if members.binary_search(&user).is_err() {
            return Err(VoteError::NotAuthorized);
        }
        let mut prop = self.assert_proposal(id);

        self.assert_member_not_involved(&prop, &user)?;

        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        let now = env::block_timestamp_ms();
        if now > prop.submission_time + self.vote_duration {
            return Err(VoteError::NotActive);
        }

        prop.add_vote(user, vote, self.threshold)?;
        prop.finalize_status(
            members.len(),
            self.threshold,
            self.min_vote_duration,
            self.vote_duration,
        );

        self.proposals.insert(&id, &prop);
        emit_vote(id);

        // automatic execution
        if matches!(prop.status, ProposalStatus::Approved) && self.cooldown == 0 {
            // We ignore a failure of self.execute here to assure that the vote is counted.
            let res = self.execute(id);
            if res.is_err() {
                emit_vote_execute_fail(id, res.err().unwrap());
            }
        }

        Ok(())
    }

    /// Allows anyone to execute proposal.
    /// If `contract.cooldown` is set, then a proposal can be only executed after the cooldown:
    /// (submission_time + vote_duration + cooldown).
    #[handle_result]
    pub fn execute(
        &mut self,
        id: u32,
    ) -> Result<PromiseOrValue<Result<(), ExecRespErr>>, ExecError> {
        self.assert_active();
        let mut prop = self.assert_proposal(id);
        if matches!(prop.status, ProposalStatus::Executed) {
            // More fine-grained errors
            return Err(ExecError::AlreadyExecuted);
        }
        // check if we can finalize the proposal status due to having enough votes during min_vote_duration
        if matches!(prop.status, ProposalStatus::InProgress) {
            let (members, _) = self.members.get().unwrap();
            if !prop.finalize_status(
                members.len(),
                self.threshold,
                self.min_vote_duration,
                self.vote_duration,
            ) {
                return Err(ExecError::MinVoteDuration);
            }
        }
        if !matches!(
            prop.status,
            // if the previous proposal execution failed, we should be able to re-execute it
            ProposalStatus::Approved | ProposalStatus::Failed
        ) {
            return Err(ExecError::NotApproved);
        }

        let now = env::block_timestamp_ms();
        if self.cooldown > 0 && now <= prop.approved_at.unwrap() + self.cooldown {
            return Err(ExecError::ExecTime);
        }

        prop.status = ProposalStatus::Executed;
        let mut result = PromiseOrValue::Value(Ok(()));
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
                        // TODO: remove the following changes in v1.2
                        0, //action.deposit.0,
                        Gas(action.gas.0) - EXEC_SELF_GAS,
                    );
                }
                result = promise.into();
            }
            PropKind::FundingRequest(b) => budget = b.0,
            PropKind::RecurrentFundingRequest(b) => {
                budget = b.0 * self.remaining_months(now) as u128
            }
            PropKind::Text => (),
            PropKind::DismissAndBan { member, house } => {
                self.proposals.insert(&id, &prop);

                let ban_promise = Promise::new(self.registry.clone()).function_call(
                    "admin_flag_accounts".to_owned(),
                    json!({ "flag": "GovBan".to_owned(),
                        "accounts": vec![member],
                        "memo": "".to_owned()
                    })
                    .to_string()
                    .as_bytes()
                    .to_vec(),
                    0,
                    EXEC_CTR_CALL_GAS,
                );

                let dismiss_promise = Promise::new(house.clone()).function_call(
                    "dismiss_hook".to_owned(),
                    json!({ "member": member }).to_string().as_bytes().to_vec(),
                    0,
                    EXEC_CTR_CALL_GAS,
                );

                return Ok(PromiseOrValue::Promise(
                    ban_promise.and(dismiss_promise).then(
                        ext_self::ext(env::current_account_id())
                            .with_static_gas(EXECUTE_CALLBACK_GAS)
                            .on_ban_dismiss(id),
                    ),
                ));
            }
        };
        if budget != 0 {
            self.budget_spent += budget;
            if self.budget_spent > self.budget_cap {
                prop.status = ProposalStatus::Rejected;
                self.proposals.insert(&id, &prop);
                return Ok(PromiseOrValue::Value(Err(ExecRespErr::BudgetOverflow)));
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
            _ => {
                emit_executed(id);
                result
            }
        };
        Ok(result)
    }

    /// Veto proposal hook
    /// * `id`: proposal id
    #[handle_result]
    pub fn veto_hook(&mut self, id: u32) -> Result<(), HookError> {
        self.assert_active();
        let mut proposal = self.assert_proposal(id);
        let is_big_or_recurrent = match proposal.kind {
            PropKind::FundingRequest(b) => b.0 >= self.big_funding_threshold,
            PropKind::RecurrentFundingRequest(_) => true,
            _ => false,
        };
        let caller = env::predecessor_account_id();
        if is_big_or_recurrent {
            self.assert_hook_perm(
                &caller,
                &[HookPerm::VetoBigOrReccurentFundingReq, HookPerm::VetoAll],
            )?;
        } else {
            self.assert_hook_perm(&caller, &[HookPerm::VetoAll])?;
        }

        match proposal.status {
            ProposalStatus::InProgress => {
                proposal.status = ProposalStatus::Vetoed;
            }
            ProposalStatus::Approved => {
                let cooldown = min(
                    proposal.submission_time + self.vote_duration,
                    proposal.approved_at.unwrap(),
                ) + self.cooldown;
                if cooldown < env::block_timestamp_ms() {
                    return Err(HookError::CooldownOver);
                }
                proposal.status = ProposalStatus::Vetoed;
            }
            _ => {
                return Err(HookError::ProposalFinalized);
            }
        }
        emit_veto(id);
        self.proposals.insert(&id, &proposal);
        Ok(())
    }

    /// Dissolve and finalize the DAO. Will send the excess account funds back to the community
    /// fund. If the term is over can be called by anyone.
    #[handle_result]
    pub fn dissolve_hook(&mut self) -> Result<(), HookError> {
        // only check permission if the DAO term is not over.
        if env::block_timestamp_ms() <= self.end_time {
            self.assert_hook_perm(&env::predecessor_account_id(), &[HookPerm::Dissolve])?;
        }
        self.dissolve_and_cleanup();
        Ok(())
    }

    #[handle_result]
    pub fn dismiss_hook(&mut self, member: AccountId) -> Result<(), HookError> {
        self.assert_active();
        self.assert_hook_perm(&env::predecessor_account_id(), &[HookPerm::Dismiss])?;
        let (mut members, perms) = self.members.get().unwrap();
        let idx = members.binary_search(&member);
        if idx.is_err() {
            // We need to return OK to allow to call this function multiple times to execute proposal which may compose other actions
            return Ok(());
        }
        members.remove(idx.unwrap());

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

    /// Returns Ok if the user has at least one of the `require_any` permissions.
    /// Otherwise returns Err.
    fn assert_hook_perm(
        &self,
        user: &AccountId,
        require_any: &[HookPerm],
    ) -> Result<(), HookError> {
        let auth_hook = self.hook_auth.get().unwrap();
        let perms = auth_hook.get(user);
        if perms.is_none() {
            return Err(HookError::NotAuthorized);
        }
        let perms = perms.unwrap();
        for r in require_any {
            if perms.contains(r) {
                return Ok(());
            }
        }
        Err(HookError::NotAuthorized)
    }

    fn assert_proposal(&self, id: u32) -> Proposal {
        self.proposals.get(&id).expect("proposal does not exist")
    }

    fn assert_member_not_involved(
        &self,
        prop: &Proposal,
        user: &AccountId,
    ) -> Result<(), VoteError> {
        match &prop.kind {
            PropKind::DismissAndBan { member, house: _ } => {
                if member == user {
                    return Err(VoteError::NoSelfVote);
                }
            }
            PropKind::FunctionCall {
                receiver_id: _,
                actions,
            } => {
                for action in actions {
                    if &action.method_name == "dismiss_hook" {
                        let encoded =
                            Base64VecU8(json!({ "member": user }).to_string().as_bytes().to_vec());
                        if encoded == action.args {
                            return Err(VoteError::NoSelfVote);
                        }
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }

    fn assert_active(&self) {
        near_sdk::require!(!self.dissolved, "dao is dissolved");
        near_sdk::require!(
            env::block_timestamp_ms() <= self.end_time,
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
        // Need to check if recurrent budget can start immediately or from the next month.
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
            PromiseResult::Successful(_) => {}
            PromiseResult::Failed => {
                let mut prop = self.assert_proposal(prop_id);
                self.budget_spent -= budget.0;
                prop.status = ProposalStatus::Failed;
                self.proposals.insert(&prop_id, &prop);
                emit_executed(prop_id);
            }
        };
    }

    #[private]
    pub fn on_ban_dismiss(
        &mut self,
        #[callback_result] ban_result: Result<(), PromiseError>,
        #[callback_result] dismiss_result: Result<(), PromiseError>,
        prop_id: u32,
    ) {
        if ban_result.is_err() || dismiss_result.is_err() {
            let mut prop = self.assert_proposal(prop_id);
            prop.status = ProposalStatus::Failed;
            self.proposals.insert(&prop_id, &prop);
            emit_executed(prop_id);
        }
    }

    /// Every house should be able to make a fun call proposals
    pub fn add_fun_call_perm(&mut self) {
        require!(env::predecessor_account_id() == env::current_account_id());
        let mut m = self.members.get().unwrap();
        if m.1.contains(&PropPerm::FunctionCall) {
            m.1.push(PropPerm::FunctionCall);
            self.members.set(&m);
        }
    }

    /// v1.1.2 release: we missed TC perm to dismiss a member from "self"
    // TODO: we can remove this method in the next release
    pub fn add_tc_dismiss_perm(&mut self) {
        let tc = env::predecessor_account_id();
        require!(tc.as_str() == "congress-tc-v1.ndc-gwg.near");
        let mut hooks = self.hook_auth.get().unwrap();
        match hooks.get_mut(&tc) {
            None => {
                hooks.insert(tc, vec![HookPerm::Dismiss]);
            }
            Some(tc_perms) => {
                if !tc_perms.contains(&HookPerm::Dismiss) {
                    tc_perms.push(HookPerm::Dismiss);
                }
            }
        }
        self.hook_auth.set(&hooks);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests {
    use near_sdk::{
        test_utils::{get_logs, VMContextBuilder},
        testing_env, VMContext,
    };

    use crate::{view::MembersOutput, *};
    use near_sdk::json_types::{U128, U64};

    /// 1ms in nano seconds
    const MSECOND: u64 = 1_000_000;

    // In milliseconds
    const START: u64 = 60 * 5 * 1000;
    const TERM: u64 = 60 * 15 * 1000;
    const VOTE_DURATION: u64 = 60 * 5 * 1000;
    const MIN_VOTE_DURATION: u64 = 30 * 5 * 1000;
    const COOLDOWN: u64 = 40 * 5 * 1000;

    fn acc(idx: u8) -> AccountId {
        AccountId::new_unchecked(format!("user-{}.near", idx))
    }

    fn community_fund() -> AccountId {
        AccountId::new_unchecked("community-fund.near".to_string())
    }

    fn voting_body() -> AccountId {
        AccountId::new_unchecked("voting-body.near".to_string())
    }

    fn coa() -> AccountId {
        AccountId::new_unchecked("coa.near".to_string())
    }

    fn registry() -> AccountId {
        AccountId::new_unchecked("registry.near".to_string())
    }

    fn setup_ctr(attach_deposit: u128) -> (VMContext, Contract, u32) {
        let mut context = VMContextBuilder::new().build();
        let end_time = START + TERM;
        let mut hook_perms = HashMap::new();
        hook_perms.insert(coa(), vec![HookPerm::VetoAll]);
        hook_perms.insert(
            voting_body(),
            vec![
                HookPerm::Dismiss,
                HookPerm::Dissolve,
                HookPerm::VetoBigOrReccurentFundingReq,
            ],
        );

        let mut contract = Contract::new(
            community_fund(),
            START,
            end_time,
            COOLDOWN,
            VOTE_DURATION,
            MIN_VOTE_DURATION,
            vec![acc(1), acc(2), acc(3), acc(4)],
            vec![
                PropPerm::Text,
                PropPerm::RecurrentFundingRequest,
                PropPerm::FundingRequest,
                PropPerm::FunctionCall,
                PropPerm::DismissAndBan,
            ],
            hook_perms,
            U128(10000),
            U128(1000),
            registry(),
        );
        context.block_timestamp = START * MSECOND;
        context.predecessor_account_id = acc(1);
        context.attached_deposit = attach_deposit * MILI_NEAR;
        testing_env!(context.clone());

        let id = contract
            .create_proposal(PropKind::Text, "Proposal unit test 1".to_string())
            .unwrap();
        (context, contract, id)
    }

    fn vote(mut ctx: VMContext, mut ctr: Contract, accounts: Vec<AccountId>, id: u32) -> Contract {
        for account in accounts {
            ctx.predecessor_account_id = account;
            testing_env!(ctx.clone());
            assert_eq!(ctr.vote(id, Vote::Approve), Ok(()));
        }
        ctr
    }

    fn assert_hook_not_auth(res: Result<(), HookError>) {
        assert!(
            matches!(res, Err(HookError::NotAuthorized)),
            "got: {:?}",
            res
        );
    }

    fn assert_create_prop_not_allowed(res: Result<u32, CreatePropError>) {
        assert!(
            matches!(res, Err(CreatePropError::KindNotAllowed)),
            "got: {:?}",
            res
        );
    }

    fn assert_exec_ok(res: Result<PromiseOrValue<Result<(), ExecRespErr>>, ExecError>) {
        match res {
            Ok(_) => (),
            Err(err) => panic!("expecting Ok, got {:?}", err),
        };
    }

    #[test]
    fn basic_flow() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        let mut prop = ctr.get_proposal(id);
        assert!(prop.is_some());
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::InProgress);

        assert_eq!(ctr.number_of_proposals(), 1);

        // check `get_proposals` query
        let res = ctr.get_proposals(0, 10, Some(false));
        assert_eq!(res, vec![ctr.get_proposal(id).unwrap()]);

        let id2 = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();

        let id3 = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 3".to_string())
            .unwrap();

        // reverse query
        let res = ctr.get_proposals(10, 10, Some(true));
        assert_eq!(
            res,
            vec![
                ctr.get_proposal(id3).unwrap(),
                ctr.get_proposal(id2).unwrap(),
                ctr.get_proposal(id).unwrap()
            ]
        );

        let res = ctr.get_proposals(3, 1, Some(true));
        assert_eq!(res, vec![ctr.get_proposal(id3).unwrap(),]);

        ctx.block_timestamp = (START + MIN_VOTE_DURATION + 10) * MSECOND;
        testing_env!(ctx.clone());
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        prop = ctr.get_proposal(id);
        assert!(prop.is_some());
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::Approved);

        ctx.predecessor_account_id = acc(4);
        testing_env!(ctx.clone());
        assert_eq!(ctr.vote(id, Vote::Approve), Err(VoteError::NotInProgress));

        ctx.block_timestamp = START * MSECOND;
        testing_env!(ctx.clone());
        let id = ctr
            .create_proposal(PropKind::Text, "proposal".to_owned())
            .unwrap();
        assert_eq!(ctr.vote(id, Vote::Approve), Ok(()));
        assert_eq!(ctr.vote(id, Vote::Reject), Err(VoteError::DoubleVote));
        assert_eq!(ctr.vote(id, Vote::Approve), Err(VoteError::DoubleVote));

        ctx.block_timestamp = (ctr.start_time + ctr.vote_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        assert_eq!(ctr.vote(id, Vote::Approve), Err(VoteError::NotActive));

        ctx.predecessor_account_id = acc(5);
        testing_env!(ctx.clone());
        assert_eq!(ctr.vote(id, Vote::Approve), Err(VoteError::NotAuthorized));

        ctx.predecessor_account_id = acc(2);
        ctx.block_timestamp = START * MSECOND;
        testing_env!(ctx.clone());

        // set cooldown=0 and min_vote_duration=0 and test for immediate execution
        ctr.cooldown = 0;
        ctr.min_vote_duration = 0;
        let id = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);
        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);

        // create proposal, set timestamp past voting period, status should be rejected
        let id = ctr
            .create_proposal(PropKind::Text, "Proposal unit test query 3".to_string())
            .unwrap();
        let prop = ctr.get_proposal(id).unwrap();
        ctx.block_timestamp = (prop.proposal.submission_time + ctr.vote_duration + 1) * MSECOND;
        testing_env!(ctx);

        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Rejected);
    }

    #[test]
    #[should_panic(expected = "proposal does not exist")]
    fn proposal_does_not_exist() {
        let (_, mut ctr, _) = setup_ctr(100);
        ctr.vote(10, Vote::Approve).unwrap();
    }

    #[test]
    fn proposal_create_prop_permissions() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);
        let (members, _) = ctr.members.get().unwrap();
        ctr.members.set(&(members, vec![PropPerm::FundingRequest]));

        ctr.create_proposal(PropKind::FundingRequest(U128(10)), "".to_string())
            .unwrap();

        // creating other proposal kinds should fail
        assert_create_prop_not_allowed(
            ctr.create_proposal(PropKind::RecurrentFundingRequest(U128(10)), "".to_string()),
        );
        assert_create_prop_not_allowed(ctr.create_proposal(PropKind::Text, "".to_string()));
        assert_create_prop_not_allowed(ctr.create_proposal(
            PropKind::FunctionCall {
                receiver_id: acc(10),
                actions: vec![],
            },
            "".to_string(),
        ));

        ctx.attached_deposit = 1;
        testing_env!(ctx.clone());
        assert!(matches!(
            ctr.create_proposal(PropKind::FundingRequest(U128(1)), "".to_string()),
            Err(CreatePropError::Storage(_))
        ));

        ctx.predecessor_account_id = acc(6);
        ctx.attached_deposit = 10 * MILI_NEAR;
        testing_env!(ctx.clone());
        assert_eq!(
            ctr.create_proposal(PropKind::Text, "".to_string()),
            Err(CreatePropError::NotAuthorized)
        );

        // set remaining months to 2
        let (members, _) = ctr.members.get().unwrap();
        ctr.members
            .set(&(members, vec![PropPerm::RecurrentFundingRequest]));
        ctr.end_time = ctr.start_time + START * 12 * 24 * 61;
        ctx.predecessor_account_id = acc(2);
        testing_env!(ctx);
        assert_eq!(
            ctr.create_proposal(
                PropKind::RecurrentFundingRequest(U128((ctr.budget_cap / 2) + 1)),
                "".to_string(),
            ),
            Err(CreatePropError::BudgetOverflow)
        );
    }

    #[test]
    fn proposal_execution_text() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        match ctr.execute(id) {
            Ok(_) => panic!("expected NotApproved, got: OK"),
            Err(err) => assert_eq!(err, ExecError::MinVoteDuration),
        }
        ctx.block_timestamp = (START + MIN_VOTE_DURATION + 10) * MSECOND;
        testing_env!(ctx.clone());
        match ctr.execute(id) {
            Ok(_) => panic!("expected NotApproved, got: OK"),
            Err(err) => assert_eq!(err, ExecError::NotApproved),
        }
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        let mut prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        match ctr.execute(id) {
            Err(err) => assert_eq!(err, ExecError::ExecTime),
            Ok(_) => panic!("expected ExecTime, got: OK"),
        }

        ctx.block_timestamp = (ctr.start_time + ctr.cooldown + ctr.vote_duration + 1) * MSECOND;
        testing_env!(ctx);
        assert_exec_ok(ctr.execute(id));

        prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);

        //
        // check double execution
        match ctr.execute(id) {
            Ok(_) => panic!("expecting Err"),
            Err(err) => assert_eq!(err, ExecError::AlreadyExecuted),
        };
    }

    #[test]
    fn proposal_execution_funding_req() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);

        let id = ctr
            .create_proposal(
                PropKind::FundingRequest(U128(1000u128)),
                "Funding req".to_owned(),
            )
            .unwrap();

        ctx.block_timestamp = (START + MIN_VOTE_DURATION + 10) * MSECOND;
        testing_env!(ctx.clone());
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        ctx.block_timestamp = (ctr.start_time + ctr.cooldown + ctr.vote_duration + 1) * MSECOND;
        testing_env!(ctx);

        assert_eq!(ctr.budget_spent, 0);
        assert_exec_ok(ctr.execute(id));
        assert_eq!(ctr.budget_spent, 1000);

        let res = ctr.create_proposal(
            PropKind::FundingRequest(U128(10000u128)),
            "Funding req".to_owned(),
        );
        match res {
            Err(CreatePropError::BudgetOverflow) => (),
            x => panic!("expected BudgetOverflow, got: {:?}", x),
        }
    }

    #[test]
    fn proposal_execution_rec_funding_req() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);

        let id = ctr
            .create_proposal(
                PropKind::RecurrentFundingRequest(U128(10u128)),
                "Rec Funding req".to_owned(),
            )
            .unwrap();

        ctx.block_timestamp = (START + MIN_VOTE_DURATION + 10) * MSECOND;
        testing_env!(ctx.clone());
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        // update to more than two months
        ctr.end_time = ctr.start_time + START * 12 * 24 * 61;
        ctx.block_timestamp = (ctr.start_time + ctr.cooldown + ctr.vote_duration + 1) * MSECOND;
        testing_env!(ctx);

        // proposal isn't executed so budget spent is 0
        assert_eq!(ctr.budget_spent, 0);
        assert_exec_ok(ctr.execute(id));

        // budget spent * remaining months
        assert_eq!(ctr.budget_spent, 20);
    }

    #[test]
    fn proposal_execution_budget_overflow() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);
        ctr.min_vote_duration = 0;

        // create and approve a funding requst that will fill up the budget.
        let id1 = ctr
            .create_proposal(
                PropKind::FundingRequest((ctr.budget_cap).into()),
                "Funding req".to_owned(),
            )
            .unwrap();
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id1);

        // create a second proposal, that will go over the budget if proposal id1 is executed
        let time_diff = 10;
        ctx.block_timestamp += time_diff * MSECOND;
        testing_env!(ctx.clone());
        let id2 = ctr
            .create_proposal(
                PropKind::FundingRequest(10.into()),
                "Funding req".to_owned(),
            )
            .unwrap();
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id2);
        let p2 = ctr.get_proposal(id2).unwrap();
        assert_eq!(p2.proposal.status, ProposalStatus::Approved);

        // execute the first proposal - it should work.
        ctx.block_timestamp += (VOTE_DURATION + COOLDOWN) * MSECOND;
        testing_env!(ctx.clone());
        assert_exec_ok(ctr.execute(id1));

        // execution of the second proposal should work, but the proposal should be rejected
        ctx.block_timestamp += (time_diff + 1) * MSECOND;
        testing_env!(ctx.clone());
        match ctr.execute(id2) {
            Ok(PromiseOrValue::Value(resp)) => assert_eq!(resp, Err(ExecRespErr::BudgetOverflow)),
            Ok(PromiseOrValue::Promise(_)) => {
                panic!("expecting Ok ExecRespErr::BudgetOverflow, got Ok Promise");
            }
            Err(err) => panic!(
                "expecting Ok ExecRespErr::BudgetOverflow, got: Err: {:?}",
                err
            ),
        }
    }

    #[test]
    #[should_panic(expected = "dao term is over, call dissolve_hook!")]
    fn dao_dissolve_time() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        ctx.block_timestamp = (ctr.end_time + 1) * MSECOND;
        testing_env!(ctx);

        ctr.vote(id, Vote::Approve).unwrap();
    }

    #[test]
    fn veto_hook() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        ctr.get_proposal(id).unwrap();
        match ctr.veto_hook(id) {
            Err(HookError::NotAuthorized) => (),
            x => panic!("expected NotAuthorized, got: {:?}", x),
        }

        ctx.predecessor_account_id = coa();
        testing_env!(ctx.clone());

        // Veto during voting phase(before cooldown)
        ctr.veto_hook(id).unwrap();
        let expected = r#"EVENT_JSON:{"standard":"ndc-congress","version":"1.0.0","event":"veto","data":{"prop_id":1}}"#;
        assert_eq!(vec![expected], get_logs());

        let mut prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Vetoed);

        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx.clone());

        // Veto during cooldown
        let id = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();

        // Set timestamp close to voting end duration
        ctx.block_timestamp = (prop.proposal.submission_time + ctr.vote_duration - 1) * MSECOND;
        testing_env!(ctx.clone());

        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);
        prop = ctr.get_proposal(id).unwrap();

        // Set timestamp to during cooldown, after voting phase
        ctx.block_timestamp = (prop.proposal.submission_time + ctr.vote_duration + 1) * MSECOND;
        ctx.predecessor_account_id = coa();
        testing_env!(ctx.clone());

        ctr.veto_hook(id).unwrap();
        // veto vetoed prop
        assert_eq!(ctr.veto_hook(id), Err(HookError::ProposalFinalized));

        ctx.block_timestamp = START * MSECOND;
        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx.clone());
        let id = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();

        ctx.block_timestamp = (START + MIN_VOTE_DURATION + 10) * MSECOND;
        testing_env!(ctx.clone());
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        // Set timestamp to after cooldown
        ctx.block_timestamp =
            (prop.proposal.submission_time + ctr.vote_duration + ctr.cooldown + 1) * MSECOND;
        ctx.predecessor_account_id = coa();
        testing_env!(ctx);

        // Can execute past cooldown but not veto proposal
        assert_eq!(ctr.veto_hook(id), Err(HookError::CooldownOver));
        assert_exec_ok(ctr.execute(id));

        // Cannot veto executed or failed proposal
        assert_eq!(ctr.veto_hook(id), Err(HookError::ProposalFinalized));

        let mut prop = ctr.proposals.get(&id).unwrap();
        prop.status = ProposalStatus::Failed;
        ctr.proposals.insert(&id, &prop);
        assert_eq!(ctr.veto_hook(id), Err(HookError::ProposalFinalized));
    }

    fn create_all_props(ctr: &mut Contract) -> (u32, u32, u32, u32, u32) {
        let prop_text = ctr
            .create_proposal(PropKind::Text, "text proposal".to_string())
            .unwrap();
        let prop_fc = ctr
            .create_proposal(
                PropKind::FunctionCall {
                    receiver_id: acc(10),
                    actions: vec![],
                },
                "function call proposal".to_string(),
            )
            .unwrap();

        let prop_big = ctr
            .create_proposal(
                PropKind::FundingRequest(U128(1100)),
                "big funding request".to_string(),
            )
            .unwrap();
        let prop_small = ctr
            .create_proposal(
                PropKind::FundingRequest(U128(200)),
                "small funding request".to_string(),
            )
            .unwrap();
        let prop_rec = ctr
            .create_proposal(
                PropKind::RecurrentFundingRequest(U128(200)),
                "recurrent funding request".to_string(),
            )
            .unwrap();

        (prop_text, prop_fc, prop_big, prop_small, prop_rec)
    }

    #[test]
    fn veto_hook_big_funding_request() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);

        // CoA should be able to veto everything
        let (p_text, p_fc, p_big, p_small, p_rec) = create_all_props(&mut ctr);
        ctx.predecessor_account_id = coa();
        testing_env!(ctx.clone());
        ctr.veto_hook(p_text).unwrap();
        ctr.veto_hook(p_fc).unwrap();
        ctr.veto_hook(p_big).unwrap();
        ctr.veto_hook(p_small).unwrap();
        ctr.veto_hook(p_rec).unwrap();

        // Voting Body should only be able to veto big funding req. or recurrent one.
        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx.clone());
        let (p_text, p_fc, p_big, p_small, p_rec) = create_all_props(&mut ctr);
        ctx.predecessor_account_id = voting_body();
        testing_env!(ctx);
        ctr.veto_hook(p_big).unwrap();
        ctr.veto_hook(p_rec).unwrap();
        assert_hook_not_auth(ctr.veto_hook(p_text));
        assert_hook_not_auth(ctr.veto_hook(p_fc));
        assert_hook_not_auth(ctr.veto_hook(p_small));
    }

    #[test]
    #[should_panic(expected = "dao is dissolved")]
    fn dissolve_hook() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);

        match ctr.dissolve_hook() {
            Err(HookError::NotAuthorized) => (),
            x => panic!("expected NotAuthorized, got: {:?}", x),
        }

        ctx.predecessor_account_id = voting_body();
        testing_env!(ctx);

        ctr.dissolve_hook().unwrap();
        let expected = r#"EVENT_JSON:{"standard":"ndc-congress","version":"1.0.0","event":"dissolve","data":""}"#;
        assert_eq!(vec![expected], get_logs());
        assert!(ctr.dissolved);

        ctr.create_proposal(
            PropKind::FundingRequest(U128(10000u128)),
            "Funding req".to_owned(),
        )
        .unwrap();
    }

    #[test]
    fn dismiss_hook() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);

        assert_eq!(ctr.dismiss_hook(acc(2)), Err(HookError::NotAuthorized));

        ctx.predecessor_account_id = voting_body();
        testing_env!(ctx.clone());
        assert_eq!(ctr.dismiss_hook(acc(10)), Ok(()));
        ctr.dismiss_hook(acc(2)).unwrap();

        let expected = r#"EVENT_JSON:{"standard":"ndc-congress","version":"1.0.0","event":"dismiss","data":{"member":"user-2.near"}}"#;
        assert_eq!(vec![expected], get_logs());
        assert_eq!(ctr.member_permissions(acc(2)), vec![]);

        // Proposal should not pass with only 2 votes
        let mut prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::InProgress);
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(3)].to_vec(), id);

        prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::InProgress);

        ctx.predecessor_account_id = voting_body();
        testing_env!(ctx);

        assert!(!ctr.dissolved);
        // Remove more members to check dissolve
        ctr.dismiss_hook(acc(1)).unwrap();
        assert!(ctr.dissolved);
    }

    #[test]
    fn dismiss_order() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);
        ctx.predecessor_account_id = voting_body();
        testing_env!(ctx);

        let (mut members, permissions) = ctr.members.get().unwrap();
        members.push(acc(5));
        members.push(acc(6));
        ctr.members.set(&(members, permissions.clone()));

        // remove from middle
        ctr.dismiss_hook(acc(2)).unwrap();

        // should be sorted list
        assert_eq!(
            ctr.get_members(),
            MembersOutput {
                members: vec![acc(1), acc(3), acc(4), acc(5), acc(6)],
                permissions: permissions.clone()
            }
        );

        // Remove more members
        ctr.dismiss_hook(acc(1)).unwrap();
        assert_eq!(
            ctr.get_members(),
            MembersOutput {
                members: vec![acc(3), acc(4), acc(5), acc(6)],
                permissions
            }
        );
    }

    #[test]
    fn tc_dismiss_ban() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);
        let motion_rem_ban = ctr
            .create_proposal(
                PropKind::DismissAndBan {
                    member: acc(1),
                    house: coa(),
                },
                "Motion to remove member and ban".to_string(),
            )
            .unwrap();

        ctx.block_timestamp = (START + MIN_VOTE_DURATION + 10) * MSECOND;
        ctr = vote(
            ctx.clone(),
            ctr,
            [acc(4), acc(2), acc(3)].to_vec(),
            motion_rem_ban,
        );
        let mut prop = ctr.get_proposal(motion_rem_ban).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        // Set timestamp to after cooldown
        ctx.block_timestamp =
            (prop.proposal.submission_time + ctr.vote_duration + ctr.cooldown + 1) * MSECOND;
        testing_env!(ctx);
        assert_exec_ok(ctr.execute(motion_rem_ban));

        prop = ctr.get_proposal(motion_rem_ban).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);

        // callback
        ctr.on_ban_dismiss(Ok(()), Ok(()), motion_rem_ban);
        prop = ctr.get_proposal(motion_rem_ban).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);

        ctr.on_ban_dismiss(Result::Err(PromiseError::Failed), Ok(()), motion_rem_ban);
        prop = ctr.get_proposal(motion_rem_ban).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Failed);

        ctr.on_ban_dismiss(Ok(()), Result::Err(PromiseError::Failed), motion_rem_ban);
        prop = ctr.get_proposal(motion_rem_ban).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Failed);
    }

    #[test]
    fn dismiss_ban_vote_against() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);
        let prop = ctr
            .create_proposal(
                PropKind::DismissAndBan {
                    member: acc(1),
                    house: coa(),
                },
                "Motion to remove member and ban".to_string(),
            )
            .unwrap();

        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx.clone());
        match ctr.vote(prop, Vote::Approve) {
            Err(VoteError::NoSelfVote) => (),
            x => panic!("expected NotAllowedAgainst, got: {:?}", x),
        }
    }

    #[test]
    fn dismiss_vote_against() {
        let (mut ctx, mut ctr, _) = setup_ctr(100);
        let prop = ctr
            .create_proposal(
                PropKind::FunctionCall {
                    receiver_id: coa(),
                    actions: [ActionCall {
                        method_name: "dismiss_hook".to_string(),
                        args: Base64VecU8(
                            json!({ "member": acc(2) }).to_string().as_bytes().to_vec(),
                        ),
                        deposit: U128(0),
                        gas: U64(EXEC_CTR_CALL_GAS.0),
                    }]
                    .to_vec(),
                },
                "Proposal to remove member".to_string(),
            )
            .unwrap();

        ctx.predecessor_account_id = acc(2);
        testing_env!(ctx.clone());
        assert_eq!(ctr.vote(prop, Vote::Approve), Err(VoteError::NoSelfVote));
    }

    #[test]
    fn abstain_vote() {
        let (_, mut ctr, id) = setup_ctr(100);
        ctr.vote(id, Vote::Abstain).unwrap();
        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.abstain, 1);
    }

    #[test]
    fn is_member() {
        let (_, ctr, _) = setup_ctr(100);
        assert!(ctr.is_member(acc(2)));
        assert!(!ctr.is_member(acc(10)))
    }

    #[test]
    fn vote_timestamp() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);

        ctx.predecessor_account_id = acc(1);
        ctx.block_timestamp = START * MSECOND;
        testing_env!(ctx.clone());
        ctr.vote(id, Vote::Approve).unwrap();
        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.votes.get(&acc(1)).unwrap().timestamp, START);

        ctx.predecessor_account_id = acc(2);
        ctx.block_timestamp = (START + 100) * MSECOND;
        testing_env!(ctx.clone());
        ctr.vote(id, Vote::Approve).unwrap();
        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(
            prop.proposal.votes.get(&acc(2)).unwrap().timestamp,
            START + 100
        );
    }

    #[test]
    fn min_vote_duration_execute() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);
        let p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::InProgress);

        // should not be able to exeucte a proposal while in min_vote_duration
        ctx.block_timestamp = (START + MIN_VOTE_DURATION - 10) * MSECOND;
        let p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::InProgress);
        testing_env!(ctx.clone());
        match ctr.execute(id) {
            Ok(_) => panic!("expecting Err"),
            Err(err) => assert_eq!(err, ExecError::MinVoteDuration),
        };

        ctx.block_timestamp = (START + MIN_VOTE_DURATION + 10) * MSECOND;
        testing_env!(ctx.clone());
        // proposal status should be reported correctly, however we need to wait for cooldow
        // to execute
        let p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::Approved);
        match ctr.execute(id) {
            Ok(_) => panic!("expecting Err"),
            Err(err) => assert_eq!(err, ExecError::ExecTime),
        };

        // cooldown starts when the proposal is "virtually approved" -> that is when it received
        // enough approve votes. In this test case, the propoosal was virtually approved
        // at START, so we should be able to execute proposal right after the cooldown.
        ctx.block_timestamp = (START + COOLDOWN + 1) * MSECOND;
        testing_env!(ctx.clone());
        let p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::Approved);
        assert_exec_ok(ctr.execute(id))
    }

    #[test]
    fn all_votes_casted() {
        let (ctx, mut ctr, id) = setup_ctr(100);
        let mut prop = ctr.get_proposal(id);
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::InProgress);

        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        prop = ctr.get_proposal(id);
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::InProgress);
        ctr = vote(ctx.clone(), ctr, [acc(4)].to_vec(), id);

        prop = ctr.get_proposal(id);
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::Approved);
    }

    #[test]
    fn members_len() {
        let (_, ctr, _) = setup_ctr(100);
        assert_eq!(ctr.members_len(), 4);
    }
}
