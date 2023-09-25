use near_sdk::env::panic_str;
use near_sdk::FunctionError;

use thiserror::Error;

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum VoteError {
    DoubleVote,
    NoProp,
    NotInProgress,
    NotActive,
}

impl FunctionError for VoteError {
    fn panic(&self) -> ! {
        match self {
            VoteError::DoubleVote => panic_str("user already voted"),
            VoteError::NoProp => panic_str("proposal not found"),
            VoteError::NotInProgress => panic_str("proposal not in progress"),
            VoteError::NotActive => panic_str("voting time is over"),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum ExecError {
    ExecTime,
    NotApproved,
    BudgetOverflow,
}

impl FunctionError for ExecError {
    fn panic(&self) -> ! {
        match self {
            ExecError::ExecTime => panic_str("can only be executed after cooldown"),
            ExecError::NotApproved => {
                panic_str("can execute only approved or re-execute failed proposals")
            }
            ExecError::BudgetOverflow => panic_str("budget cap overflow"),
        }
    }
}
