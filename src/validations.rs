use cosmwasm_std::{HumanAddr, StdError, StdResult, Uint128};

pub fn authorize(allowed: Vec<HumanAddr>, received: HumanAddr) -> StdResult<()> {
    if !allowed.contains(&received) {
        return Err(StdError::Unauthorized { backtrace: None });
    }

    Ok(())
}

pub fn validate_human_addr(
    expected: HumanAddr,
    received: HumanAddr,
    message: &str,
) -> StdResult<()> {
    if expected != received {
        return Err(StdError::generic_err(message));
    }

    Ok(())
}

pub fn validate_uint128(expected: Uint128, received: Uint128, message: &str) -> StdResult<()> {
    if expected != received {
        return Err(StdError::generic_err(message));
    }

    Ok(())
}
