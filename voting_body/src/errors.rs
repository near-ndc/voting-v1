use near_sdk::env::panic_str;
use near_sdk::serde::Serialize;
use near_sdk::FunctionError;

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum VoteError {
    PropNotFound,
    NotAuthorized,
    NotInProgress,
    Timeout,
    LockedUntil,
    Storage(String),
    NotIAHreg,
}

impl FunctionError for VoteError {
    fn panic(&self) -> ! {
        match self {
            VoteError::PropNotFound => panic_str("proposal doesn't exist"),
            VoteError::NotAuthorized => panic_str("not authorized"),
            VoteError::NotInProgress => panic_str("proposal not in progress"),
            VoteError::Timeout => panic_str("voting time is over"),
            VoteError::LockedUntil => {
                panic_str("account must be locked in iah_registry longer than the voting end")
            }
            VoteError::Storage(reason) => panic_str(reason),
            VoteError::NotIAHreg => panic_str("must be called by iah_registry"),
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
    NotIAHreg,
    BadRequest(String),
}

impl FunctionError for CreatePropError {
    fn panic(&self) -> ! {
        match self {
            CreatePropError::NotAuthorized => panic_str("not authorized"),
            CreatePropError::Storage(reason) => panic_str(reason),
            CreatePropError::MinBond => panic_str("min pre_vote_bond is required"),
            CreatePropError::BadRequest(reason) => panic_str(reason),
            CreatePropError::NotIAHreg => panic_str("must be called by iah_registry"),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(PartialEq, Debug))]
pub enum PrevoteError {
    NotFound,
    MinBond,
    NotOverdue,
    DoubleSupport,
    NotCongress,
    NotCongressMember,
    LockedUntil,
    NotIAHreg,
}

impl FunctionError for PrevoteError {
    fn panic(&self) -> ! {
        match self {
            PrevoteError::NotFound => panic_str("proposal not found"),
            PrevoteError::MinBond => panic_str("min active_queue_bond is required"),
            PrevoteError::NotOverdue => panic_str("proposal is not overdue"),
            PrevoteError::DoubleSupport => panic_str("already supported the proposal"),
            PrevoteError::NotCongress => panic_str("dao is not part of the congress"),
            PrevoteError::NotCongressMember => panic_str("user is not part of the congress dao"),
            PrevoteError::LockedUntil => {
                panic_str("account must be locked in iah_registry longer than the prevote end")
            }
            PrevoteError::NotIAHreg => panic_str("must be called by iah_registry"),
        }
    }
}
