use std::collections::{HashMap, HashSet};

use common::finalize_storage_check;
use events::*;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap};
use near_sdk::json_types::U128;
use near_sdk::{
    env, near_bindgen, require, Balance, PanicOnDefault, Promise, PromiseOrValue, PromiseResult,
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

// TODO temp value
const THRESHOLD: u32 = 3;

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
    /// amount of users that need to support a proposal to move it to the active queue;
    pub pre_vote_support: u32,

    /// minimum amount of members to approve the proposal
    /// u32 can hold a number up to 4.2 B. That is enough for many future iterations.
    pub simple_consent: Consent,
    pub super_consent: Consent,

    /// all times below are in miliseconds
    pub voting_duration: u64,
    pub pre_vote_duration: u64,
    pub accounts: LazyOption<Accounts>,
}

#[near_bindgen]
impl Contract {
    #[init]
    /// * hook_auth : map of accounts authorized to call hooks
    pub fn new(
        pre_vote_duration: u64,
        voting_duration: u64,
        pre_vote_support: u32,
        pre_vote_bond: U128,
        active_queue_bond: U128,
        accounts: Accounts,
        simple_consent: Consent,
        super_consent: Consent,
    ) -> Self {
        require!(
            pre_vote_bond.0 >= PROPOSAL_STORAGE_COST,
            "min proposal storage cost is required"
        );
        Self {
            prop_counter: 0,
            pre_vote_proposals: LookupMap::new(StorageKey::PreVoteProposals),
            proposals: LookupMap::new(StorageKey::Proposals),
            pre_vote_duration,
            voting_duration,
            pre_vote_bond: pre_vote_bond.0,
            active_queue_bond: active_queue_bond.0,
            pre_vote_support,
            accounts: LazyOption::new(StorageKey::Accounts, Some(&accounts)),
            simple_consent,
            super_consent,
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
    /// possible votes.
    /// NOTE: storage is paid from the bond.
    #[payable]
    #[handle_result]
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
            support: 0,
            supported: HashSet::new(),
            votes: HashMap::new(),
            start: now,
            approved_at: None,
        };
        if active {
            self.proposals.insert(&self.prop_counter, &prop);
        } else {
            self.pre_vote_proposals.insert(&self.prop_counter, &prop);
        }

        if let Err(reason) = finalize_storage_check(storage_start, 0, user) {
            return Err(CreatePropError::Storage(reason));
        }

        Ok(self.prop_counter)
    }

    /// Removes overdue pre-vote proposal.
    /// Fails if proposal is not overdue or not in pre-vote queue.
    #[handle_result]
    pub fn remove_overdue_proposal(&mut self, id: u32) -> Result<(), PrevotePropError> {
        let p = self.remove_pre_vote_prop(id)?;
        if env::block_timestamp_ms() - p.start <= self.pre_vote_duration {
            return Err(PrevotePropError::NotOverdue);
        }
        Promise::new(env::predecessor_account_id()).transfer(REMOVE_REWARD);
        self.slash_prop(id, p.bond - REMOVE_REWARD);
        // NOTE: we don't need to check p.additional_bond: if it is set then the prop wouldn't
        // be in the pre-vote queue.

        Ok(())
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
    pub fn top_up_proposal(&mut self, id: u32) -> Result<bool, PrevotePropError> {
        let user = env::predecessor_account_id();
        let mut bond = env::attached_deposit();
        let mut p = self.remove_pre_vote_prop(id)?;

        if env::block_timestamp_ms() - p.start > self.pre_vote_duration {
            // Transfer attached N, slash bond & keep the proposal removed.
            // Note: user wanted to advance the proposal, rather than slash it, so reward is not
            // distributed.
            Promise::new(user.clone()).transfer(bond);
            self.slash_prop(id, p.bond);
            return Ok(false);
        }

        let required_bond = self.active_queue_bond - p.bond;
        if bond < required_bond {
            return Err(PrevotePropError::MinBond);
        }
        let diff = bond - required_bond;
        if diff > 0 {
            Promise::new(user.clone()).transfer(diff);
            bond -= diff;
        }
        p.additional_bond = Some((user, bond));
        self.insert_prop_to_active(id, &mut p);
        Ok(true)
    }

    /// Supports proposal in the pre-vote queue.
    /// Returns false if the proposal can't be supported because it is overdue.
    #[handle_result]
    pub fn support_proposal(&mut self, id: u32) -> Result<bool, PrevotePropError> {
        let mut p = self.assert_pre_vote_prop(id)?;
        if env::block_timestamp_ms() - p.start > self.pre_vote_duration {
            self.slash_prop(id, p.bond);
            self.pre_vote_proposals.remove(&id);
            return Ok(false);
        }
        let user = env::predecessor_account_id();
        p.add_support(user)?;
        if p.support >= self.pre_vote_support {
            self.pre_vote_proposals.remove(&id);
            self.insert_prop_to_active(id, &mut p);
        } else {
            self.pre_vote_proposals.insert(&id, &p);
        }
        Ok(true)
    }

    #[payable]
    #[handle_result]
    // TODO: must be called via iah_call
    pub fn vote(&mut self, id: u32, vote: Vote) -> Result<(), VoteError> {
        let user = env::predecessor_account_id();
        let storage_start = env::storage_usage();
        let mut prop = self.assert_proposal(id);

        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        if env::block_timestamp_ms() > prop.start + self.voting_duration {
            return Err(VoteError::NotActive);
        }

        prop.add_vote(user.clone(), vote, THRESHOLD)?;

        if prop.status == ProposalStatus::Spam {
            self.proposals.remove(&id);
            emit_spam(id);
            let treasury = self.accounts.get().unwrap().community_treasury;
            Promise::new(treasury).transfer(prop.bond);
            emit_prop_slashed(id, prop.bond);
            return Ok(());
        } else {
            self.proposals.insert(&id, &prop);
            if let Err(reason) = finalize_storage_check(storage_start, 0, user) {
                return Err(VoteError::Storage(reason));
            }
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
        self.refund_bond(id);
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
            PropKind::Dismiss { dao, member } => {
                result = ext_congress::ext(dao.clone())
                    .dismiss_hook(member.clone())
                    .into();
            }
            PropKind::Dissolve { dao } => {
                result = ext_congress::ext(dao.clone()).dissolve_hook().into();
            }
            PropKind::Veto { dao, prop_id } => {
                result = ext_congress::ext(dao.clone()).veto_hook(*prop_id).into();
            }
            PropKind::ApproveBudget { .. } => (),
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

    /// Refund after voting period is over
    pub fn refund_bond(&mut self, id: u32) -> bool {
        let mut prop = self.assert_proposal(id);
        if prop.bond == 0 {
            return false;
        }
        if (prop.status == ProposalStatus::InProgress
            && env::block_timestamp_ms() <= prop.start + self.voting_duration)
            || prop.status == ProposalStatus::PreVote
            || prop.status == ProposalStatus::Spam
            || prop.status == ProposalStatus::Vetoed
        {
            return false;
        }

        // Vote storage is already paid by voters. We only keep storage for proposal.
        let refund = prop.bond - PROPOSAL_STORAGE_COST;
        Promise::new(prop.proposer.clone()).transfer(refund);
        if let Some(val) = prop.additional_bond.clone() {
            Promise::new(val.0).transfer(val.1);
        }
        prop.bond = 0;
        prop.additional_bond = None;
        self.proposals.insert(&id, &prop);
        true
    }

    /*****************
     * ADMIN
     ****************/

    pub fn admin_update_consent(&mut self, simple_consent: Consent, super_consent: Consent) {
        self.assert_admin();
        self.simple_consent = simple_consent;
        self.super_consent = super_consent;
    }

    /*****************
     * INTERNAL
     ****************/

    fn assert_admin(&self) {
        require!(
            env::predecessor_account_id() == self.accounts.get().unwrap().admin,
            "not authorized"
        );
    }

    fn assert_proposal(&self, id: u32) -> Proposal {
        self.proposals.get(&id).expect("proposal does not exist")
    }

    fn remove_pre_vote_prop(&mut self, id: u32) -> Result<Proposal, PrevotePropError> {
        match self.pre_vote_proposals.remove(&id) {
            Some(p) => Ok(p),
            None => Err(PrevotePropError::NotFound),
        }
    }

    fn assert_pre_vote_prop(&mut self, id: u32) -> Result<Proposal, PrevotePropError> {
        match self.pre_vote_proposals.get(&id) {
            Some(p) => Ok(p),
            None => Err(PrevotePropError::NotFound),
        }
    }

    fn insert_prop_to_active(&mut self, prop_id: u32, p: &mut Proposal) {
        p.supported.clear();
        p.status = ProposalStatus::InProgress;
        p.start = env::block_timestamp_ms();
        self.proposals.insert(&prop_id, p);
        emit_prop_active(prop_id);
    }

    fn slash_prop(&mut self, prop_id: u32, amount: Balance) {
        let treasury = self.accounts.get().unwrap().community_treasury;
        Promise::new(treasury).transfer(amount);
        emit_prevote_prop_slashed(prop_id, amount);
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
    use near_sdk::{test_utils::VMContextBuilder, testing_env, AccountId, VMContext, ONE_NEAR};

    use crate::{view::ConfigOutput, *};

    /// 1ms in nano seconds
    const MSECOND: u64 = 1_000_000;

    const START: u64 = 60 * 5 * 1000 * MSECOND;
    // In milliseconds
    const VOTING_DURATION: u64 = 60 * 5 * 1000;
    const PRE_VOTE_DURATION: u64 = 60 * 10 * 1000;
    const PRE_BOND: u128 = ONE_NEAR * 3;
    const BOND: u128 = ONE_NEAR * 500;
    const PRE_VOTE_SUPPORT: u32 = 10;
    const VOTE_DEPOSIT: u128 = ONE_NEAR / 1000;

    fn acc(idx: u8) -> AccountId {
        AccountId::new_unchecked(format!("user-{}.near", idx))
    }

    fn hom() -> AccountId {
        AccountId::new_unchecked("hom.near".to_string())
    }

    fn coa() -> AccountId {
        AccountId::new_unchecked("coa.near".to_string())
    }

    fn tc() -> AccountId {
        AccountId::new_unchecked("tc.near".to_string())
    }

    fn iah_registry() -> AccountId {
        AccountId::new_unchecked("registry.near".to_string())
    }

    fn treasury() -> AccountId {
        AccountId::new_unchecked("treasury.near".to_string())
    }

    fn admin() -> AccountId {
        AccountId::new_unchecked("admin.near".to_string())
    }

    /// creates a test contract with proposal
    fn setup_ctr(attach_deposit: u128) -> (VMContext, Contract, u32) {
        let mut context = VMContextBuilder::new().build();
        let mut contract = Contract::new(
            PRE_VOTE_DURATION,
            VOTING_DURATION,
            PRE_VOTE_SUPPORT,
            U128(PRE_BOND),
            U128(BOND),
            Accounts {
                iah_registry: iah_registry(),
                community_treasury: treasury(),
                congress_hom: hom(),
                congress_coa: coa(),
                congress_tc: tc(),
                admin: admin(),
            },
            Consent {
                quorum: 3,
                threshold: 50,
            },
            Consent {
                quorum: 5,
                threshold: 60,
            },
        );
        context.block_timestamp = START;
        context.predecessor_account_id = acc(1);
        context.attached_deposit = attach_deposit;
        context.account_balance = ONE_NEAR * 2000;
        testing_env!(context.clone());

        let id = contract
            .create_proposal(PropKind::Text, "Proposal unit test 1".to_string())
            .unwrap();

        context.attached_deposit = 0;
        testing_env!(context.clone());

        (context, contract, id)
    }

    fn vote(mut ctx: VMContext, ctr: &mut Contract, accs: Vec<AccountId>, id: u32, vote: Vote) {
        for a in accs {
            ctx.predecessor_account_id = a.clone();
            ctx.attached_deposit = VOTE_DEPOSIT;
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
        // Try vote with less storage
        match ctr.vote(id, Vote::Approve) {
            Err(VoteError::Storage(_)) => (),
            x => panic!("expected Storage, got: {:?}", x),
        }
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
        ctx.account_balance = ONE_NEAR * 1000;
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
        ctx.attached_deposit = BOND;
        testing_env!(ctx.clone());
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
    fn refund_bond_test() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );
        ctx.block_timestamp = START + (ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx.clone());

        ctr.execute(id).unwrap();
        let mut prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Executed);

        // try to get refund again
        assert_eq!(ctr.refund_bond(id), false);

        // Get refund for proposal with no status update
        ctx.attached_deposit = BOND;
        testing_env!(ctx.clone());
        let id2 = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();
        prop = ctr.get_proposal(id2).unwrap();

        // Set time after voting period
        ctx.block_timestamp = (prop.proposal.start + ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        // Call refund
        assert_eq!(ctr.refund_bond(id2), true);
    }

    #[test]
    fn config_query() {
        let (_, ctr, _) = setup_ctr(PRE_BOND);
        let expected = ConfigOutput {
            prop_counter: 1,
            pre_vote_bond: U128(PRE_BOND),
            active_queue_bond: U128(BOND),
            pre_vote_support: 10,
            simple_consent: Consent {
                quorum: 3,
                threshold: 50,
            },
            super_consent: Consent {
                quorum: 5,
                threshold: 60,
            },
            pre_vote_duration: PRE_VOTE_DURATION,
            voting_duration: VOTING_DURATION,
            accounts: Accounts {
                iah_registry: iah_registry(),
                community_treasury: treasury(),
                congress_hom: hom(),
                congress_coa: coa(),
                congress_tc: tc(),
                admin: admin(),
            },
        };
        assert_eq!(ctr.config(), expected);
    }

    #[test]
    fn overwrite_votes() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        let mut p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::InProgress);
        assert!((p.proposal.votes.is_empty()));

        ctx.predecessor_account_id = acc(1);
        ctx.attached_deposit = VOTE_DEPOSIT;
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

    #[test]
    fn get_proposals() {
        let (mut ctx, mut ctr, id1) = setup_ctr(BOND);
        ctx.attached_deposit = BOND;
        testing_env!(ctx.clone());
        let id2 = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 2".to_string())
            .unwrap();
        ctx.attached_deposit = BOND;
        testing_env!(ctx);
        let id3 = ctr
            .create_proposal(PropKind::Text, "Proposal unit test 3".to_string())
            .unwrap();
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

    #[test]
    fn support_proposal() {
        let (mut ctx, mut ctr, id) = setup_ctr(PRE_BOND);

        // make one less support then what is necessary to test that the proposal is still in prevote
        for i in 1..PRE_VOTE_SUPPORT {
            ctx.predecessor_account_id = acc(i as u8);
            testing_env!(ctx.clone());
            assert_eq!(ctr.support_proposal(id), Ok(true));
        }

        assert_eq!(
            ctr.support_proposal(id),
            Err(PrevotePropError::DoubleSupport)
        );

        let p = ctr.assert_pre_vote_prop(id).unwrap();
        assert_eq!(p.status, ProposalStatus::PreVote);
        assert_eq!(p.support, PRE_VOTE_SUPPORT - 1);
        for i in 1..PRE_VOTE_SUPPORT {
            assert!(p.supported.contains(&acc(i as u8)))
        }

        // add the missing support and assert that the proposal was moved to active
        ctx.predecessor_account_id = acc(PRE_VOTE_SUPPORT as u8);
        ctx.block_timestamp = START + 2 * MSECOND;
        testing_env!(ctx.clone());
        assert_eq!(ctr.support_proposal(id), Ok(true));

        // should be removed from prevote queue
        assert_eq!(
            ctr.assert_pre_vote_prop(id),
            Err(PrevotePropError::NotFound)
        );
        let p = ctr.assert_proposal(id);
        assert_eq!(p.status, ProposalStatus::InProgress);
        assert_eq!(p.support, PRE_VOTE_SUPPORT);
        assert_eq!(p.start, ctx.block_timestamp / MSECOND);
        assert!(p.supported.is_empty());

        // can't support proposal which was already moved
        assert_eq!(ctr.support_proposal(id), Err(PrevotePropError::NotFound));

        //
        // Should not be able to support an overdue proposal
        //
        ctx.attached_deposit = PRE_BOND;
        testing_env!(ctx.clone());

        let id = ctr
            .create_proposal(PropKind::Text, "proposal".to_owned())
            .unwrap();
        ctx.block_timestamp += (PRE_VOTE_DURATION + 1) * MSECOND;
        testing_env!(ctx.clone());
        assert_eq!(ctr.support_proposal(id), Ok(false));
        assert_eq!(ctr.get_proposal(id), None);
    }

    #[test]
    fn update_consent() {
        let (mut ctx, mut ctr, _) = setup_ctr(BOND);
        ctx.predecessor_account_id = admin();
        testing_env!(ctx.clone());

        let c1 = Consent {
            quorum: 11,
            threshold: 1,
        };
        let c2 = Consent {
            quorum: 12,
            threshold: 2,
        };
        ctr.admin_update_consent(c1, c2);
        assert_eq!(c1, ctr.simple_consent);
        assert_eq!(c2, ctr.super_consent);
    }
}
