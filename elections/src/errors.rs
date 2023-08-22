use near_sdk::env::panic_str;
use near_sdk::FunctionError;

use crate::TokenId;

/// Contract errors
#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum VoteError {
    WrongIssuer,
    NoSBTs,
    DuplicateCandidate,
    DoubleVote(TokenId),
    NoBond,
}

impl FunctionError for VoteError {
    fn panic(&self) -> ! {
        match self {
            VoteError::WrongIssuer => {
                panic_str("expected human SBTs proof from the human issuer only")
            }
            VoteError::NoSBTs => panic_str("voter is not a verified human"),
            VoteError::DuplicateCandidate => panic_str("double vote for the same candidate"),
            VoteError::DoubleVote(sbt) => {
                panic_str(&format!("user already voted with sbt={}", sbt))
            },
            VoteError::NoBond => panic_str("bond doesn't exist"),
        }
    }
}

/// Contract errors
#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum RevokeVoteError {
    NotActive,
    NotVoted,
}

impl FunctionError for RevokeVoteError {
    fn panic(&self) -> ! {
        match self {
            RevokeVoteError::NotActive => {
                panic_str("can only revoke votes between proposal start and (end time + cooldown)")
            }
            RevokeVoteError::NotVoted => panic_str(
                "voter did not vote on this proposal or the vote has been already revoked",
            ),
        }
    }
}
