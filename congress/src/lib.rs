use std::cmp::min;
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

        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        if env::block_timestamp_ms() > prop.submission_time + self.voting_duration {
            return Err(VoteError::NotActive);
        }
        prop.add_vote(user, vote, self.threshold)?;
        self.proposals.insert(&id, &prop);
        emit_vote(id);

        if prop.status == ProposalStatus::Approved && self.cooldown == 0 {
            // We ignore a failure of self.execute here to assure that the vote is counted.
            let res = self.execute(id);
            if res.is_err() {
                emit_vote_execute(id, res.err().unwrap());
            }
        }

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
            _ => {
                emit_executed(id);
                result
            }
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
        let mut proposal = self.assert_proposal(id);

        match proposal.status {
            ProposalStatus::InProgress => {
                proposal.status = ProposalStatus::Vetoed;
            }
            ProposalStatus::Approved => {
                let cooldown = min(
                    proposal.submission_time + self.voting_duration,
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
        test_utils::{get_logs, VMContextBuilder},
        testing_env, VMContext,
    };

    use crate::*;

    /// 1ms in nano seconds
    const MSECOND: u64 = 1_000_000;

    // In milliseconds
    const START: u64 = 60 * 5 * 1000;
    const TERM: u64 = 60 * 15 * 1000;
    const VOTING_DURATION: u64 = 60 * 5 * 1000;
    const COOLDOWN_DURATION: u64 = 60 * 5 * 1000;

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

    fn setup_ctr(attach_deposit: u128) -> (VMContext, Contract, u32) {
        let mut context = VMContextBuilder::new().build();
        let start_time = START;
        let end_time = START + TERM;

        let mut hash_map = HashMap::new();
        hash_map.insert(coa(), vec![HookPerm::Veto]);
        hash_map.insert(voting_body(), vec![HookPerm::Dismiss, HookPerm::Dissolve]);

        let mut contract = Contract::new(
            community_fund(),
            start_time,
            end_time,
            COOLDOWN_DURATION,
            VOTING_DURATION,
            vec![acc(1), acc(2), acc(3), acc(4)],
            vec![
                PropPerm::Text,
                PropPerm::RecurrentFundingRequest,
                PropPerm::FundingRequest,
            ],
            hash_map,
            U128(10000),
            U128(100000),
        );
        context.block_timestamp = start_time * MSECOND;
        context.predecessor_account_id = acc(1);
        context.attached_deposit = attach_deposit * MILI_NEAR;
        testing_env!(context.clone());

        let id = contract
            .create_proposal(PropKind::Text, "Proposal unit test 1".to_string())
            .unwrap();
        (context, contract, id)
    }

    fn vote(
        mut ctx: VMContext,
        mut contract: Contract,
        accounts: Vec<AccountId>,
        id: u32,
    ) -> Contract {
        for account in accounts {
            ctx.predecessor_account_id = account;
            testing_env!(ctx.clone());
            let res = contract.vote(id, Vote::Approve);
            assert!(res.is_ok());
        }
        contract
    }

    #[test]
    fn basic_flow() {
        let (mut ctx, mut contract, id) = setup_ctr(100);
        let mut prop = contract.get_proposal(id);
        assert!(prop.is_some());
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::InProgress);

        // check `get_proposals` query
        let res = contract.get_proposals(0, 10);
        assert_eq!(res, vec![contract.get_proposal(id).unwrap()]);

        contract = vote(ctx.clone(), contract, [acc(1), acc(2), acc(3)].to_vec(), id);

        prop = contract.get_proposal(id);
        assert!(prop.is_some());
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::Approved);

        ctx.predecessor_account_id = acc(4);
        testing_env!(ctx.clone());
        match contract.vote(id, Vote::Approve) {
            Err(VoteError::NotInProgress) => (),
            x => panic!("expected NotInProgress, got: {:?}", x),
        }
        //let (mut ctx, mut contract, id) = setup_ctr(100);
        let id = contract
            .create_proposal(PropKind::Text, "proposal".to_owned())
            .unwrap();

        let res = contract.vote(id, Vote::Approve);
        assert!(res.is_ok());

        match contract.vote(id, Vote::Approve) {
            Err(VoteError::DoubleVote) => (),
            x => panic!("expected DoubleVoted, got: {:?}", x),
        }

        ctx.block_timestamp = (contract.start_time + contract.voting_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        match contract.vote(id, Vote::Approve) {
            Err(VoteError::NotActive) => (),
            x => panic!("expected NotActive, got: {:?}", x),
        }

        ctx.predecessor_account_id = acc(5);
        testing_env!(ctx.clone());
        match contract.vote(id, Vote::Approve) {
            Err(VoteError::NotAuthorized) => (),
            x => panic!("expected NotAuthorized, got: {:?}", x),
        }

        ctx.predecessor_account_id = acc(2);
        testing_env!(ctx.clone());
        // set cooldown=0 and test for immediate execution
        contract.cooldown = 0;
        let id = contract
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();
        contract = vote(ctx, contract, [acc(1), acc(2), acc(3)].to_vec(), id);
        let prop = contract.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);
    }

    #[test]
    fn proposal_execution_text() {
        let (mut ctx, mut contract, id) = setup_ctr(100);
        match contract.execute(id) {
            Err(ExecError::NotApproved) => (),
            Ok(_) => panic!("expected NotApproved, got: OK"),
            Err(err) => panic!("expected NotApproved got: {:?}", err),
        }
        contract = vote(ctx.clone(), contract, [acc(1), acc(2), acc(3)].to_vec(), id);

        let mut prop = contract.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        match contract.execute(id) {
            Err(ExecError::ExecTime) => (),
            Ok(_) => panic!("expected ExecTime, got: OK"),
            Err(err) => panic!("expected ExecTime got: {:?}", err),
        }

        ctx.block_timestamp =
            (contract.start_time + contract.cooldown + contract.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        match contract.execute(id) {
            Ok(_) => (),
            Err(x) => panic!("expected OK, got: {:?}", x),
        }

        prop = contract.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);
    }

    #[test]
    fn proposal_execution_funding_req() {
        let (mut ctx, mut contract, _) = setup_ctr(100);

        let id = contract
            .create_proposal(PropKind::FundingRequest(1000u128), "Funding req".to_owned())
            .unwrap();
        contract = vote(ctx.clone(), contract, [acc(1), acc(2), acc(3)].to_vec(), id);

        ctx.block_timestamp =
            (contract.start_time + contract.cooldown + contract.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        assert_eq!(contract.budget_spent, 0);
        match contract.execute(id) {
            Ok(_) => (),
            Err(x) => panic!("expected OK, got: {:?}", x),
        }
        assert_eq!(contract.budget_spent, 1000);

        let res = contract.create_proposal(
            PropKind::FundingRequest(10000u128),
            "Funding req".to_owned(),
        );
        match res {
            Err(CreatePropError::BudgetOverflow) => (),
            Ok(_) => panic!("expected BudgetOverflow, got: OK"),
            Err(x) => panic!("expected BudgetOverflow, got: {:?}", x),
        }
    }

    #[test]
    fn proposal_execution_rec_funding_req() {
        let (mut ctx, mut contract, _) = setup_ctr(100);

        let id = contract
            .create_proposal(
                PropKind::RecurrentFundingRequest(10u128),
                "Rec Funding req".to_owned(),
            )
            .unwrap();
        contract = vote(ctx.clone(), contract, [acc(1), acc(2), acc(3)].to_vec(), id);

        // update to more than two months
        contract.end_time = contract.start_time + START * 12 * 24 * 61;
        ctx.block_timestamp =
            (contract.start_time + contract.cooldown + contract.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        // proposal isn't executed so budget spent is 0
        assert_eq!(contract.budget_spent, 0);

        match contract.execute(id) {
            Ok(_) => (),
            Err(x) => panic!("expected OK, got: {:?}", x),
        }

        // budget spent * remaining months
        assert_eq!(contract.budget_spent, 20);
    }

    #[test]
    #[should_panic(expected = "dao term is over, call dissolve_hook!")]
    fn dao_dissolve_time() {
        let (mut ctx, mut contract, id) = setup_ctr(100);
        ctx.block_timestamp = (contract.end_time + 1) * MSECOND;
        testing_env!(ctx);

        contract.vote(id, Vote::Approve).unwrap();
    }

    #[test]
    fn veto_hook() {
        let (mut ctx, mut contract, id) = setup_ctr(100);
        contract.get_proposal(id).unwrap();
        match contract.veto_hook(id) {
            Err(HookError::NotAuthorized) => (),
            x => panic!("expected NotAuthorized, got: {:?}", x),
        }

        ctx.predecessor_account_id = coa();
        testing_env!(ctx.clone());

        // Veto during voting phase(before cooldown)
        match contract.veto_hook(id) {
            Ok(_) => (),
            x => panic!("expected Ok, got: {:?}", x),
        }
        let expected = r#"EVENT_JSON:{"standard":"ndc-congress","version":"1.0.0","event":"veto","data":{"prop_id":1}}"#;
        assert_eq!(vec![expected], get_logs());

        let mut prop = contract.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Vetoed);

        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx.clone());

        // Veto during cooldown
        let id = contract
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();

        // Set timestamp close to voting end duration
        ctx.block_timestamp =
            (prop.proposal.submission_time + contract.voting_duration - 1) * MSECOND;
        testing_env!(ctx.clone());

        contract = vote(ctx.clone(), contract, [acc(1), acc(2), acc(3)].to_vec(), id);
        prop = contract.get_proposal(id).unwrap();

        // Set timestamp to during cooldown, after voting phase
        ctx.block_timestamp =
            (prop.proposal.submission_time + contract.voting_duration + 1) * MSECOND;
        ctx.predecessor_account_id = coa();
        testing_env!(ctx.clone());

        match contract.veto_hook(id) {
            Ok(_) => (),
            x => panic!("expected Ok, got: {:?}", x),
        }

        ctx.block_timestamp = contract.start_time;
        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx.clone());

        let id = contract
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();
        contract = vote(ctx.clone(), contract, [acc(1), acc(2), acc(3)].to_vec(), id);

        prop = contract.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        // Set timestamp to after cooldown
        ctx.block_timestamp =
            (prop.proposal.submission_time + contract.voting_duration + contract.cooldown + 1)
                * MSECOND;
        ctx.predecessor_account_id = coa();
        testing_env!(ctx);

        // Can execute past cooldown but not veto proposal
        match contract.veto_hook(id) {
            Err(HookError::CooldownOver) => (),
            x => panic!("expected CooldownOver, got: {:?}", x),
        }

        match contract.execute(id) {
            Ok(_) => (),
            Err(x) => panic!("expected OK, got: {:?}", x),
        }
    }

    #[test]
    #[should_panic(expected = "dao is dissolved")]
    fn dissolve_hook() {
        let (mut ctx, mut contract, _) = setup_ctr(100);

        match contract.dissolve_hook() {
            Err(HookError::NotAuthorized) => (),
            x => panic!("expected NotAuthorized, got: {:?}", x),
        }

        ctx.predecessor_account_id = voting_body();
        testing_env!(ctx);

        match contract.dissolve_hook() {
            Ok(_) => (),
            x => panic!("expected Ok, got: {:?}", x),
        }
        let expected = r#"EVENT_JSON:{"standard":"ndc-congress","version":"1.0.0","event":"dissolve","data":""}"#;
        assert_eq!(vec![expected], get_logs());

        assert!(contract.dissolved);

        contract
            .create_proposal(
                PropKind::FundingRequest(10000u128),
                "Funding req".to_owned(),
            )
            .unwrap();
    }

    #[test]
    fn dismiss_hook() {
        let (mut ctx, mut contract, _) = setup_ctr(100);

        match contract.dismiss_hook(acc(2)) {
            Err(HookError::NotAuthorized) => (),
            x => panic!("expected NotAuthorized, got: {:?}", x),
        }

        ctx.predecessor_account_id = voting_body();
        testing_env!(ctx);

        match contract.dismiss_hook(acc(2)) {
            Ok(_) => (),
            x => panic!("expected Ok, got: {:?}", x),
        }
        let expected = r#"EVENT_JSON:{"standard":"ndc-congress","version":"1.0.0","event":"dismiss","data":{"member":"user-2.near"}}"#;
        assert_eq!(vec![expected], get_logs());

        assert_eq!(contract.member_permissions(acc(2)), vec![]);

        assert!(!contract.dissolved);
        // Remove more members to check dissolve
        match contract.dismiss_hook(acc(1)) {
            Ok(_) => (),
            x => panic!("expected Ok, got: {:?}", x),
        }

        assert!(contract.dissolved);
    }
}
