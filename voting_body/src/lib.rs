use std::collections::HashMap;

use common::finalize_storage_check;
use events::*;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, near_bindgen, AccountId, Balance, Gas, PanicOnDefault, Promise, PromiseError,
    PromiseOrValue, PromiseResult,
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
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,

    /// Near amount required to create a proposal. Will be slashed if the proposal is marked as
    /// spam.
    pub bond: Balance,
    /// minimum amount of members to approve the proposal
    pub threshold: u64,

    /// all times below are in miliseconds
    pub end_time: u64,
    pub voting_duration: u64,

    /// address of the community fund, where the excess of NEAR will be sent on dissolve and cleanup.
    pub community_fund: AccountId,
    pub iah_registry: AccountId,
}

#[near_bindgen]
impl Contract {
    #[init]
    /// * hook_auth : map of accounts authorized to call hooks
    pub fn new(
        end_time: u64,
        cooldown: u64,
        voting_duration: u64,
        community_fund: AccountId,
        iah_registry: AccountId,
        bond: U128,
    ) -> Self {
        Self {
            prop_counter: 0,
            proposals: LookupMap::new(StorageKey::Proposals),
            end_time,
            voting_duration,
            community_fund,
            iah_registry,
            bond: bond.0,
            threshold: 1, // TODO, need to add dynamic quorum and threshold
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
    // TODO: bond
    // TODO: must be called via iah_call
    pub fn create_proposal(
        &mut self,
        kind: PropKind,
        description: String,
    ) -> Result<u32, CreatePropError> {
        self.assert_active();
        let storage_start = env::storage_usage();
        let user = env::predecessor_account_id();

        let now = env::block_timestamp_ms();
        let mut new_budget = 0;

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
                spam: 0,
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
    // TODO: must be called via iah_call
    pub fn vote(&mut self, id: u32, vote: Vote) -> Result<(), VoteError> {
        self.assert_active();
        let user = env::predecessor_account_id();
        let mut prop = self.assert_proposal(id);

        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        if env::block_timestamp_ms() > prop.submission_time + self.voting_duration {
            return Err(VoteError::NotActive);
        }

        prop.add_vote(user, vote, self.threshold)?;
        self.proposals.insert(&id, &prop);

        if prop.status == ProposalStatus::Spam {
            self.proposals.remove(&id);
            emit_spam(id);
            // TODO: slash bonds
            return Ok(());
        }

        emit_vote(id);

        if prop.status == ProposalStatus::Approved {
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
        if now <= prop.submission_time + self.voting_duration {
            return Err(ExecError::ExecTime);
        }

        prop.status = ProposalStatus::Executed;
        let mut result = PromiseOrValue::Value(());
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
            PropKind::Text => (),
        };
        self.proposals.insert(&id, &prop);

        let result = match result {
            PromiseOrValue::Promise(promise) => promise
                .then(
                    ext_self::ext(env::current_account_id())
                        .with_static_gas(EXECUTE_CALLBACK_GAS)
                        .on_execute(id),
                )
                .into(),
            _ => {
                emit_executed(id);
                result
            }
        };
        Ok(result)
    }

    /*****************
     * INTERNAL
     ****************/

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

    #[private]
    pub fn on_execute(&mut self, prop_id: u32) {
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
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests {
    use near_sdk::{
        test_utils::{get_logs, VMContextBuilder},
        testing_env, VMContext,
    };

    use crate::{view::MembersOutput, *};

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
            COOLDOWN_DURATION,
            VOTING_DURATION,
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
            let res = ctr.vote(id, Vote::Approve);
            assert!(res.is_ok());
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

    #[test]
    fn basic_flow() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        let mut prop = ctr.get_proposal(id);
        assert!(prop.is_some());
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::InProgress);

        assert_eq!(ctr.number_of_proposals(), 1);

        // check `get_proposals` query
        let res = ctr.get_proposals(0, 10);
        assert_eq!(res, vec![ctr.get_proposal(id).unwrap()]);
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        prop = ctr.get_proposal(id);
        assert!(prop.is_some());
        assert_eq!(prop.unwrap().proposal.status, ProposalStatus::Approved);

        ctx.predecessor_account_id = acc(4);
        testing_env!(ctx.clone());
        match ctr.vote(id, Vote::Approve) {
            Err(VoteError::NotInProgress) => (),
            x => panic!("expected NotInProgress, got: {:?}", x),
        }
        //let (mut ctx, mut contract, id) = setup_ctr(100);
        let id = ctr
            .create_proposal(PropKind::Text, "proposal".to_owned())
            .unwrap();

        let res = ctr.vote(id, Vote::Approve);
        assert!(res.is_ok());

        match ctr.vote(id, Vote::Approve) {
            Err(VoteError::DoubleVote) => (),
            x => panic!("expected DoubleVoted, got: {:?}", x),
        }

        ctx.block_timestamp = (ctr.start_time + ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        match ctr.vote(id, Vote::Approve) {
            Err(VoteError::NotActive) => (),
            x => panic!("expected NotActive, got: {:?}", x),
        }

        ctx.predecessor_account_id = acc(5);
        testing_env!(ctx.clone());
        match ctr.vote(id, Vote::Approve) {
            Err(VoteError::NotAuthorized) => (),
            x => panic!("expected NotAuthorized, got: {:?}", x),
        }

        ctx.predecessor_account_id = acc(2);
        testing_env!(ctx.clone());
        // set cooldown=0 and test for immediate execution
        ctr.cooldown = 0;
        let id = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();
        ctr = vote(ctx, ctr, [acc(1), acc(2), acc(3)].to_vec(), id);
        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);
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

        ctr.create_proposal(PropKind::FundingRequest(10), "".to_string())
            .unwrap();

        // creating other proposal kinds should fail
        assert_create_prop_not_allowed(
            ctr.create_proposal(PropKind::RecurrentFundingRequest(10), "".to_string()),
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

        match ctr.create_proposal(PropKind::FundingRequest(1), "".to_string()) {
            Err(CreatePropError::Storage(_)) => (),
            Ok(_) => panic!("expected Storage, got: OK"),
            Err(err) => panic!("expected Storage got: {:?}", err),
        }

        ctx.predecessor_account_id = acc(6);
        ctx.attached_deposit = 10 * MILI_NEAR;
        testing_env!(ctx.clone());
        match ctr.create_proposal(PropKind::Text, "".to_string()) {
            Err(CreatePropError::NotAuthorized) => (),
            Ok(_) => panic!("expected NotAuthorized, got: OK"),
            Err(err) => panic!("expected NotAuthorized got: {:?}", err),
        }

        // set remaining months to 2
        let (members, _) = ctr.members.get().unwrap();
        ctr.members
            .set(&(members, vec![PropPerm::RecurrentFundingRequest]));
        ctr.end_time = ctr.start_time + START * 12 * 24 * 61;
        ctx.predecessor_account_id = acc(2);
        testing_env!(ctx);

        match ctr.create_proposal(
            PropKind::RecurrentFundingRequest((ctr.budget_cap / 2) + 1),
            "".to_string(),
        ) {
            Err(CreatePropError::BudgetOverflow) => (),
            Ok(_) => panic!("expected BudgetOverflow, got: OK"),
            Err(err) => panic!("expected BudgetOverflow got: {:?}", err),
        }
    }

    #[test]
    fn proposal_execution_text() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        match ctr.execute(id) {
            Err(ExecError::NotApproved) => (),
            Ok(_) => panic!("expected NotApproved, got: OK"),
            Err(err) => panic!("expected NotApproved got: {:?}", err),
        }
        ctr = vote(ctx.clone(), ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        let mut prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        match ctr.execute(id) {
            Err(ExecError::ExecTime) => (),
            Ok(_) => panic!("expected ExecTime, got: OK"),
            Err(err) => panic!("expected ExecTime got: {:?}", err),
        }

        ctx.block_timestamp = (ctr.start_time + ctr.cooldown + ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        ctr.execute(id).unwrap();

        prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);
    }

    #[test]
    #[should_panic(expected = "dao term is over, call dissolve_hook!")]
    fn dao_dissolve_time() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        ctx.block_timestamp = (ctr.end_time + 1) * MSECOND;
        testing_env!(ctx);

        ctr.vote(id, Vote::Approve).unwrap();
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
                PropKind::FundingRequest(1100),
                "big funding request".to_string(),
            )
            .unwrap();
        let prop_small = ctr
            .create_proposal(
                PropKind::FundingRequest(200),
                "small funding request".to_string(),
            )
            .unwrap();
        let prop_rec = ctr
            .create_proposal(
                PropKind::RecurrentFundingRequest(200),
                "recurrent funding request".to_string(),
            )
            .unwrap();

        (prop_text, prop_fc, prop_big, prop_small, prop_rec)
    }
}
