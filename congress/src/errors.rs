use near_sdk::env::panic_str;
use near_sdk::FunctionError;

/// Contract errors
#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum VoteError {
    DoubleVote,
    NoProp,
    NotInProgress,
}

impl FunctionError for VoteError {
    fn panic(&self) -> ! {
        match self {
            VoteError::DoubleVote => panic_str("user already voted"),
            VoteError::NoProp => panic_str("proposal not found"),
            VoteError::NotInProgress => panic_str("proposal not in progress"),
        }
    }
}
