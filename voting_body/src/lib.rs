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
    /// Set of proposals in the pre-vote queue.
    pub pre_vote_proposals: LookupMap<u32, Proposal>,
    /// Set of active proposals.
    pub proposals: LookupMap<u32, Proposal>,

    /// Near amount required to create a proposal. Will be slashed if the proposal is marked as
    /// spam.
    pub pre_vote_bond: Balance,
    pub active_queue_bond: Balance,

    /// minimum amount of members to approve the proposal
    /// u32 can hold a number up to 4.2 B. That is enough for many future iterations.
    pub threshold: u32,

    /// all times below are in miliseconds
    pub voting_duration: u64,
    pub pre_vote_duration: u64,

    pub iah_registry: AccountId,
    /// Slashed bonds are send to the community treasury.
    pub community_treasury: AccountId,
}

#[near_bindgen]
impl Contract {
    #[init]
    /// * hook_auth : map of accounts authorized to call hooks
    pub fn new(
        pre_vote_duration: u64,
        voting_duration: u64,
        iah_registry: AccountId,
        community_treasury: AccountId,
        // TODO: make sure the threshold is calculated properly
        threshold: u32,
        pre_vote_bond: U128,
        active_queue_bond: U128,
    ) -> Self {
        Self {
            prop_counter: 0,
            pre_vote_proposals: LookupMap::new(StorageKey::PreVoteProposals),
            proposals: LookupMap::new(StorageKey::Proposals),
            pre_vote_duration,
            voting_duration,
            iah_registry,
            pre_vote_bond: pre_vote_bond.0,
            active_queue_bond: active_queue_bond.0,
            threshold, // TODO, need to add dynamic quorum and threshold
            community_treasury,
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
        let bond = env::attached_deposit();

        if bond < self.pre_vote_bond {
            return Err(CreatePropError::MinBond);
        }
        // TODO: check if proposal is created by a congress member. If yes, move it to active
        // immediately.
        let active = bond >= self.active_queue_bond;
        self.prop_counter += 1;
        emit_prop_created(self.prop_counter, &kind, active);
        let prop = Proposal {
            proposer: user.clone(),
            bond,
            additional_bond: None,
            description,
            kind,
            status: if active {
                ProposalStatus::InProgress
            } else {
                ProposalStatus::PreVote
            },
            approve: 0,
            reject: 0,
            abstain: 0,
            spam: 0,
            votes: HashMap::new(),
            start: now,
            approved_at: None,
        };
        if active {
            self.proposals.insert(&self.prop_counter, &prop);
        } else {
            self.pre_vote_proposals.insert(&self.prop_counter, &prop);
        }

        // TODO: this has to change, because we can have more votes
        // max amount of votes is threshold + threshold-1.
        let extra_storage = VOTE_STORAGE * (2 * self.threshold - 1) as u64;
        if let Err(reason) = finalize_storage_check(storage_start, extra_storage, user) {
            return Err(CreatePropError::Storage(reason));
        }

        Ok(self.prop_counter)
    }

    #[payable]
    #[handle_result]
    /// Allows to add more bond to a proposal to move it to the active queue. Anyone can top up.
    /// Returns true if the transaction succeeded, or false if the proposal is outdated and
    /// can't be top up any more.
    /// Emits:
    /// * proposal-prevote-slashed: when the prevote proposal is overdue and didn't get enough
    ///   support on time.
    /// * proposal-active: when a proposal was successfully updated.
    /// Excess of attached bond is sent back to the caller.
    /// Returns error when proposal is not in the pre-vote queue or not enough bond was attached.
    pub fn top_up_proposal(&mut self, id: u32) -> Result<bool, MovePropError> {
        let user = env::predecessor_account_id();
        let now = env::block_timestamp_ms();
        let mut bond = env::attached_deposit();
        let mut p = self.remove_pre_vote_prop(id)?;

        if now - p.start > self.pre_vote_duration {
            // transfer attached N, slash bond & keep the proposal removed.
            Promise::new(user.clone()).transfer(bond);
            Promise::new(self.community_treasury.clone()).transfer(p.bond);
            emit_prevote_prop_slashed(id, p.bond);
            return Ok(false);
        }

        let required_bond = self.active_queue_bond - p.bond;
        if bond < required_bond {
            return Err(MovePropError::MinBond);
        }
        let diff = bond - required_bond;
        if diff > 0 {
            Promise::new(user.clone()).transfer(diff);
            bond -= diff;
        }
        p.status = ProposalStatus::InProgress;
        p.additional_bond = Some((user, bond));
        p.start = now;
        self.proposals.insert(&id, &p);
        emit_prop_active(id);

        Ok(true)
    }

    #[handle_result]
    // TODO: must be called via iah_call
    pub fn vote(&mut self, id: u32, vote: Vote) -> Result<(), VoteError> {
        let user = env::predecessor_account_id();
        let mut prop = self.assert_proposal(id);

        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        if env::block_timestamp_ms() > prop.start + self.voting_duration {
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
        if now <= prop.start + self.voting_duration {
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

    fn remove_pre_vote_prop(&mut self, id: u32) -> Result<Proposal, MovePropError> {
        match self.pre_vote_proposals.remove(&id) {
            Some(p) => Ok(p),
            None => Err(MovePropError::NotFound),
        }
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

    const START: u64 = 60 * 5 * 1000 * MSECOND;
    // In milliseconds
    const VOTING_DURATION: u64 = 60 * 5 * 1000;
    const PRE_VOTE_DURATION: u64 = 60 * 10 * 1000;
    const PRE_BOND: u128 = ONE_NEAR * 3;
    const BOND: u128 = ONE_NEAR * 500;

    fn acc(idx: u8) -> AccountId {
        AccountId::new_unchecked(format!("user-{}.near", idx))
    }

    fn coa() -> AccountId {
        AccountId::new_unchecked("coa.near".to_string())
    }

    fn iah_registry() -> AccountId {
        AccountId::new_unchecked("registry.near".to_string())
    }

    fn treasury() -> AccountId {
        AccountId::new_unchecked("treasury.near".to_string())
    }

    /// creates a test contract with proposal threshold=3
    fn setup_ctr(attach_deposit: u128) -> (VMContext, Contract, u32) {
        let mut context = VMContextBuilder::new().build();
        let mut contract = Contract::new(
            PRE_VOTE_DURATION,
            VOTING_DURATION,
            iah_registry(),
            treasury(),
            3,
            U128(PRE_BOND),
            U128(BOND),
        );
        context.block_timestamp = START;
        context.predecessor_account_id = acc(1);
        context.attached_deposit = attach_deposit;
        testing_env!(context.clone());

        let id = contract
            .create_proposal(PropKind::Text, "Proposal unit test 1".to_string())
            .unwrap();
        (context, contract, id)
    }

    fn vote(mut ctx: VMContext, ctr: &mut Contract, accs: Vec<AccountId>, id: u32, vote: Vote) {
        for a in accs {
            ctx.predecessor_account_id = a.clone();
            testing_env!(ctx.clone());
            let res = ctr.vote(id, vote.clone());
            assert_eq!(
                res,
                Ok(()),
                "\nacc {} _____ prop: {:?}",
                a,
                ctr.proposals.get(&id).unwrap(),
            );
        }
    }

    #[test]
    fn basic_flow() {
        let (mut ctx, mut ctr, id) = setup_ctr(PRE_BOND);
        let mut prop1 = ctr.get_proposal(id).unwrap();
        assert_eq!(prop1.proposal.status, ProposalStatus::PreVote);
        assert_eq!(ctr.number_of_proposals(), 1);
        assert_eq!(
            ctr.get_proposals(0, 10, None),
            vec![],
            "should only return active proposals"
        );
        //
        // move proposal to an active queue and vote
        //
        ctx.attached_deposit = BOND;
        ctx.block_timestamp += MSECOND;
        ctx.predecessor_account_id = acc(2);
        testing_env!(ctx.clone());
        assert_eq!(Ok(true), ctr.top_up_proposal(id));
        // update the prop1 to the expected vaules
        prop1.proposal.status = ProposalStatus::InProgress;
        prop1.proposal.start = (START + MSECOND) / MSECOND;
        prop1.proposal.additional_bond = Some((acc(2), BOND - PRE_BOND));
        assert_eq!(ctr.get_proposals(0, 10, None), vec![prop1.clone()]);

        ctx.attached_deposit = 0;
        testing_env!(ctx.clone());
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );

        prop1 = ctr.get_proposal(id).unwrap();
        assert_eq!(prop1.proposal.status, ProposalStatus::Approved);

        // Proposal already got enough votes - it's approved
        ctx.predecessor_account_id = acc(5);
        testing_env!(ctx.clone());
        assert_eq!(ctr.vote(id, Vote::Approve), Err(VoteError::NotInProgress));

        //
        // Create a new proposal, not enough bond
        //
        let resp = ctr.create_proposal(PropKind::Text, "proposal".to_owned());
        assert_eq!(resp, Err(CreatePropError::MinBond));

        //
        // Create a new proposal with bond to active queue and check double vote and expire
        // Check all votes
        //
        ctx.attached_deposit = BOND;
        ctx.predecessor_account_id = acc(3);
        testing_env!(ctx.clone());
        let id = ctr
            .create_proposal(PropKind::Text, "proposal".to_owned())
            .unwrap();
        let mut prop2 = ctr.get_proposal(id).unwrap();
        assert_eq!(ctr.vote(id, Vote::Approve), Ok(()));
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2)],
            id,
            Vote::Reject,
        );

        prop2.proposal.approve = 1;
        prop2.proposal.reject = 2;
        prop2.proposal.votes.insert(acc(3), Vote::Approve);
        prop2.proposal.votes.insert(acc(1), Vote::Reject);
        prop2.proposal.votes.insert(acc(2), Vote::Reject);

        assert_eq!(ctr.get_proposals(0, 1, None), vec![prop1.clone()]);
        assert_eq!(
            ctr.get_proposals(0, 10, None),
            vec![prop1.clone(), prop2.clone()]
        );
        assert_eq!(
            ctr.get_proposals(1, 10, None),
            vec![prop1.clone(), prop2.clone()]
        );
        assert_eq!(ctr.get_proposals(2, 10, None), vec![prop2.clone()]);
        assert_eq!(ctr.get_proposals(3, 10, None), vec![]);

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

        //
        // create proposal, set timestamp past voting period, status should be rejected
        //
        let id = ctr
            .create_proposal(PropKind::Text, "Proposal unit test query 3".to_string())
            .unwrap();

        let prop = ctr.get_proposal(id).unwrap();
        ctx.block_timestamp = (prop.proposal.start + ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Rejected);
    }

    #[test]
    fn proposal_overdue() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        ctx.block_timestamp = START + (ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        assert_eq!(ctr.vote(id, Vote::Approve), Err(VoteError::NotActive));
    }

    #[test]
    #[should_panic(expected = "proposal does not exist")]
    fn proposal_does_not_exist() {
        let (_, mut ctr, _) = setup_ctr(BOND);
        ctr.vote(10, Vote::Approve).unwrap();
    }

    #[test]
    fn proposal_execution_text() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        match ctr.execute(id) {
            Err(ExecError::NotApproved) => (),
            Ok(_) => panic!("expected NotApproved, got: OK"),
            Err(err) => panic!("expected NotApproved got: {:?}", err),
        }
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );

        let mut prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Approved);

        match ctr.execute(id) {
            Err(ExecError::ExecTime) => (),
            Ok(_) => panic!("expected ExecTime, got: OK"),
            Err(err) => panic!("expected ExecTime got: {:?}", err),
        }

        ctx.block_timestamp = START + (ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        ctr.execute(id).unwrap();

        prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);
    }

    #[test]
    fn overwrite_votes() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        let mut p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::InProgress);
        assert!((p.proposal.votes.is_empty()));

        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx.clone());

        assert_eq!(ctr.vote(id, Vote::Approve), Ok(()));
        p.proposal.approve = 1;
        p.proposal.votes.insert(acc(1), Vote::Approve);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(ctr.vote(id, Vote::Abstain), Ok(()));
        p.proposal.approve = 0;
        p.proposal.abstain = 1;
        p.proposal.votes.insert(acc(1), Vote::Abstain);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(ctr.vote(id, Vote::Reject), Ok(()));
        p.proposal.abstain = 0;
        p.proposal.reject = 1;
        p.proposal.votes.insert(acc(1), Vote::Reject);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(ctr.vote(id, Vote::Spam), Ok(()));
        p.proposal.reject = 0;
        p.proposal.spam = 1;
        p.proposal.votes.insert(acc(1), Vote::Spam);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);
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

    #[test]
    fn get_proposals() {
        let (_, mut ctr, id1) = setup_ctr(100);
        let (id2, id3) = create_all_props(&mut ctr);
        let prop1 = ctr.get_proposal(id1).unwrap();
        let prop2 = ctr.get_proposal(id2).unwrap();
        let prop3 = ctr.get_proposal(id3).unwrap();
        assert_eq!(ctr.number_of_proposals(), 3);
        // non reversed
        assert_eq!(
            ctr.get_proposals(0, 10, None),
            vec![prop1.clone(), prop2.clone(), prop3.clone()]
        );
        // non reversed with litmit
        assert_eq!(
            ctr.get_proposals(0, 2, None),
            vec![prop1.clone(), prop2.clone()]
        );
        // reversed
        assert_eq!(
            ctr.get_proposals(3, 10, Some(true)),
            vec![prop3.clone(), prop2.clone(), prop1.clone()]
        );
        // reversed with limit
        assert_eq!(
            ctr.get_proposals(3, 2, Some(true)),
            vec![prop3.clone(), prop2.clone()]
        );
    }
}
