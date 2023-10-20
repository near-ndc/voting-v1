pub mod errors;
mod events;

pub use events::*;
use near_sdk::{env, AccountId, Promise};

/// checks if there was enough storage deposit provided, and returns the excess of the deposit
/// back to the user.
/// * `storage_extra`: extra storage which should be credited for future operations.
pub fn finalize_storage_check(
    storage_start: u64,
    storage_extra: u64,
    user: AccountId,
) -> Result<(), String> {
    let storage_deposit = env::attached_deposit();
    let required_deposit =
        (env::storage_usage() - storage_start + storage_extra) as u128 * env::storage_byte_cost();
    if storage_deposit < required_deposit {
        return Err(format!(
            "not enough NEAR storage deposit, required: {}",
            required_deposit
        ));
    }
    let diff = storage_deposit - required_deposit;
    if diff > 0 {
        Promise::new(user).transfer(diff);
    }
    Ok(())
}
