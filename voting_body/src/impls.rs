use super::*;

impl Contract {
    /**********
     * TRANSACTIONS
     **********/

    /// Creates a new proposal.
    /// Returns the new proposal ID.
    /// Caller is required to attach enough deposit to cover the proposal storage as well as all
    /// possible votes.
    /// NOTE: storage is paid from the bond.
    /// Panics when the FunctionCall is trying to call any of the congress contracts.
    pub fn create_proposal_impl(
        &mut self,
        caller: AccountId,
        payload: CreatePropPayload,
    ) -> Result<u32, CreatePropError> {
        let storage_start = env::storage_usage();
        let now = env::block_timestamp_ms();
        let bond = env::attached_deposit();

        if bond < self.pre_vote_bond {
            return Err(CreatePropError::MinBond);
        }

        // validate proposals
        match &payload.kind {
            PropKind::FunctionCall { receiver_id, .. } => {
                let accounts = self.accounts.get().unwrap();
                if *receiver_id == accounts.congress_coa
                    || *receiver_id == accounts.congress_hom
                    || *receiver_id == accounts.congress_tc
                {
                    return Err(CreatePropError::BadRequest(
                    "receiver_id can't be a congress house, use a specific proposal to interact with the congress".to_string(),
                ));
                }
            }
            PropKind::UpdateVoteDuration {
                pre_vote_duration,
                vote_duration,
            } => {
                if *pre_vote_duration < MIN_DURATION
                    || *vote_duration < MIN_DURATION
                    || *pre_vote_duration > MAX_DURATION
                    || *vote_duration > MAX_DURATION
                {
                    return Err(CreatePropError::BadRequest(
                    "receiver_id can't be a congress house, use a specific proposal to interact with the congress".to_string(),
                ));
                }
            }
            _ => (),
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
            start: now,
            executed_at: None,
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

    /// Supports proposal in the pre-vote queue.
    /// Returns false if the proposal can't be supported because it is overdue.
    /// `lock_duration: self.pre_vote_duration + 1`.
    /// `payload` must be a pre-vote proposal ID.
    pub fn support_proposal_impl(
        &mut self,
        caller: AccountId,
        locked_until: u64,
        payload: u32,
    ) -> Result<bool, PrevoteError> {
        let prop_id = payload;
        let mut p = self.assert_pre_vote_prop(prop_id)?;
        let now = env::block_timestamp_ms();
        if now - p.start > self.pre_vote_duration {
            self.slash_prop(prop_id, p.bond);
            self.pre_vote_proposals.remove(&prop_id);
            return Ok(false);
        }
        if locked_until <= p.start + self.pre_vote_duration {
            return Err(PrevoteError::LockedUntil);
        }

        p.add_support(caller)?;
        if p.support >= self.pre_vote_support {
            self.pre_vote_proposals.remove(&prop_id);
            self.insert_prop_to_active(prop_id, &mut p);
        } else {
            self.pre_vote_proposals.insert(&prop_id, &p);
        }
        Ok(true)
    }

    pub fn vote_impl(
        &mut self,
        caller: AccountId,
        locked_until: u64,
        payload: VotePayload,
    ) -> Result<(), VoteError> {
        let storage_start = env::storage_usage();
        let mut prop = self
            .proposals
            .get(&payload.prop_id)
            .ok_or(VoteError::PropNotFound)?;
        if !matches!(prop.status, ProposalStatus::InProgress) {
            return Err(VoteError::NotInProgress);
        }
        if !prop.is_active(self.vote_duration) {
            return Err(VoteError::Timeout);
        }
        if locked_until <= prop.start + self.vote_duration {
            return Err(VoteError::LockedUntil);
        }

        self.add_vote(payload.prop_id, caller.clone(), payload.vote, &mut prop);
        // NOTE: we can't quickly set a status to a finalized one because we don't know the total number of
        // voters

        self.proposals.insert(&payload.prop_id, &prop);
        emit_vote(payload.prop_id);

        if let Err(reason) = finalize_storage_check(storage_start, 0, caller) {
            return Err(VoteError::Storage(reason));
        }
        Ok(())
    }
}
