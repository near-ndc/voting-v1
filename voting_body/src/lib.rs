use std::collections::HashSet;

use common::finalize_storage_check;
use events::*;
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::{LazyOption, LookupMap},
    env::{self, panic_str},
    json_types::U128,
    near_bindgen, require,
    store::LookupSet,
    AccountId, Balance, FunctionError, Gas, PanicOnDefault, Promise, PromiseOrValue, PromiseResult,
};
use types::{CreatePropPayload, ExecResponse, SBTs, VotePayload};

mod constants;
mod errors;
mod events;
mod ext;
mod impls;
pub mod migrate;
pub mod proposal;
mod storage;
pub mod types;
pub mod view;

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
    /// map (prop_id, voter) -> VoteRecord
    pub votes: LookupMap<(u32, AccountId), VoteRecord>,

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

    /// all times below are in milliseconds
    pub pre_vote_duration: u64,
    pub vote_duration: u64,
    pub accounts: LazyOption<Accounts>,

    /// Workaround for removing people from iom registry blacklist
    /// As we don't have a way to remove people from the blacklist, we can add them to the whitelist
    /// and allow them to vote directly.
    pub iom_whitelist: LookupSet<AccountId>,
}

#[near_bindgen]
impl Contract {
    #[init]
    /// All duration arguments are in milliseconds.
    /// * hook_auth : map of accounts authorized to call hooks.
    pub fn new(
        pre_vote_duration: u64,
        vote_duration: u64,
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
            votes: LookupMap::new(StorageKey::Votes),
            pre_vote_duration,
            vote_duration,
            pre_vote_bond: pre_vote_bond.0,
            active_queue_bond: active_queue_bond.0,
            pre_vote_support,
            accounts: LazyOption::new(StorageKey::Accounts, Some(&accounts)),
            simple_consent,
            super_consent,
            iom_whitelist: LookupSet::new(StorageKey::IomWhitelist),
        }
    }

    /*
     * Queries are in view.rs
     */

    /**********
     * TRANSACTIONS
     **********/

    #[payable]
    pub fn create_proposal_whitelist(&mut self, payload: CreatePropPayload) -> u32 {
        let caller = env::predecessor_account_id();
        self.assert_whitelist(&caller);

        match self.create_proposal_impl(caller, payload) {
            Ok(id) => id,
            Err(error) => error.panic(),
        }
    }

    /// Must be called via `iah_registry.is_human_call`.
    #[payable]
    #[handle_result]
    pub fn create_proposal(
        &mut self,
        caller: AccountId,
        #[allow(unused_variables)] iah_proof: SBTs,
        payload: CreatePropPayload,
    ) -> Result<u32, CreatePropError> {
        if env::predecessor_account_id() != self.accounts.get().unwrap().iah_registry {
            return Err(CreatePropError::NotIAHreg);
        }

        self.create_proposal_impl(caller, payload)
    }

    /// Removes overdue pre-vote proposal and slashes the proposer.
    /// User who calls the function to receives REMOVE_REWARD.
    /// Fails if proposal is not overdue or not in pre-vote queue.
    #[handle_result]
    pub fn slash_prevote_proposal(&mut self, id: u32) -> Result<(), PrevoteError> {
        let p = self.remove_pre_vote_prop(id)?;
        if env::block_timestamp_ms() - p.start <= self.pre_vote_duration {
            return Err(PrevoteError::NotOverdue);
        }
        Promise::new(env::predecessor_account_id()).transfer(SLASH_REWARD);
        self.slash_prop(id, p.bond - SLASH_REWARD);
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
    pub fn top_up_proposal(&mut self, id: u32) -> Result<bool, PrevoteError> {
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
            return Err(PrevoteError::MinBond);
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

    pub fn support_proposal_whitelist(&mut self, payload: u32) -> bool {
        let caller = env::predecessor_account_id();
        self.assert_whitelist(&caller);

        match self.support_proposal_impl(
            caller,
            // Lock is required to prevent double voting by moving sbt to another account.
            // It is not required for the whitelist version, as only whitelisted accounts can call
            env::block_timestamp_ms() + MAX_DURATION + 1,
            payload,
        ) {
            Ok(supported) => supported,
            Err(err) => err.panic(),
        }
    }

    /// Supports proposal in the pre-vote queue.
    /// Returns false if the proposal can't be supported because it is overdue.
    /// Must be called via `iah_registry.is_human_call_lock` with
    /// `lock_duration: self.pre_vote_duration + 1`.
    /// `payload` must be a pre-vote proposal ID.
    #[handle_result]
    pub fn support_proposal(
        &mut self,
        caller: AccountId,
        locked_until: u64,
        #[allow(unused_variables)] iah_proof: Option<SBTs>,
        payload: u32,
    ) -> Result<bool, PrevoteError> {
        if env::predecessor_account_id() != self.accounts.get().unwrap().iah_registry {
            return Err(PrevoteError::NotIAHreg);
        }
        self.support_proposal_impl(caller, locked_until, payload)
    }

    /// Congressional support for a pre-vote proposal to move it to the active queue.
    /// Returns false if the proposal can't be supported because it is overdue.
    #[handle_result]
    pub fn support_proposal_by_congress(
        &mut self,
        prop_id: u32,
        dao: AccountId,
    ) -> Result<Promise, PrevoteError> {
        let a = self.accounts.get().unwrap();
        if !(a.congress_coa == dao || a.congress_hom == dao || a.congress_tc == dao) {
            return Err(PrevoteError::NotCongress);
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
    ) -> Result<bool, PrevoteError> {
        if !is_member.unwrap_or(false) {
            return Err(PrevoteError::NotCongressMember);
        }

        let mut p = self.remove_pre_vote_prop(prop_id)?;
        if env::block_timestamp_ms() - p.start > self.pre_vote_duration {
            self.slash_prop(prop_id, p.bond);
            return Ok(false);
        }
        self.insert_prop_to_active(prop_id, &mut p);
        Ok(true)
    }

    pub fn vote_whitelist(&mut self, payload: VotePayload) {
        let caller = env::predecessor_account_id();
        self.assert_whitelist(&caller);

        match self.vote_impl(
            caller,
            env::block_timestamp_ms() + MAX_DURATION + 1,
            payload,
        ) {
            Ok(_) => (),
            Err(err) => err.panic(),
        }
    }

    /// Must be called via `iah_registry.is_human_call_lock` with
    /// `lock_duration: self.vote_duration + 1`.
    #[payable]
    #[handle_result]
    pub fn vote(
        &mut self,
        caller: AccountId,
        locked_until: u64,
        #[allow(unused_variables)] iah_proof: Option<SBTs>,
        payload: VotePayload,
    ) -> Result<(), VoteError> {
        if env::predecessor_account_id() != self.accounts.get().unwrap().iah_registry {
            return Err(VoteError::NotIAHreg);
        }

        self.vote_impl(caller, locked_until, payload)
    }

    /// Allows anyone to execute or slash the proposal.
    /// If proposal is slasheable, the user who executes gets REMOVE_REWARD.
    #[handle_result]
    pub fn execute(&mut self, id: u32) -> Result<PromiseOrValue<ExecResponse>, ExecError> {
        let mut prop = self.proposals.get(&id).ok_or(ExecError::PropNotFound)?;
        // quick return, can only execute if the status was not switched yet, or it
        // failed (previous attempt to execute failed).
        if !matches!(
            prop.status,
            ProposalStatus::InProgress | ProposalStatus::Failed
        ) {
            return Err(ExecError::AlreadyFinalized);
        }

        prop.recompute_status(self.vote_duration, self.prop_consent(&prop));
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
        prop.executed_at = Some(env::block_timestamp_ms());
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
            PropKind::Text | PropKind::TextSuper | PropKind::ApproveBudget { .. } => (),
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
            PropKind::UpdateBonds {
                pre_vote_bond,
                active_queue_bond,
            } => {
                self.pre_vote_bond = pre_vote_bond.0;
                self.active_queue_bond = active_queue_bond.0;
            }
            PropKind::UpdateVoteDuration {
                pre_vote_duration,
                vote_duration,
            } => {
                self.pre_vote_duration = *pre_vote_duration;
                self.vote_duration = *vote_duration;
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

    /// Allows admin to udpate the consent based on the latest amount of humans verified accounts.
    pub fn admin_update_consent(&mut self, simple_consent: Consent, super_consent: Consent) {
        self.assert_admin();
        self.simple_consent = simple_consent;
        self.super_consent = super_consent;
    }

    /// Allows admin to add a user to the whitelist.
    pub fn admin_add_to_whitelist(&mut self, user: AccountId) {
        self.assert_admin();
        self.iom_whitelist.insert(user);
    }

    /// Allows admin to remove a user from the whitelist.
    pub fn admin_remove_from_whitelist(&mut self, user: AccountId) {
        self.assert_admin();
        self.iom_whitelist.remove(&user);
    }

    // /// udpate voting time for e2e tests purposes
    // /// TODO: remove
    // pub fn admin_update_durations(&mut self, pre_vote_duration: u64, vote_duration: u64) {
    //     self.assert_admin();
    //     require!(
    //         env::current_account_id().as_ref().contains("test"),
    //         "can only be run in test contracts"
    //     );

    //     self.pre_vote_duration = pre_vote_duration;
    //     self.vote_duration = vote_duration;
    // }

    /*****************
     * CALLBACKS
     ****************/

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
                let mut prop = self.proposals.get(&prop_id).expect("proposal not found");
                prop.status = ProposalStatus::Failed;
                prop.executed_at = None;
                self.proposals.insert(&prop_id, &prop);
                emit_executed(prop_id);
            }
        };
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

    fn assert_whitelist(&self, account_id: &AccountId) {
        require!(self.iom_whitelist.contains(account_id), "not whitelisted");
    }

    fn remove_pre_vote_prop(&mut self, id: u32) -> Result<Proposal, PrevoteError> {
        self.pre_vote_proposals
            .remove(&id)
            .ok_or(PrevoteError::NotFound)
    }

    fn assert_pre_vote_prop(&mut self, id: u32) -> Result<Proposal, PrevoteError> {
        self.pre_vote_proposals
            .get(&id)
            .ok_or(PrevoteError::NotFound)
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

    fn prop_consent(&self, prop: &Proposal) -> Consent {
        match prop.kind.required_consent() {
            ConsentKind::Simple => self.simple_consent.clone(),
            ConsentKind::Super => self.super_consent.clone(),
        }
    }

    fn add_vote(&mut self, prop_id: u32, user: AccountId, vote: Vote, prop: &mut Proposal) {
        match vote {
            Vote::Abstain => prop.abstain += 1,
            Vote::Approve => prop.approve += 1,
            Vote::Reject => prop.reject += 1,
            Vote::Spam => prop.spam += 1,
        };
        // allow to overwrite existing votes
        let v = VoteRecord {
            timestamp: env::block_timestamp_ms(),
            vote,
        };
        if let Some(old_vote) = self.votes.insert(&(prop_id, user), &v) {
            match old_vote.vote {
                Vote::Approve => prop.approve -= 1,
                Vote::Reject => prop.reject -= 1,
                Vote::Abstain => prop.abstain -= 1,
                Vote::Spam => prop.spam -= 1,
            }
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
    const VOTE_DURATION: u64 = 60 * 5 * 1000;
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

    fn vote_payload(prop_id: u32, vote: Vote) -> VotePayload {
        VotePayload { prop_id, vote }
    }

    fn vote_record(timestamp_ns: u64, vote: Vote) -> VoteRecord {
        VoteRecord {
            timestamp: timestamp_ns / MSECOND,
            vote,
        }
    }

    fn create_prop_payload(kind: PropKind, description: String) -> CreatePropPayload {
        CreatePropPayload { kind, description }
    }

    fn iah_proof() -> SBTs {
        vec![(iah_registry(), vec![1, 4])]
    }

    /// creates a test contract with proposal
    fn setup_ctr(attach_deposit: u128) -> (VMContext, Contract, u32) {
        let mut context = VMContextBuilder::new().build();
        let mut contract = Contract::new(
            PRE_VOTE_DURATION,
            VOTE_DURATION,
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

    fn min_vote_lock(ctx: &VMContext) -> u64 {
        ctx.block_timestamp / MSECOND + VOTE_DURATION + 1
    }

    fn min_prevote_lock(ctx: &VMContext) -> u64 {
        ctx.block_timestamp / MSECOND + PRE_VOTE_DURATION + 1
    }

    fn vote(mut ctx: VMContext, ctr: &mut Contract, accs: Vec<AccountId>, id: u32, vote: Vote) {
        for a in accs {
            ctx.predecessor_account_id = iah_registry();
            ctx.attached_deposit = VOTE_DEPOSIT;
            testing_env!(ctx.clone());
            let locked_until = min_vote_lock(&ctx);
            let res = ctr.vote(
                a.clone(),
                locked_until,
                None,
                vote_payload(id, vote.clone()),
            );
            assert_eq!(
                res,
                Ok(()),
                "\nacc {} _____ prop: {:?}",
                a,
                ctr.proposals.get(&id).unwrap(),
            );
        }
    }

    fn insert_vote(ctr: &mut Contract, prop_id: u32, voter: AccountId, timestamp_ns: u64, v: Vote) {
        ctr.votes
            .insert(&(prop_id, voter), &vote_record(timestamp_ns, v));
    }

    fn vote_and_fast_forward_status_check(
        ctx: &mut VMContext,
        ctr: &mut Contract,
        accs: Vec<AccountId>,
        id: u32,
        v: Vote,
        expected_status: ProposalStatus,
    ) {
        vote(ctx.clone(), ctr, accs, id, v);
        ctx.block_timestamp += (ctr.vote_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        let prop = ctr.get_proposal(id).unwrap();
        assert_eq!(prop.proposal.status, expected_status);
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
    fn basic_flows() {
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
        let locked = min_vote_lock(&ctx);
        match ctr.vote(acc(2), locked, None, vote_payload(id, Vote::Approve)) {
            Err(VoteError::Storage(_)) => (),
            x => panic!("expected Storage, got: {:?}", x),
        }

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
        ctx.block_timestamp += VOTE_DURATION / 2 * MSECOND;
        ctx.attached_deposit = ONE_NEAR / 10;
        testing_env!(ctx.clone());
        prop1 = ctr.get_proposal(id).unwrap();
        assert_eq!(prop1.proposal.status, ProposalStatus::InProgress);
        let locked = min_vote_lock(&ctx);
        assert_eq!(
            ctr.vote(acc(5), locked, None, vote_payload(id, Vote::Spam)),
            Ok(())
        );
        prop1.proposal.spam += 1;
        insert_vote(&mut ctr, id, acc(5), ctx.block_timestamp, Vote::Spam);
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
            ctr.vote(acc(3), locked, None, vote_payload(id, Vote::Approve)),
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
        insert_vote(&mut ctr, id, acc(3), ctx.block_timestamp, Vote::Approve);
        insert_vote(&mut ctr, id, acc(1), ctx.block_timestamp, Vote::Reject);
        insert_vote(&mut ctr, id, acc(2), ctx.block_timestamp, Vote::Reject);

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
        vote_and_fast_forward_status_check(
            &mut ctx,
            &mut ctr,
            vec![acc(1), acc(2)],
            id,
            Vote::Approve,
            ProposalStatus::Rejected,
        );

        //
        // enough approve votes, but more reject votes.
        let id = create_proposal(ctx.clone(), &mut ctr, BOND);
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );
        vote_and_fast_forward_status_check(
            &mut ctx,
            &mut ctr,
            vec![acc(10), acc(11), acc(12)],
            id,
            Vote::Reject,
            ProposalStatus::Rejected,
        );

        //
        // enough approve votes, but same amount of reject + spam votes.
        let id = create_proposal(ctx.clone(), &mut ctr, BOND);
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );
        vote(ctx.clone(), &mut ctr, vec![acc(4)], id, Vote::Spam);
        vote_and_fast_forward_status_check(
            &mut ctx,
            &mut ctr,
            vec![acc(10), acc(11)],
            id,
            Vote::Reject,
            ProposalStatus::Rejected,
        );

        //
        // enough approve votes, but more reject + spam votes.
        let id = create_proposal(ctx.clone(), &mut ctr, BOND);
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );
        vote(ctx.clone(), &mut ctr, vec![acc(4)], id, Vote::Reject);
        vote_and_fast_forward_status_check(
            &mut ctx,
            &mut ctr,
            vec![acc(10), acc(11)],
            id,
            Vote::Spam,
            ProposalStatus::Spam,
        );
    }

    #[test]
    fn proposal_overdue() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        ctx.block_timestamp = START + (ctr.vote_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        let locked = min_vote_lock(&ctx);
        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id, Vote::Approve)),
            Err(VoteError::Timeout)
        );
    }

    #[test]
    fn vote_not_found() {
        let (ctx, mut ctr, id) = setup_ctr(PRE_BOND);
        // proposal is in pre-vote queue, so should not be found in the active queue
        let locked = min_vote_lock(&ctx);
        match ctr.vote(acc(1), locked, None, vote_payload(id, Vote::Approve)) {
            Err(err) => assert_eq!(err, VoteError::PropNotFound),
            Ok(_) => panic!("expect PropNotFound, got: Ok"),
        }

        match ctr.vote(acc(1), locked, None, vote_payload(999, Vote::Approve)) {
            Err(err) => assert_eq!(err, VoteError::PropNotFound),
            Ok(_) => panic!("expect PropNotFound, got: Ok"),
        }
    }

    #[test]
    fn execute_not_active() {
        let (_, mut ctr, id) = setup_ctr(PRE_BOND);
        match ctr.execute(id) {
            Err(ExecError::PropNotFound) => (),
            Err(err) => panic!("expect PropNotFound, got: {:?}", err),
            Ok(_) => panic!("expect PropNotFound, got: Ok"),
        }
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

        ctx.block_timestamp = START + ctr.vote_duration / 2 * MSECOND;
        testing_env!(ctx.clone());
        let mut p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::InProgress);
        match ctr.execute(id) {
            Ok(_) => panic!("expected InProgress, got: OK"),
            Err(err) => assert_eq!(err, ExecError::InProgress),
        }

        // fast forward to voting overtime
        ctx.block_timestamp = START + (ctr.vote_duration + 1) * MSECOND;
        testing_env!(ctx.clone());
        assert!(matches!(
            ctr.execute(id),
            Ok(PromiseOrValue::Value(ExecResponse::Executed))
        ));
        p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::Executed);
        assert_eq!(p.proposal.executed_at, Some(ctx.block_timestamp / MSECOND));

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

        ctx.block_timestamp += (ctr.vote_duration + 1) * MSECOND * 10;
        testing_env!(ctx.clone());
        match ctr.execute(id) {
            Ok(PromiseOrValue::Value(ExecResponse::Slashed)) => (),
            Ok(_) => panic!("expected Ok(ExecResponse:Slashed)"),
            Err(err) => panic!("expected Ok(ExecResponse:Slashed), got: Err {:?}", err),
        }

        assert_eq!(ctr.get_proposal(id), None);
        // second execute should return AlreadySlashed
        match ctr.execute(id) {
            Ok(_) => panic!("expected Err(ExecError::PropNotFound)"),
            Err(err) => assert_eq!(err, ExecError::PropNotFound),
        }
    }

    #[test]
    fn execution_update_bonds() {
        let (mut ctx, mut ctr, _) = setup_ctr(PRE_BOND);
        ctx.attached_deposit = BOND;
        testing_env!(ctx.clone());
        let id = ctr
            .create_proposal(
                acc(1),
                iah_proof(),
                CreatePropPayload {
                    kind: PropKind::UpdateBonds {
                        pre_vote_bond: (PRE_BOND * 2).into(),
                        active_queue_bond: (BOND * 5).into(),
                    },
                    description: "updating bonds".to_owned(),
                },
            )
            .unwrap();
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );

        ctx.predecessor_account_id = acc(10);
        ctx.block_timestamp += ctr.vote_duration * 10 * MSECOND;
        testing_env!(ctx.clone());

        match ctr.execute(id) {
            Ok(_) => (),
            Err(err) => panic!("expected OK, got: {:?}", err),
        }
        let p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::Executed);
        assert_eq!(p.proposal.executed_at, Some(ctx.block_timestamp / MSECOND));
        assert_eq!(ctr.pre_vote_bond, PRE_BOND * 2);
        assert_eq!(ctr.active_queue_bond, BOND * 5);
    }

    #[test]
    fn execution_update_vote_duration() {
        let (mut ctx, mut ctr, _) = setup_ctr(PRE_BOND);
        ctx.attached_deposit = BOND;
        testing_env!(ctx.clone());
        let id = ctr
            .create_proposal(
                acc(1),
                iah_proof(),
                CreatePropPayload {
                    kind: PropKind::UpdateVoteDuration {
                        pre_vote_duration: MIN_DURATION,
                        vote_duration: MAX_DURATION,
                    },
                    description: "updating voting duration".to_owned(),
                },
            )
            .unwrap();
        vote(
            ctx.clone(),
            &mut ctr,
            vec![acc(1), acc(2), acc(3)],
            id,
            Vote::Approve,
        );

        ctx.predecessor_account_id = acc(10);
        ctx.block_timestamp += ctr.vote_duration * 10 * MSECOND;
        testing_env!(ctx.clone());

        match ctr.execute(id) {
            Ok(_) => (),
            Err(err) => panic!("expected OK, got: {:?}", err),
        }
        let p = ctr.get_proposal(id).unwrap();
        assert_eq!(p.proposal.status, ProposalStatus::Executed);
        assert_eq!(p.proposal.executed_at, Some(ctx.block_timestamp / MSECOND));
        assert_eq!(ctr.pre_vote_duration, MIN_DURATION);
        assert_eq!(ctr.vote_duration, MAX_DURATION);
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
        ctx.block_timestamp = START + (ctr.vote_duration + 1) * MSECOND;
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
        ctx.block_timestamp = (p.proposal.start + ctr.vote_duration + 1) * MSECOND;
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
            vote_duration: VOTE_DURATION,
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

        ctx.attached_deposit = VOTE_DEPOSIT;
        testing_env!(ctx.clone());
        let locked = min_vote_lock(&ctx);
        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id, Vote::Approve)),
            Ok(())
        );
        p.proposal.approve = 1;
        insert_vote(&mut ctr, id, acc(1), ctx.block_timestamp, Vote::Approve);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id, Vote::Abstain)),
            Ok(())
        );
        p.proposal.approve = 0;
        p.proposal.abstain = 1;
        insert_vote(&mut ctr, id, acc(1), ctx.block_timestamp, Vote::Abstain);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id, Vote::Reject)),
            Ok(())
        );
        p.proposal.abstain = 0;
        p.proposal.reject = 1;
        insert_vote(&mut ctr, id, acc(1), ctx.block_timestamp, Vote::Reject);
        assert_eq!(ctr.get_proposal(id).unwrap(), p);

        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id, Vote::Spam)),
            Ok(())
        );
        p.proposal.reject = 0;
        p.proposal.spam = 1;
        insert_vote(&mut ctr, id, acc(1), ctx.block_timestamp, Vote::Spam);
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
        let locked = min_prevote_lock(&ctx);
        for i in 1..PRE_VOTE_SUPPORT {
            assert_eq!(
                ctr.support_proposal(acc(i as u8), locked, None, id),
                Ok(true)
            );
        }

        assert_eq!(
            ctr.support_proposal(acc(1), locked, None, id),
            Err(PrevoteError::DoubleSupport)
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
        let locked = min_prevote_lock(&ctx);
        assert_eq!(
            ctr.support_proposal(acc(PRE_VOTE_SUPPORT as u8), locked, None, id),
            Ok(true)
        );

        // should be removed from prevote queue
        assert_eq!(ctr.assert_pre_vote_prop(id), Err(PrevoteError::NotFound));
        let p = ctr.proposals.get(&id).unwrap();
        assert_eq!(p.status, ProposalStatus::InProgress);
        assert_eq!(p.support, PRE_VOTE_SUPPORT);
        assert_eq!(p.start, ctx.block_timestamp / MSECOND);
        assert!(p.supported.is_empty());

        // can't support proposal which was already moved
        assert_eq!(
            ctr.support_proposal(acc(1), locked, None, id),
            Err(PrevoteError::NotFound)
        );

        //
        // Should not be able to support an overdue proposal
        //
        let id = create_proposal(ctx.clone(), &mut ctr, PRE_BOND);
        ctx.block_timestamp += (PRE_VOTE_DURATION + 1) * MSECOND;
        testing_env!(ctx.clone());
        let locked = min_prevote_lock(&ctx);
        assert_eq!(ctr.support_proposal(acc(1), locked, None, id), Ok(false));
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

    #[test]
    fn update_white_list() {
        let (mut ctx, mut ctr, _) = setup_ctr(BOND);
        ctx.predecessor_account_id = admin();
        testing_env!(ctx.clone());

        assert_eq!(ctr.is_iom_whitelisted(&acc(1)), false);
        ctr.admin_add_to_whitelist(acc(1));
        assert_eq!(ctr.is_iom_whitelisted(&acc(1)), true);
        ctr.admin_remove_from_whitelist(acc(1));
        assert_eq!(ctr.is_iom_whitelisted(&acc(1)), false);
    }

    #[test]
    fn whitelisted_can_vote() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        ctx.predecessor_account_id = admin();
        testing_env!(ctx.clone());
        ctr.admin_add_to_whitelist(acc(1));
        ctx.predecessor_account_id = acc(1);
        ctx.attached_deposit = VOTE_DEPOSIT;
        testing_env!(ctx.clone());
        ctr.vote_whitelist(vote_payload(id, Vote::Approve));
    }

    #[test]
    fn not_called_by_iah_registry() {
        let (mut ctx, mut ctr, id) = setup_ctr(BOND);
        let locked = min_vote_lock(&ctx);
        ctx.predecessor_account_id = acc(1);
        testing_env!(ctx);
        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id, Vote::Approve)),
            Err(VoteError::NotIAHreg)
        );

        let resp = ctr.create_proposal(
            acc(1),
            iah_proof(),
            create_prop_payload(PropKind::Text, "Proposal unit test".to_string()),
        );
        assert_eq!(resp, Err(CreatePropError::NotIAHreg));

        let resp = ctr.support_proposal(acc(1), locked, None, id);
        assert_eq!(resp, Err(PrevoteError::NotIAHreg));
    }

    #[test]
    fn iah_lock_not_enough() {
        let (ctx, mut ctr, id) = setup_ctr(BOND);
        let locked = min_vote_lock(&ctx) - 1;
        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id, Vote::Approve)),
            Err(VoteError::LockedUntil)
        );

        let locked = min_prevote_lock(&ctx) - 1;
        let id2 = create_proposal(ctx, &mut ctr, PRE_BOND);
        let resp = ctr.support_proposal(acc(1), locked, None, id2);
        assert_eq!(resp, Err(PrevoteError::LockedUntil));
    }

    #[test]
    fn support_proposal_by_congress() {
        let (_, mut ctr, id) = setup_ctr(PRE_BOND);

        match ctr.support_proposal_by_congress(id, iah_registry()) {
            Err(PrevoteError::NotCongress) => (),
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
            Err(PrevoteError::NotCongressMember)
        );
        assert_eq!(
            ctr.on_support_by_congress(Err(near_sdk::PromiseError::Failed), id),
            Err(PrevoteError::NotCongressMember)
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
        assert_eq!(ctr.assert_pre_vote_prop(id), Err(PrevoteError::NotFound));
        // modify prop to expected values and see if it equals the stored one
        prop.proposal.status = ProposalStatus::InProgress;
        prop.proposal.start += 1; // start is in milliseconds
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
                CreatePropError::BadRequest("receiver_id can't be a congress house, use a specific proposal to interact with the congress".to_string())
            ),
        }
    }

    #[test]
    fn get_pre_vote_proposals() {
        let (ctx, mut ctr, _) = setup_ctr(BOND);
        create_proposal(ctx.clone(), &mut ctr, PRE_BOND);
        create_proposal(ctx.clone(), &mut ctr, PRE_BOND);
        let active_proposals = ctr.get_proposals(0, 10, None);
        assert_eq!(active_proposals.len(), 1);
        let pre_vote_proposals = ctr.get_pre_vote_proposals(0, 10, None);
        assert_eq!(pre_vote_proposals.len(), 2);
    }

    #[test]
    fn vote_map() {
        let (ctx, mut ctr, id1) = setup_ctr(BOND);
        let id2 = create_proposal(ctx.clone(), &mut ctr, BOND);
        let locked = min_vote_lock(&ctx);

        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id1, Vote::Approve)),
            Ok(())
        );
        assert_eq!(
            ctr.vote(acc(1), locked, None, vote_payload(id2, Vote::Reject)),
            Ok(())
        );
        assert_eq!(
            ctr.vote(acc(2), locked, None, vote_payload(id2, Vote::Spam)),
            Ok(())
        );

        let now = ctx.block_timestamp;
        let get = |prop_id, acc| ctr.votes.get(&(prop_id, acc)).unwrap();
        assert_eq!(get(id1, acc(1)), vote_record(now, Vote::Approve));
        assert_eq!(get(id2, acc(1)), vote_record(now, Vote::Reject));
        assert_eq!(get(id2, acc(2)), vote_record(now, Vote::Spam));
    }

    #[test]
    fn check_serialization() {
        assert_eq!(
            serde_json::to_string(&PropKind::Text {}).unwrap(),
            "\"Text\"".to_string()
        );

        let k = PropKind::Dismiss {
            dao: coa(),
            member: acc(1),
        };
        assert_eq!(
            serde_json::to_string(&k).unwrap(),
            "{\"Dismiss\":{\"dao\":\"coa.near\",\"member\":\"user-1.near\"}}".to_string()
        );

        let k = PropKind::Veto {
            dao: hom(),
            prop_id: 12,
        };
        assert_eq!(
            serde_json::to_string(&k).unwrap(),
            "{\"Veto\":{\"dao\":\"hom.near\",\"prop_id\":12}}".to_string()
        );
    }
}
