use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::{Base64VecU8, U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, AccountId, Balance};

use std::collections::HashMap;

use crate::VoteError;

/// Proposal that are sent to this DAO.
#[derive(BorshSerialize, BorshDeserialize, Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Deserialize, Debug, PartialEq))]
pub struct Proposal {
    /// Original proposer.
    pub proposer: AccountId,
    /// Description of this proposal.
    pub description: String,
    /// Kind of proposal with relevant information.
    pub kind: PropKind,
    /// Current status of the proposal.
    pub status: ProposalStatus,
    /// Sum of approval votes. Note: contract assumes that max amount of members is 255
    pub approve: u8,
    /// Sum of rejection votes. Note: contract assumes that max amount of members is 255
    pub reject: u8,
    /// Map of who voted and how.
    pub votes: HashMap<AccountId, Vote>,
    /// Submission time (for voting period).
    pub submission_time: u64,
    /// Unix time in miliseconds when the proposal reached approval threshold. `None` if it is not approved.
    pub approved_at: Option<u64>,
}

impl Proposal {
    pub fn add_vote(
        &mut self,
        user: AccountId,
        vote: Vote,
        threshold: u8,
    ) -> Result<(), VoteError> {
        if self.votes.contains_key(&user) {
            return Err(VoteError::DoubleVote);
        }
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
                if self.reject >= threshold {
                    self.status = ProposalStatus::Rejected;
                }
            }
        }
        self.votes.insert(user, vote);
        Ok(())
    }
}

/// Kinds of proposals, doing different action.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, PartialEq)]
#[serde(crate = "near_sdk::serde")]
pub enum PropKind {
    /// Calls `receiver_id` with list of method names in a single promise.
    /// Allows this contract to execute any arbitrary set of actions in other contracts.
    FunctionCall {
        receiver_id: AccountId,
        actions: Vec<ActionCall>,
    },
    /// a default, text based proposal.
    /// Note: NewBudget, UpdateBudge are modelled using Text.
    // NOTE: In Sputnik, this variant kind is called `Vote`
    Text,
    /// Single funding request.
    FundingRequest(Balance),
    /// Funding request that will renew every month until the end of the terms. The balance
    /// parameter is the size of the single month spending for this funding request.
    RecurrentFundingRequest(Balance),
    // TODO: support self upgrade.
    // /// Upgrade this contract with given hash from blob store.
    // UpgradeSelf { hash: Base58CryptoHash },
    // A proposal to remove the member from their role and ban them from future participation.
    MotionRemoveAndBan {
        member: AccountId,
        receiver_id: AccountId,
    },
    MotionRetain(AccountId),
}

impl PropKind {
    pub fn required_perm(&self) -> PropPerm {
        match self {
            PropKind::FunctionCall { .. } => PropPerm::FunctionCall,
            PropKind::Text { .. } => PropPerm::Text,
            PropKind::FundingRequest { .. } => PropPerm::FundingRequest,
            PropKind::RecurrentFundingRequest { .. } => PropPerm::RecurrentFundingRequest,
            PropKind::MotionRemoveAndBan{ .. } => PropPerm::MotionRemoveAndBan,
            PropKind::MotionRetain{ .. } => PropPerm::MotionRetain,
        }
    }

    /// name of the kind
    pub fn to_name(&self) -> String {
        match self {
            PropKind::FunctionCall { .. } => "function-call".to_string(),
            PropKind::Text { .. } => "text".to_string(),
            PropKind::FundingRequest { .. } => "funding-request".to_string(),
            PropKind::RecurrentFundingRequest { .. } => "recurrent-funding-request".to_string(),
            PropKind::MotionRemoveAndBan { .. } => "motion-remove-and-ban".to_string(),
            PropKind::MotionRetain { .. } => "motion-retain".to_string(),
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
pub enum ProposalStatus {
    InProgress,
    Approved,
    Rejected,
    Executed,
    /// If proposal has failed when executing. Allowed to re-finalize again to either expire or approved.
    Failed,
    // note: In Astra++ we have also: Removed nor Moved
    Vetoed,
}

/// Votes recorded in the proposal.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Clone, Debug, PartialEq))]
#[serde(crate = "near_sdk::serde")]
pub enum Vote {
    Approve = 0x0,
    Reject = 0x1,
    // note: we don't have Remove
}

/// Function call arguments.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, PartialEq)]
#[serde(crate = "near_sdk::serde")]
pub struct ActionCall {
    pub method_name: String,
    pub args: Base64VecU8,
    pub deposit: U128,
    pub gas: U64,
}

/// Permissions for creating proposals. See PropposalKind for more information.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(Debug, Clone))]
#[serde(crate = "near_sdk::serde")]
pub enum PropPerm {
    FunctionCall,
    Text,
    FundingRequest,
    RecurrentFundingRequest,
    MotionRemoveAndBan,
    MotionRetain,
}

/// Permissions for calling hooks
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq, Clone)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
#[serde(crate = "near_sdk::serde")]
pub enum HookPerm {
    /// Allows to veto any proposal kind
    VetoAll,
    /// Allows to veto only big funding requests or recurrent funding requests
    VetoBigOrReccurentFundingReq,
    Dismiss,
    Dissolve,
}
