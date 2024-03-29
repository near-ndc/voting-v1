use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::{Base64VecU8, U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, AccountId, Balance, Promise};

use std::collections::HashSet;

use crate::{PrevoteError, SLASH_REWARD};

/// Consent sets the conditions for vote to pass. It specifies a quorum (minimum amount of
/// accounts that have to vote and the approval threshold (% of #approve votes) for a proposal
/// to pass.
#[derive(BorshSerialize, BorshDeserialize, Deserialize, Serialize, Clone)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, PartialEq, Copy))]
pub struct Consent {
    pub quorum: u32,
    /// percentage value
    pub threshold: u8,
}

impl Consent {
    pub fn verify(&self) -> bool {
        self.threshold <= 100
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
pub enum ConsentKind {
    Simple,
    Super,
}

/// Proposals that are sent to this DAO.
#[derive(BorshSerialize, BorshDeserialize, Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(
    all(test, not(target_arch = "wasm32")),
    derive(Debug, PartialEq, Clone)
)]
pub struct Proposal {
    /// Original proposer.
    pub proposer: AccountId,
    /// original bond, used to cover the storage for all votes
    pub bond: Balance,
    pub(crate) additional_bond: Option<(AccountId, Balance)>,
    /// Description of this proposal.
    pub description: String,
    /// Kind of proposal with relevant information.
    pub kind: PropKind,
    /// Current status of the proposal.
    pub status: ProposalStatus,
    pub approve: u32,
    pub reject: u32,
    pub spam: u32,
    pub abstain: u32,
    pub support: u32,
    pub supported: HashSet<AccountId>,
    /// start time (for voting period).
    pub start: u64,
    /// Unix time in milliseconds when the proposal was executed. `None` if it is not approved
    /// or execution failed.
    pub executed_at: Option<u64>,
    /// Proposal storage cost (excluding vote)
    pub(crate) proposal_storage: u128,
}

impl Proposal {
    pub fn add_support(&mut self, user: AccountId) -> Result<(), PrevoteError> {
        if self.supported.contains(&user) {
            return Err(PrevoteError::DoubleSupport);
        }
        self.support += 1;
        self.supported.insert(user);
        Ok(())
    }

    pub fn is_active(&self, vote_duration: u64) -> bool {
        env::block_timestamp_ms() <= self.start + vote_duration
    }

    pub fn recompute_status(&mut self, vote_duration: u64, consent: Consent) {
        // still in progress or already finalzied
        if self.is_active(vote_duration) || self.status != ProposalStatus::InProgress {
            return;
        }
        let total_no = self.reject + self.spam;
        let qualified = self.approve + total_no;

        // check if we have quorum
        if qualified + self.abstain < consent.quorum {
            self.status = ProposalStatus::Rejected;
            return;
        }

        if self.approve > qualified * consent.threshold as u32 / 100 {
            self.status = ProposalStatus::Approved;
        } else if self.spam > self.reject
            && total_no >= qualified * (100 - consent.threshold) as u32 / 100
        {
            self.status = ProposalStatus::Spam;
        } else {
            self.status = ProposalStatus::Rejected;
        }
    }

    /// Refund after voting period is over
    pub fn refund_bond(&mut self) -> bool {
        if self.bond == 0 {
            return false;
        }

        // Vote storage is already paid by voters. We only keep storage for proposal.
        let refund = self.bond - self.proposal_storage;
        Promise::new(self.proposer.clone()).transfer(refund);
        if let Some((account, amount)) = &self.additional_bond {
            Promise::new(account.clone()).transfer(*amount);
        }
        self.bond = 0;
        self.additional_bond = None;
        true
    }

    /// returns false if there is nothing to slash.
    pub fn slash_bond(&mut self, treasury: AccountId) -> bool {
        if self.bond == 0 {
            return false;
        }
        let mut bond = self.bond - self.proposal_storage;
        if let Some((_, amount)) = self.additional_bond {
            bond += amount;
        }
        let reward = if bond >= SLASH_REWARD {
            Promise::new(treasury).transfer(bond - SLASH_REWARD);
            SLASH_REWARD
        } else {
            bond
        };
        Promise::new(env::predecessor_account_id()).transfer(reward);
        self.bond = 0;
        self.additional_bond = None;
        true
    }
}

/// Kinds of proposals, doing different action.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone, PartialEq))]
#[serde(crate = "near_sdk::serde")]
pub enum PropKind {
    Dismiss {
        dao: AccountId,
        member: AccountId,
    },
    Dissolve {
        dao: AccountId,
    },
    Veto {
        dao: AccountId,
        prop_id: u32,
    },
    ApproveBudget {
        dao: AccountId,
        prop_id: u32,
    },
    /// A default, text based proposal.
    /// NewBudget, UpdateBudget are modelled using Text.
    // NOTE: In Sputnik, this variant kind is called `Vote`
    Text,
    /// Same as the `Text` proposal, but requires the Super Consent to approve.
    TextSuper,
    /// Calls `receiver_id` with list of method names in a single promise.
    /// Allows this contract to execute any arbitrary set of actions in other contracts
    /// except for congress contracts.
    FunctionCall {
        receiver_id: AccountId,
        actions: Vec<ActionCall>,
    },
    UpdateBonds {
        pre_vote_bond: U128,
        active_queue_bond: U128,
    },
    UpdateVoteDuration {
        pre_vote_duration: u64,
        vote_duration: u64,
    },
}

impl PropKind {
    /// name of the kind
    pub fn to_name(&self) -> String {
        match self {
            PropKind::Dismiss { .. } => "dismiss".to_string(),
            PropKind::Dissolve { .. } => "dissolve".to_string(),
            PropKind::Veto { .. } => "veto".to_string(),
            PropKind::ApproveBudget { .. } => "approve-budget".to_string(),
            PropKind::Text => "text".to_string(),
            PropKind::TextSuper => "text super consent".to_string(),
            PropKind::FunctionCall { .. } => "function call".to_string(),
            PropKind::UpdateBonds { .. } => "config: update bonds".to_string(),
            PropKind::UpdateVoteDuration { .. } => "config: update voting duration".to_string(),
        }
    }

    pub fn required_consent(&self) -> ConsentKind {
        match self {
            Self::Dismiss { .. }
            | Self::Veto { .. }
            | Self::ApproveBudget { .. }
            | Self::Text
            | Self::FunctionCall { .. }
            | Self::UpdateBonds { .. }
            | Self::UpdateVoteDuration { .. } => ConsentKind::Simple,
            Self::Dissolve { .. } | Self::TextSuper => ConsentKind::Super,
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone))]
pub enum ProposalStatus {
    PreVote,
    InProgress,
    Approved,
    Rejected,
    /// Spam is a tempral status when set when the proposal reached spam threshold and will be
    /// removed.
    Spam,
    Executed,
    /// If proposal has failed when executing. Allowed to re-finalize again to either expire or
    /// approved.
    Failed,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, PartialEq))]
pub struct VoteRecord {
    pub timestamp: u64, // unix time of when this vote was submitted
    pub vote: Vote,
}

/// Votes recorded in the proposal.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Clone, Debug, PartialEq))]
#[serde(crate = "near_sdk::serde")]
pub enum Vote {
    Approve = 0x0,
    Reject = 0x1,
    /// Spam vote indicates that the proposal creates a spam, must be removed and the bond
    /// slashed.
    Spam = 0x2,
    Abstain = 0x3,
    // note: we don't have Remove, we use Spam.
}

/// Function call arguments.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone, PartialEq))]
#[serde(crate = "near_sdk::serde")]
pub struct ActionCall {
    pub method_name: String,
    pub args: Base64VecU8,
    pub deposit: U128,
    pub gas: U64,
}
