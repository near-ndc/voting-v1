use near_sdk::env::panic_str;
use near_sdk::FunctionError;

use thiserror::Error;

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum VoteError {
    NotAuthorized,
    DoubleVote,
    NoProp,
    NotInProgress,
    NotActive,
}

impl FunctionError for VoteError {
    fn panic(&self) -> ! {
        match self {
            VoteError::NotAuthorized => panic_str("not authorized"),
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

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum CreatePropError {
    BudgetOverflow,
    NotAuthorized,
    KindNotAllowed,
    Storage(String),
}

impl FunctionError for CreatePropError {
    fn panic(&self) -> ! {
        match self {
            CreatePropError::BudgetOverflow => panic_str("budget cap overflow"),
            CreatePropError::NotAuthorized => panic_str("not authorized"),
            CreatePropError::KindNotAllowed => panic_str("proposal kind not allowed"),
            CreatePropError::Storage(reason) => panic_str(&reason),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum HookError {
    NotAuthorized,
    NoMember,
}

impl FunctionError for HookError {
    fn panic(&self) -> ! {
        match self {
            HookError::NotAuthorized => panic_str("not authorized"),
            HookError::NoMember => panic_str("member not found"),
        }
    }
}
