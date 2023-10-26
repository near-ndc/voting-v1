use near_sdk::env::panic_str;
use near_sdk::serde::Serialize;
use near_sdk::FunctionError;

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum VoteError {
    PropNotFound,
    NotAuthorized,
    NotInProgress,
    Timeout,
    Storage(String),
}

impl FunctionError for VoteError {
    fn panic(&self) -> ! {
        match self {
            VoteError::PropNotFound => panic_str("proposal doesn't exist"),
            VoteError::NotAuthorized => panic_str("not authorized"),
            VoteError::NotInProgress => panic_str("proposal not in progress"),
            VoteError::Timeout => panic_str("voting time is over"),
            VoteError::Storage(reason) => panic_str(reason),
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum ExecError {
    PropNotFound,
    AlreadyFinalized,
    InProgress,
}

impl FunctionError for ExecError {
    fn panic(&self) -> ! {
        match self {
            ExecError::PropNotFound => panic_str("proposal doesn't exist"),
            ExecError::AlreadyFinalized => panic_str("proposal is already successfully finalized"),
            ExecError::InProgress => panic_str("proposal is still in progress"),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum CreatePropError {
    NotAuthorized,
    Storage(String),
    MinBond,
    FunctionCall(String),
}

impl FunctionError for CreatePropError {
    fn panic(&self) -> ! {
        match self {
            CreatePropError::NotAuthorized => panic_str("not authorized"),
            CreatePropError::Storage(reason) => panic_str(reason),
            CreatePropError::MinBond => panic_str("min pre_vote_bond is required"),
            CreatePropError::FunctionCall(reason) => panic_str(reason),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum PrevotePropError {
    NotFound,
    MinBond,
    NotOverdue,
    DoubleSupport,
    NotCongress,
    NotCongressMember,
}

impl FunctionError for PrevotePropError {
    fn panic(&self) -> ! {
        match self {
            PrevotePropError::NotFound => panic_str("proposal not found"),
            PrevotePropError::MinBond => panic_str("min active_queue_bond is required"),
            PrevotePropError::NotOverdue => panic_str("proposal is not overdue"),
            PrevotePropError::DoubleSupport => panic_str("already supported the proposal"),
            PrevotePropError::NotCongress => panic_str("dao is not part of the congress"),
            PrevotePropError::NotCongressMember => {
                panic_str("user is not part of the congress dao")
            }
        }
    }
}
