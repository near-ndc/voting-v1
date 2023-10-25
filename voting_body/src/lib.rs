use std::collections::{HashMap, HashSet};

use common::finalize_storage_check;
use events::*;
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::{LazyOption, LookupMap},
    env::{self, panic_str},
    json_types::U128,
    near_bindgen, require, AccountId, Balance, Gas, PanicOnDefault, Promise, PromiseOrValue,
    PromiseResult,
};
use types::{CreatePropPayload, ExecResponse, SBTs, SupportPropPayload, VotePayload};

mod constants;
mod errors;
mod events;
mod ext;
pub mod proposal;
mod storage;
mod types;
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
    /// All duration arguments are in miliseconds.
    /// * hook_auth : map of accounts authorized to call hooks.
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
            simple_consent.verify() && super_consent.verify(),
            "threshold must be a percentage (0-100%)"
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
    /// Must be called via `iah_registry.is_human_call`.
    /// NOTE: storage is paid from the bond.
    /// Panics when the FunctionCall is trying to call any of the congress contracts.
    #[payable]
    #[handle_result]
    pub fn create_proposal(
        &mut self,
        caller: AccountId,
        #[allow(unused_variables)] iah_proof: SBTs,
        payload: CreatePropPayload,
    ) -> Result<u32, CreatePropError> {
        self.assert_iah_registry();
        let storage_start = env::storage_usage();
        let now = env::block_timestamp_ms();
        let bond = env::attached_deposit();

        if bond < self.pre_vote_bond {
            return Err(CreatePropError::MinBond);
        }

        if let PropKind::FunctionCall { receiver_id, .. } = &payload.kind {
            let accounts = self.accounts.get().unwrap();

            if *receiver_id == accounts.congress_coa
                || *receiver_id == accounts.congress_hom
                || *receiver_id == accounts.congress_tc
            {
                return Err(CreatePropError::FunctionCall(
                    "receiver_id can't be a congress house, use a specific proposal to interact with the congress".to_string(),
                ));
            }
        }

        // TODO: check if proposal is created by a congress member. If yes, move it to active
        // immediately.
        let active = bond >= self.active_queue_bond;
        self.prop_counter += 1;
        emit_prop_created(self.prop_counter, &payload.kind, active);
        let mut prop = Proposal {
            proposer: caller.clone(),
            bond,
            additional_bond: None,
            description: payload.description,
            kind: payload.kind,
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
            proposal_storage: 0,
        };
        if active {
            self.proposals.insert(&self.prop_counter, &prop);
        } else {
            self.pre_vote_proposals.insert(&self.prop_counter, &prop);
        }

        prop.proposal_storage = match finalize_storage_check(storage_start, 0, caller) {
            Err(reason) => return Err(CreatePropError::Storage(reason)),
            Ok(required) => required,
        };
        if active {
            self.proposals.insert(&self.prop_counter, &prop);
        } else {
            self.pre_vote_proposals.insert(&self.prop_counter, &prop);
        }

        Ok(self.prop_counter)
    }

    /// Removes overdue pre-vote proposal and slashes the proposer.
    /// User who calls the function to receives REMOVE_REWARD.
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
    /// Must be called via `iah_registry.is_human_call`.
    #[handle_result]
    pub fn support_proposal(
        &mut self,
        caller: AccountId,
        #[allow(unused_variables)] iah_proof: SBTs,
        payload: SupportPropPayload,
    ) -> Result<bool, PrevotePropError> {
        self.assert_iah_registry();
        let mut p = self.assert_pre_vote_prop(payload.prop_id)?;
        if env::block_timestamp_ms() - p.start > self.pre_vote_duration {
            self.slash_prop(payload.prop_id, p.bond);
            self.pre_vote_proposals.remove(&payload.prop_id);
            return Ok(false);
        }
        p.add_support(caller)?;
        if p.support >= self.pre_vote_support {
            self.pre_vote_proposals.remove(&payload.prop_id);
            self.insert_prop_to_active(payload.prop_id, &mut p);
        } else {
            self.pre_vote_proposals.insert(&payload.prop_id, &p);
        }
        Ok(true)
    }

    /// Congressional support for a pre-vote proposal to move it to the active queue.
    /// Returns false if the proposal can't be supported because it is overdue.
    #[handle_result]
    pub fn support_proposal_by_congress(
        &mut self,
        prop_id: u32,
        dao: AccountId,
    ) -> Result<Promise, PrevotePropError> {
        let a = self.accounts.get().unwrap();
        if !(a.congress_coa == dao || a.congress_hom == dao || a.congress_tc == dao) {
            return Err(PrevotePropError::NotCongress);
        }

        Ok(ext_congress::ext(dao)
            .is_member(env::predecessor_account_id())
            .then(ext_self::ext(env::current_account_id()).on_support_by_congress(prop_id)))
    }

    /// Returns false if the proposal can't be supported because it is overdue.
    #[private]
    #[handle_result]
    pub fn on_support_by_congress(
        &mut self,
        #[callback_result] is_member: Result<bool, near_sdk::PromiseError>,
        prop_id: u32,
    ) -> Result<bool, PrevotePropError> {
        if !is_member.unwrap_or(false) {
            return Err(PrevotePropError::NotCongressMember);
        }

        let mut p = self.remove_pre_vote_prop(prop_id)?;
        if env::block_timestamp_ms() - p.start > self.pre_vote_duration {
            self.slash_prop(prop_id, p.bond);
            return Ok(false);
        }
        self.insert_prop_to_active(prop_id, &mut p);
        Ok(true)
    }

    /// Must be called via `iah_registry.is_human_call`.
    #[payable]
    #[handle_result]
    pub fn vote(
        &mut self,
        caller: AccountId,
        #[allow(unused_variables)] iah_proof: SBTs,
        payload: VotePayload,
    ) -> Result<(), VoteError> {
        self.assert_iah_registry();
        let storage_start = env::storage_usage();
        let mut prop = self.assert_proposal(payload.prop_id);

        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        if !prop.is_active(self.voting_duration) {
            return Err(VoteError::Timeout);
        }

        prop.add_vote(caller.clone(), payload.vote)?;
        // NOTE: we can't quickly set a status to a finalized one because we don't know the total number of
        // voters

        self.proposals.insert(&payload.prop_id, &prop);
        emit_vote(payload.prop_id);

        if let Err(reason) = finalize_storage_check(storage_start, 0, caller) {
            return Err(VoteError::Storage(reason));
        }
        Ok(())
    }

    /// Allows anyone to execute or slash the proposal.
    /// If proposal is slasheable, the user who executes gets REMOVE_REWARD.
    #[handle_result]
    pub fn execute(&mut self, id: u32) -> Result<PromiseOrValue<ExecResponse>, ExecError> {
        let mut prop = self.assert_proposal(id);
        // quick return, can only execute if the status was not switched yet, or it
        // failed (previous attempt to execute failed).
        if !matches!(
            prop.status,
            ProposalStatus::InProgress | ProposalStatus::Failed
        ) {
            return Err(ExecError::AlreadyFinalized);
        }

        prop.recompute_status(self.voting_duration, self.prop_consent(&prop));
        match prop.status {
            ProposalStatus::PreVote => panic_str("pre-vote proposal can't be in the active queue"),
            ProposalStatus::InProgress => return Err(ExecError::InProgress),
            ProposalStatus::Executed => return Ok(PromiseOrValue::Value(ExecResponse::Executed)),
            ProposalStatus::Rejected => {
                self.proposals.insert(&id, &prop);
                return Ok(PromiseOrValue::Value(ExecResponse::Rejected));
            }
            ProposalStatus::Spam => {
                emit_spam(id);
                emit_prop_slashed(id, prop.bond); // needs to be called before we zero prop.bond
                prop.slash_bond(self.accounts.get().unwrap().community_treasury);
                self.proposals.remove(&id);
                return Ok(PromiseOrValue::Value(ExecResponse::Slashed));
            }
            ProposalStatus::Approved | ProposalStatus::Failed => (), // execute below
        };

        prop.refund_bond();
        prop.status = ProposalStatus::Executed;
        let mut out = PromiseOrValue::Value(ExecResponse::Executed);
        match &prop.kind {
            PropKind::Dismiss { dao, member } => {
                out = ext_congress::ext(dao.clone())
                    .dismiss_hook(member.clone())
                    .into();
            }
            PropKind::Dissolve { dao } => {
                out = ext_congress::ext(dao.clone()).dissolve_hook().into();
            }
            PropKind::Veto { dao, prop_id } => {
                out = ext_congress::ext(dao.clone()).veto_hook(*prop_id).into();
            }
            PropKind::ApproveBudget { .. } => (),
            PropKind::Text => (),
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
                out = promise.into();
            }
        };

        self.proposals.insert(&id, &prop);

        let out = match out {
            PromiseOrValue::Promise(promise) => promise
                .then(
                    ext_self::ext(env::current_account_id())
                        .with_static_gas(EXECUTE_CALLBACK_GAS)
                        .on_execute(id),
                )
                .into(),
            _ => {
                emit_executed(id);
                out
            }
        };
        Ok(out)
    }

    /*****************
     * ADMIN
     ****************/

    pub fn admin_update_consent(&mut self, simple_consent: Consent, super_consent: Consent) {
        self.assert_admin();
        self.simple_consent = simple_consent;
        self.super_consent = super_consent;
    }

    /// udpate voting time for e2e tests purposes
    /// TODO: remove
    pub fn admin_update_durations(&mut self, pre_vote_duration: u64, voting_duration: u64) {
        self.assert_admin();
        require!(
            env::current_account_id().as_ref().contains("test"),
            "can only be run in test contracts"
        );

        self.pre_vote_duration = pre_vote_duration;
        self.voting_duration = voting_duration;
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

    fn assert_iah_registry(&self) {
        require!(
            env::predecessor_account_id() == self.accounts.get().unwrap().iah_registry,
            "must be called by iah_registry"
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

    fn prop_consent(&self, prop: &Proposal) -> Consent {
        match prop.kind.required_consent() {
            ConsentKind::Simple => self.simple_consent.clone(),
            ConsentKind::Super => self.super_consent.clone(),
        }
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

    fn vote_payload(id: u32, vote: Vote) -> VotePayload {
        VotePayload { prop_id: id, vote }
    }

    fn create_prop_payload(kind: PropKind, description: String) -> CreatePropPayload {
        CreatePropPayload { kind, description }
    }

    fn support_prop_payload(id: u32) -> SupportPropPayload {
        SupportPropPayload { prop_id: id }
    }

    fn iah_proof() -> SBTs {
        vec![(iah_registry(), vec![1, 4])]
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
        context.predecessor_account_id = iah_registry();
        context.attached_deposit = attach_deposit;
        context.account_balance = ONE_NEAR * 2000;
        testing_env!(context.clone());

        let id = contract
            .create_proposal(
                acc(1),
                iah_proof(),
                create_prop_payload(PropKind::Text, "Proposal unit test 1".to_string()),
            )
            .unwrap();

        context.attached_deposit = 0;
        testing_env!(context.clone());

        (context, contract, id)
    }

    fn vote(mut ctx: VMContext, ctr: &mut Contract, accs: Vec<AccountId>, id: u32, vote: Vote) {
        for a in accs {
            ctx.predecessor_account_id = iah_registry();
            ctx.attached_deposit = VOTE_DEPOSIT;
            testing_env!(ctx.clone());
            let res = ctr.vote(a.clone(), iah_proof(), vote_payload(id, vote.clone()));
            assert_eq!(
                res,
                Ok(()),
                "\nacc {} _____ prop: {:?}",
                a,
                ctr.proposals.get(&id).unwrap(),
            );
        }
    }

    fn create_proposal(mut ctx: VMContext, ctr: &mut Contract, bond: Balance) -> u32 {
        ctx.predecessor_account_id = iah_registry();
        ctx.attached_deposit = bond;
        testing_env!(ctx.clone());
        ctr.create_proposal(
            acc(1),
            iah_proof(),
            create_prop_payload(PropKind::Text, "Proposal unit test".to_string()),
        )
        .unwrap()
    }

    fn create_proposal_with_status(
        mut ctx: VMContext,
        ctr: &mut Contract,
        status: ProposalStatus,
    ) -> u32 {
        ctx.predecessor_account_id = iah_registry();
        ctx.attached_deposit = BOND;
        testing_env!(ctx.clone());
        let id = ctr
            .create_proposal(
                acc(1),
                iah_proof(),
                create_prop_payload(PropKind::Text, "Proposal unit test".to_string()),
            )
            .unwrap();
        let mut prop = ctr.proposals.get(&id).unwrap();
        prop.status = status;
        ctr.proposals.insert(&id, &prop);
        id
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

        //
        // Try vote with less storage
        ctx.predecessor_account_id = iah_registry();
        ctx.attached_deposit = 0;
        testing_env!(ctx.clone());
        match ctr.vote(acc(2), iah_proof(), vote_payload(id, Vote::Approve)) {
            Err(VoteError::Storage(_)) => (),
            x => panic!("expected Storage, got: {:?}", x),
        }

        // TODO: vote through the SBT coin check

        //
        // Successful vote
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );

        //
        // Proposal already got enough votes, but the voting time is not over yet. So, we can
        // still vote, but we can't execute.
        ctx.block_timestamp += VOTING_DURATION / 2 * MSECOND;
        ctx.attached_deposit = ONE_NEAR / 10;
        testing_env!(ctx.clone());
        prop1 = ctr.get_proposal(id).unwrap();
        assert_eq!(prop1.proposal.status, ProposalStatus::InProgress);
        assert_eq!(
            ctr.vote(acc(5), iah_proof(), vote_payload(id, Vote::Spam)),
            Ok(())
        );
        prop1.proposal.spam += 1;
        prop1.proposal.votes.insert(acc(5), Vote::Spam);
        assert!(matches!(ctr.execute(id), Err(ExecError::InProgress)));

        //
        // Create a new proposal, not enough bond
        //
        let resp = ctr.create_proposal(
            acc(1),
            iah_proof(),
            create_prop_payload(PropKind::Text, "proposal".to_owned()),
        );
        assert_eq!(resp, Err(CreatePropError::MinBond));

        //
        // Create a new proposal with bond to active queue and check double vote and expire
        // Check all votes
        //
        ctx.account_balance = ONE_NEAR * 1000;
        let id = create_proposal(ctx.clone(), &mut ctr, BOND);
        let mut prop2 = ctr.get_proposal(id).unwrap();
        assert_eq!(
            ctr.vote(acc(3), iah_proof(), vote_payload(id, Vote::Approve)),
            Ok(())
        );
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

        //
        // create proposal, cast votes, but not enough to approve.
        // set timestamp past voting period, status should be rejected
        //
        let id = create_proposal(ctx.clone(), &mut ctr, BOND);
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2)],
            id,
            Vote::Approve,
        );
        vote(ctx.clone(), &mut ctr, vec![acc(3)], id, Vote::Reject);

        ctx.block_timestamp += (ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx);
        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, ProposalStatus::Rejected);
    }

    #[test]
    fn proposal_overdue() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        ctx.block_timestamp = START + (ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        assert_eq!(
            ctr.vote(acc(1), iah_proof(), vote_payload(id, Vote::Approve)),
            Err(VoteError::Timeout)
        );
    }

    #[test]
    #[should_panic(expected = "proposal does not exist")]
    fn vote_not_exist() {
        let (_, mut ctr, _) = setup_ctr(BOND);
        ctr.vote(acc(1), iah_proof(), vote_payload(10, Vote::Approve))
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "proposal does not exist")]
    fn vote_not_active() {
        let (_, mut ctr, id) = setup_ctr(BOND);
        ctr.vote(acc(1), iah_proof(), vote_payload(id, Vote::Approve))
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "proposal does not exist")]
    fn execute_not_active() {
        let (_, mut ctr, id) = setup_ctr(PRE_BOND);
        assert!(ctr.execute(id).is_err());
    }

    #[test]
    fn execution_text() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        match ctr.execute(id) {
            Ok(_) => panic!("expected InProgress, got: OK"),
            Err(err) => assert_eq!(err, ExecError::InProgress),
        }
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );

        ctx.block_timestamp = START + ctr.voting_duration / 2 * MSECOND;
        testing_env!(ctx.clone());
        let mut p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::InProgress);
        match ctr.execute(id) {
            Ok(_) => panic!("expected InProgress, got: OK"),
            Err(err) => assert_eq!(err, ExecError::InProgress),
        }

        // fast forward to voting overtime
        ctx.block_timestamp = START + (ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        assert!(matches!(
            ctr.execute(id),
            Ok(PromiseOrValue::Value(ExecResponse::Executed))
        ));
        p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::Executed);

        //
        // check spam transaction
        let id = create_proposal_with_status(ctx.clone(), &mut ctr, ProposalStatus::Spam);
        assert!(matches!(ctr.execute(id), Err(ExecError::AlreadyFinalized)));

        //
        // check spam transaction, part2
        let id = create_proposal_with_status(ctx.clone(), &mut ctr, ProposalStatus::InProgress);
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Spam,
        );
        ctx.block_timestamp += (ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        match ctr.execute(id) {
            Ok(PromiseOrValue::Value(ExecResponse::Slashed)) => (),
            Ok(_) => panic!("expected Ok(ExecResponse:Slashed)"),
            Err(err) => panic!("expected Ok(ExecResponse:Slashed), got: {:?}", err),
        }

        p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.bond, 0);
        // second execute should return AlreadySlashed
        match ctr.execute(id) {
            Ok(_) => panic!("expected Err(ExecError::AlreadyFinalized)"),
            Err(err) => assert_eq!(err, ExecError::AlreadyFinalized),
        }
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
        let mut p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::Executed);

        // try to get refund again
        assert!(!p.proposal.refund_bond());

        // Get refund for proposal with no status update
        ctx.attached_deposit = BOND;
        testing_env!(ctx.clone());
        let id2 = ctr
            .create_proposal(
                acc(1),
                iah_proof(),
                create_prop_payload(PropKind::Text, "Proposal unit test".to_string()),
            )
            .unwrap();
        p = ctr.get_proposal(id2).unwrap();

        // Set time after voting period
        ctx.block_timestamp = (p.proposal.start + ctr.voting_duration + 1) * MSECOND;
        testing_env!(ctx);

        // Call refund
        assert!(p.proposal.refund_bond());
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

        ctx.attached_deposit = VOTE_DEPOSIT;
        testing_env!(ctx.clone());

        assert_eq!(
            ctr.vote(acc(1), iah_proof(), vote_payload(id, Vote::Approve)),
            Ok(())
        );
        p.proposal.approve = 1;
        p.proposal.votes.insert(acc(1), Vote::Approve);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(
            ctr.vote(acc(1), iah_proof(), vote_payload(id, Vote::Abstain)),
            Ok(())
        );
        p.proposal.approve = 0;
        p.proposal.abstain = 1;
        p.proposal.votes.insert(acc(1), Vote::Abstain);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(
            ctr.vote(acc(1), iah_proof(), vote_payload(id, Vote::Reject)),
            Ok(())
        );
        p.proposal.abstain = 0;
        p.proposal.reject = 1;
        p.proposal.votes.insert(acc(1), Vote::Reject);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(
            ctr.vote(acc(1), iah_proof(), vote_payload(id, Vote::Spam)),
            Ok(())
        );
        p.proposal.reject = 0;
        p.proposal.spam = 1;
        p.proposal.votes.insert(acc(1), Vote::Spam);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);
    }

    #[test]
    fn get_proposals() {
        let (ctx, mut ctr, id1) = setup_ctr(BOND);
        let id2 = create_proposal(ctx.clone(), &mut ctr, BOND);
        let id3 = create_proposal(ctx.clone(), &mut ctr, BOND);
        let prop1 = ctr.get_proposal(id1).unwrap();
        let prop2 = ctr.get_proposal(id2).unwrap();
        let prop3 = ctr.get_proposal(id3).unwrap();
        assert_eq!(ctr.number_of_proposals(), 3);
        // non reversed
        assert_eq!(ctr.get_proposals(1, 1, None), vec![prop1.clone()]);
        assert_eq!(
            ctr.get_proposals(0, 10, None),
            vec![prop1.clone(), prop2.clone(), prop3.clone()]
        );
        // non reversed with litmit
        assert_eq!(
            ctr.get_proposals(0, 2, None),
            vec![prop1.clone(), prop2.clone()]
        );
        assert_eq!(
            ctr.get_proposals(0, 2, Some(false)),
            vec![prop1.clone(), prop2.clone()]
        );
        assert_eq!(
            ctr.get_proposals(1, 2, Some(false)),
            vec![prop1.clone(), prop2.clone()]
        );
        // reversed, limit bigger than amount of proposals -> return all
        assert_eq!(
            ctr.get_proposals(3, 10, Some(true)),
            vec![prop3.clone(), prop2.clone(), prop1.clone()]
        );
        // reversed with limit and over the "last proposal"
        assert_eq!(
            ctr.get_proposals(5, 2, Some(true)),
            vec![prop3.clone(), prop2.clone()]
        );

        // few more edge cases
        assert_eq!(ctr.get_proposals(1, 0, None), vec![], "limit=0");
        assert_eq!(
            ctr.get_proposals(0, 1, None),
            vec![prop1.clone()],
            "0 = start from the last proposal (rev=false)"
        );
        assert_eq!(
            ctr.get_proposals(0, 1, Some(true)),
            vec![prop3.clone()],
            "0 = start from the last proposal (rev=true)"
        );
        assert_eq!(ctr.get_proposals(2, 1, None), vec![prop2.clone()],);
        assert_eq!(ctr.get_proposals(2, 1, Some(true)), vec![prop2.clone()],);
    }

    #[test]
    fn support_proposal() {
        let (mut ctx, mut ctr, id) = setup_ctr(PRE_BOND);

        // make one less support then what is necessary to test that the proposal is still in prevote
        for i in 1..PRE_VOTE_SUPPORT {
            assert_eq!(
                ctr.support_proposal(acc(i as u8), iah_proof(), support_prop_payload(id)),
                Ok(true)
            );
        }

        assert_eq!(
            ctr.support_proposal(acc(1), iah_proof(), support_prop_payload(id)),
            Err(PrevotePropError::DoubleSupport)
        );

        let p = ctr.assert_pre_vote_prop(id).unwrap();
        assert_eq!(p.status, ProposalStatus::PreVote);
        assert_eq!(p.support, PRE_VOTE_SUPPORT - 1);
        for i in 1..PRE_VOTE_SUPPORT {
            assert!(p.supported.contains(&acc(i as u8)))
        }

        // add the missing support and assert that the proposal was moved to active
        ctx.block_timestamp = START + 2 * MSECOND;
        testing_env!(ctx.clone());
        assert_eq!(
            ctr.support_proposal(
                acc(PRE_VOTE_SUPPORT as u8),
                iah_proof(),
                support_prop_payload(id)
            ),
            Ok(true)
        );

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
        assert_eq!(
            ctr.support_proposal(acc(1), iah_proof(), support_prop_payload(id)),
            Err(PrevotePropError::NotFound)
        );

        //
        // Should not be able to support an overdue proposal
        //
        let id = create_proposal(ctx.clone(), &mut ctr, PRE_BOND);
        ctx.block_timestamp += (PRE_VOTE_DURATION + 1) * MSECOND;
        testing_env!(ctx.clone());
        assert_eq!(
            ctr.support_proposal(acc(1), iah_proof(), support_prop_payload(id)),
            Ok(false)
        );
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

    #[should_panic(expected = "must be called by iah_registry")]
    #[test]
    fn vote_not_called_by_iah_registry() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx);
        ctr.vote(acc(1), iah_proof(), vote_payload(id, Vote::Approve))
            .unwrap();
    }

    #[should_panic(expected = "must be called by iah_registry")]
    #[test]
    fn create_proposal_not_called_by_iah_registry() {
        let (mut ctx, mut ctr, _) = setup_ctr(BOND);
        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx);
        ctr.create_proposal(
            acc(1),
            iah_proof(),
            create_prop_payload(PropKind::Text, "Proposal unit test".to_string()),
        )
        .unwrap();
    }

    #[should_panic(expected = "must be called by iah_registry")]
    #[test]
    fn support_proposal_not_called_by_iah_registry() {
        let (mut ctx, mut ctr, id) = setup_ctr(PRE_BOND);
        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx);
        ctr.support_proposal(acc(1), iah_proof(), support_prop_payload(id))
            .unwrap();
    }

    #[test]
    fn support_proposal_by_congress() {
        let (_, mut ctr, id) = setup_ctr(PRE_BOND);

        match ctr.support_proposal_by_congress(id, iah_registry()) {
            Err(PrevotePropError::NotCongress) => (),
            _ => panic!("expected error: provided DAO must be one of the congress houses"),
        };
        assert!(
            ctr.support_proposal_by_congress(id, tc()).is_ok(),
            "must accept valid dao parameter"
        );
    }

    #[test]
    fn on_support_by_congress() {
        let (mut ctx, mut ctr, id) = setup_ctr(PRE_BOND);

        assert_eq!(
            ctr.on_support_by_congress(Ok(false), id),
            Err(PrevotePropError::NotCongressMember)
        );
        assert_eq!(
            ctr.on_support_by_congress(Err(near_sdk::PromiseError::Failed), id),
            Err(PrevotePropError::NotCongressMember)
        );
        assert!(
            ctr.pre_vote_proposals.contains_key(&id),
            "should not be moved"
        );

        //
        // outdated proposal should be removed
        ctx.block_timestamp += (ctr.pre_vote_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        assert_eq!(ctr.on_support_by_congress(Ok(true), id), Ok(false));
        assert_eq!(ctr.get_proposal(id), None);

        //
        // check that proposal was moved
        let id = create_proposal(ctx.clone(), &mut ctr, PRE_BOND);
        ctx.block_timestamp += MSECOND;
        testing_env!(ctx.clone());
        let mut prop = ctr.get_proposal(id).unwrap();

        assert_eq!(ctr.on_support_by_congress(Ok(true), id), Ok(true));
        assert_eq!(
            ctr.assert_pre_vote_prop(id),
            Err(PrevotePropError::NotFound)
        );
        // modify prop to expected values and see if it equals the stored one
        prop.proposal.status = ProposalStatus::InProgress;
        prop.proposal.start += 1; // start is in miliseconds
        assert_eq!(ctr.get_proposal(id).unwrap(), prop);
    }

    #[test]
    fn create_proposal_function_call_to_congress() {
        let (mut ctx, mut ctr, _) = setup_ctr(BOND);
        ctx.predecessor_account_id = iah_registry();
        ctx.attached_deposit = BOND;
        testing_env!(ctx.clone());
        match ctr.create_proposal(
            acc(1),
            iah_proof(),
            create_prop_payload(
                PropKind::FunctionCall {
                    receiver_id: hom(),
                    actions: vec![],
                },
                "Proposal unit test".to_string(),
            ),
        ) {
            Ok(_) => panic!("expected Err(CreatePropError::FunctionCall)"),
            Err(err) => assert_eq!(
                err,
                CreatePropError::FunctionCall("receiver_id can't be a congress house, use a specific proposal to interact with the congress".to_string())
            ),
        }
    }
}
