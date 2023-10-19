use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::{Base64VecU8, U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, AccountId, Balance};

use std::collections::HashMap;

use crate::VoteError;

/// Consent sets the conditions for vote to pass. It specifies a quorum (minimum amount of
/// accounts that have to vote and the approval threshold (% of #approve votes) for a proposal
/// to pass.
#[derive(BorshSerialize, BorshDeserialize, Deserialize, Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, PartialEq, Clone))]
pub struct Consent {
    pub quorum: u32,
    pub threshold: u16,
}

/// Proposals that are sent to this DAO.
#[derive(BorshSerialize, BorshDeserialize, Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(
    not(target_arch = "wasm32"),
    derive(Deserialize, Debug, PartialEq, Clone)
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
    /// Map of who voted and how.
    // TODO: must not be a hashmap
    pub votes: HashMap<AccountId, Vote>,
    /// start time (for voting period).
    pub start: u64,
    /// Unix time in miliseconds when the proposal reached approval threshold. `None` if it is not approved.
    pub approved_at: Option<u64>,
}

impl Proposal {
    pub fn add_vote(
        &mut self,
        user: AccountId,
        vote: Vote,
        threshold: u32,
        //TODO: quorum
    ) -> Result<(), VoteError> {
        // allow to overwrite existing votes
        if let Some(old_vote) = self.votes.get(&user) {
            match old_vote {
                Vote::Approve => self.approve -= 1,
                Vote::Reject => self.reject -= 1,
                Vote::Abstain => self.abstain -= 1,
                Vote::Spam => self.spam -= 1,
            }
        }
        // TODO: this have to be fixed:
        // + threshold must not change the status. If threshold is smaller than 50% of eligible voters,
        //   then it may happen that we reach threshold, even though the rest of the voters are able to
        //   change the voting direction!
        // + need to integrate quorum

        match vote {
            Vote::Approve => {
                self.approve += 1;
                if self.approve >= threshold {
                    self.status = ProposalStatus::Approved;
                    self.approved_at = Some(env::block_timestamp_ms());
                }
            }
            Vote::Reject => {
                self.reject += 1;
                if self.reject + self.spam >= threshold {
                    if self.reject > self.spam {
                        self.status = ProposalStatus::Rejected;
                    } else {
                        self.status = ProposalStatus::Spam;
                    }
                }
            }
            Vote::Abstain => {
                self.abstain += 1;
                // TODO
            }
            Vote::Spam => {
                self.spam += 1;
                if self.reject + self.spam >= threshold {
                    if self.spam > self.reject {
                        self.status = ProposalStatus::Spam;
                    } else {
                        self.status = ProposalStatus::Rejected;
                    }
                }
                // TODO: remove proposal and slash bond
            }
        }
        self.votes.insert(user, vote);
        Ok(())
    }

    pub fn recompute_status(&mut self, voting_duration: u64) {
        if &self.status == &ProposalStatus::InProgress
            && env::block_timestamp_ms() > self.start + voting_duration
        {
            self.status = ProposalStatus::Rejected;
        }
    }
}

/// Kinds of proposals, doing different action.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone))]
#[serde(crate = "near_sdk::serde")]
pub enum PropKind {
    /// Calls `receiver_id` with list of method names in a single promise.
    /// Allows this contract to execute any arbitrary set of actions in other contracts.
    FunctionCall {
        receiver_id: AccountId,
        actions: Vec<ActionCall>,
    },
    /// A default, text based proposal.
    /// NewBudget, UpdateBudget are modelled using Text.
    // NOTE: In Sputnik, this variant kind is called `Vote`
    Text,
}

impl PropKind {
    /// name of the kind
    pub fn to_name(&self) -> String {
        match self {
            PropKind::FunctionCall { .. } => "function-call".to_string(),
            PropKind::Text { .. } => "text".to_string(),
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
    Vetoed,
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
    // note: we don't have Remove
}

/// Function call arguments.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, Clone))]
#[serde(crate = "near_sdk::serde")]
pub struct ActionCall {
    pub method_name: String,
    pub args: Base64VecU8,
    pub deposit: U128,
    pub gas: U64,
}
