use near_sdk::{env::panic_str, FunctionError};

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
