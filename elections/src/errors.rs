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
    MinBond(u128, u128),
    Blacklisted,
    NoBond
}

impl FunctionError for VoteError {
    fn panic(&self) -> ! {
        match self {
            VoteError::WrongIssuer => {
                panic_str("expected human SBTs proof from the human issuer only")
            }
            VoteError::NoSBTs => panic_str("voter is not a verified human, expected IAH SBTs proof from the IAH issuer only"),
            VoteError::DuplicateCandidate => panic_str("double vote for the same candidate"),
            VoteError::DoubleVote(sbt) => {
                panic_str(&format!("user already voted with sbt={}", sbt))
            },
            VoteError::MinBond(req, amt) => panic_str(&format!("required bond amount={}, deposited={}", req, amt)),
            VoteError::Blacklisted => panic_str("user is blacklisted"),
            VoteError::NoBond => panic_str("Voter didn't bond")
        }
    }
}

/// Contract errors
#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum RevokeVoteError {
    NotActive,
    NotVoted,
    NotBlacklisted,
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
            RevokeVoteError::NotBlacklisted => {
                panic_str("can not revoke a not blacklisted voter")
            }
        }
    }
}
