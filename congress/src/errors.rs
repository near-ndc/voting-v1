use near_sdk::env::panic_str;
use near_sdk::serde::Serialize;
use near_sdk::FunctionError;

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum VoteError {
    NotAuthorized,
    DoubleVote,
    NotInProgress,
    NotActive,
    NotAllowedAgainst,
}

impl FunctionError for VoteError {
    fn panic(&self) -> ! {
        match self {
            VoteError::NotAuthorized => panic_str("not authorized"),
            VoteError::DoubleVote => panic_str("user already voted"),
            VoteError::NotInProgress => panic_str("proposal not in progress"),
            VoteError::NotActive => panic_str("voting time is over"),
            VoteError::NotAllowedAgainst => {
                panic_str("not allowed to vote on proposal against them")
            }
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
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
            CreatePropError::Storage(reason) => panic_str(reason),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum HookError {
    NotAuthorized,
    NoMember,
    ProposalFinalized,
    CooldownOver,
}

impl FunctionError for HookError {
    fn panic(&self) -> ! {
        match self {
            HookError::NotAuthorized => panic_str("not authorized"),
            HookError::NoMember => panic_str("member not found"),
            HookError::ProposalFinalized => panic_str("proposal finalized"),
            HookError::CooldownOver => panic_str("cooldown period is over"),
        }
    }
}
