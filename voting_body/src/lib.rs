use std::collections::HashMap;

use common::finalize_storage_check;
use events::*;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
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
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,

    /// Near amount required to create a proposal. Will be slashed if the proposal is marked as
    /// spam.
    pub bond: Balance,
    /// minimum amount of members to approve the proposal
    /// u32 can hold a number up to 4.2 B. That is enough for many future iterations.
    pub threshold: u32,

    /// all times below are in miliseconds
    pub end_time: u64,
    pub voting_duration: u64,

    pub iah_registry: AccountId,
}

#[near_bindgen]
impl Contract {
    #[init]
    /// * hook_auth : map of accounts authorized to call hooks
    pub fn new(
        end_time: u64,
        voting_duration: u64,
        iah_registry: AccountId,
        // TODO: make sure the threshold is calculated properly
        threshold: u32,
        bond: U128,
    ) -> Self {
        Self {
            prop_counter: 0,
            proposals: LookupMap::new(StorageKey::Proposals),
            end_time,
            voting_duration,
            iah_registry,
            bond: bond.0,
            threshold, // TODO, need to add dynamic quorum and threshold
        }
    }

    /*
     * Queries are in view.rs
     */

    /**********
     * TRANSACTIONS
     **********/

    /// Creates a new proposal.
    /// Returns the new proposal ID.
    /// Caller is required to attach enough deposit to cover the proposal storage as well as all
    /// possible votes (2*self.threshold - 1).
    /// NOTE: storage is paid from the bond.
    #[payable]
    #[handle_result]
    // TODO: bond, and deduce storage cost from the bond.
    // TODO: must be called via iah_call
    pub fn create_proposal(
        &mut self,
        kind: PropKind,
        description: String,
    ) -> Result<u32, CreatePropError> {
        let storage_start = env::storage_usage();
        let user = env::predecessor_account_id();

        let now = env::block_timestamp_ms();

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
        let user = env::predecessor_account_id();
        let mut prop = self.assert_proposal(id);

        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        if env::block_timestamp_ms() > prop.submission_time + self.voting_duration {
            return Err(VoteError::NotActive);
        }

        prop.add_vote(user, vote, self.threshold)?;

        if prop.status == ProposalStatus::Spam {
            self.proposals.remove(&id);
            emit_spam(id);
            // TODO: slash bonds
            return Ok(());
        } else {
            self.proposals.insert(&id, &prop);
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
    #[handle_result]
    pub fn execute(&mut self, id: u32) -> Result<PromiseOrValue<()>, ExecError> {
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
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests {
    use near_sdk::{test_utils::VMContextBuilder, testing_env, VMContext, ONE_NEAR};

    use crate::*;

    /// 1ms in nano seconds
    const MSECOND: u64 = 1_000_000;

    // In milliseconds
    const START: u64 = 60 * 5 * 1000;
    const TERM: u64 = 60 * 15 * 1000;
    const VOTING_DURATION: u64 = 60 * 5 * 1000;
    const BOND: u128 = ONE_NEAR * 10;

    fn acc(idx: u8) -> AccountId {
        AccountId::new_unchecked(format!("user-{}.near", idx))
    }

    fn coa() -> AccountId {
        AccountId::new_unchecked("coa.near".to_string())
    }

    fn iah_registry() -> AccountId {
        AccountId::new_unchecked("registry.near".to_string())
    }

    /// creates a test contract with proposal threshold=3
    fn setup_ctr(attach_deposit: u128) -> (VMContext, Contract, u32) {
        let mut context = VMContextBuilder::new().build();
        let end_time = START + TERM;

        let mut contract = Contract::new(end_time, VOTING_DURATION, iah_registry(), 3, U128(BOND));
        context.block_timestamp = START * MSECOND;
        context.predecessor_account_id = acc(1);
        context.attached_deposit = attach_deposit * MILI_NEAR;
        testing_env!(context.clone());

        let id = contract
            .create_proposal(PropKind::Text, "Proposal unit test 1".to_string())
            .unwrap();
        (context, contract, id)
    }

    fn vote(mut ctx: VMContext, ctr: &mut Contract, accounts: Vec<AccountId>, id: u32) {
        for account in accounts {
            ctx.predecessor_account_id = account;
            testing_env!(ctx.clone());
            let res = ctr.vote(id, Vote::Approve);
            assert_eq!(res, Ok(()));
        }
    }

    #[test]
    fn basic_flow() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        let mut prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::InProgress);

        assert_eq!(ctr.number_of_proposals(), 1);

        // check `get_proposals` query
        assert_eq!(ctr.get_proposals(1, 10), vec![prop.clone()]);
        assert_eq!(ctr.get_proposals(0, 10), vec![prop.clone()]);
        assert_eq!(ctr.get_proposals(2, 10), vec![]);

        vote(ctx.clone(), &mut ctr, vec![acc(1), acc(2), acc(3)], id);

        prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        ctx.predecessor_account_id = acc(4);
        testing_env!(ctx.clone());
        match ctr.vote(id, Vote::Approve) {
            Err(VoteError::NotInProgress) => (),
            x => panic!("expected NotInProgress, got: {:?}", x),
        }

        //
        // Create a new proposal
        let id = ctr
            .create_proposal(PropKind::Text, "proposal".to_owned())
            .unwrap();

        assert_eq!(ctr.vote(id, Vote::Approve), Ok(()));

        match ctr.vote(id, Vote::Approve) {
            Err(VoteError::DoubleVote) => (),
            x => panic!("expected DoubleVoted, got: {:?}", x),
        }

        ctx.block_timestamp = (START + ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        match ctr.vote(id, Vote::Approve) {
            Err(VoteError::NotActive) => (),
            x => panic!("expected NotActive, got: {:?}", x),
        }

        // TODO: add a test case for checking not authorized (but firstly we need to implement that)
        // ctx.predecessor_account_id = acc(5);
        // testing_env!(ctx.clone());
        // match ctr.vote(id, Vote::Approve) {
        //     Err(VoteError::NotAuthorized) => (),
        //     x => panic!("expected NotAuthorized, got: {:?}", x),
        // }

        // TODO: test case checking automatic execution
        // ctx.predecessor_account_id = acc(2);
        // testing_env!(ctx.clone());
        // let id = ctr
        //     .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
        //     .unwrap();
        // vote(ctx, &mut ctr, vec![acc(1), acc(2), acc(3)], id);
        // let prop = ctr.get_proposal(id).unwrap();
        // assert_eq!(prop.proposal.status, ProposalStatus::Executed);
    }

    #[test]
    #[should_panic(expected = "proposal does not exist")]
    fn proposal_does_not_exist() {
        let (_, mut ctr, _) = setup_ctr(100);
        ctr.vote(10, Vote::Approve).unwrap();
    }

    #[test]
    fn proposal_execution_text() {
        let (mut ctx, mut ctr, id) = setup_ctr(100);
        match ctr.execute(id) {
            Err(ExecError::NotApproved) => (),
            Ok(_) => panic!("expected NotApproved, got: OK"),
            Err(err) => panic!("expected NotApproved got: {:?}", err),
        }
        vote(ctx.clone(), &mut ctr, [acc(1), acc(2), acc(3)].to_vec(), id);

        let mut prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        match ctr.execute(id) {
            Err(ExecError::ExecTime) => (),
            Ok(_) => panic!("expected ExecTime, got: OK"),
            Err(err) => panic!("expected ExecTime got: {:?}", err),
        }

        ctx.block_timestamp = (START + ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        ctr.execute(id).unwrap();

        prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);
    }

    fn create_all_props(ctr: &mut Contract) -> (u32, u32) {
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

        (prop_text, prop_fc)
    }
}
