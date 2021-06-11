use crate::msg::{BalanceResponse, ConfigResponse, HandleMsg, InitMsg, QueryMsg};
use crate::state::{config, config_read, State};
use cosmwasm_std::{
    to_binary, Api, Binary, Env, Extern, HandleResponse, HumanAddr, InitResponse, Querier,
    StdError, StdResult, Storage, Uint128,
};
use secret_toolkit::snip20;

pub const RESPONSE_BLOCK_SIZE: usize = 256;

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    let state = State {
        accepted_token: msg.accepted_token.clone(),
        admin: env.message.sender,
        contract_address: env.contract.address,
        viewing_key: msg.viewing_key.clone(),
        withdrawal_allowed_from: msg.withdrawal_allowed_from,
    };

    config(&mut deps.storage).save(&state)?;

    // https://github.com/enigmampc/secret-toolkit/tree/master/packages/snip20
    let messages = vec![
        snip20::register_receive_msg(
            env.contract_code_hash.clone(),
            None,
            1,
            msg.accepted_token.contract_hash.clone(),
            msg.accepted_token.address.clone(),
        )?,
        snip20::set_viewing_key_msg(
            msg.viewing_key,
            None,
            RESPONSE_BLOCK_SIZE,
            msg.accepted_token.contract_hash,
            msg.accepted_token.address,
        )?,
    ];

    Ok(InitResponse {
        messages,
        log: vec![],
    })
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    match msg {
        HandleMsg::ChangeAdmin { address, .. } => change_admin(deps, env, address),
        HandleMsg::Receive {
            from, amount, msg, ..
        } => receive(deps, env, from, amount, msg),
        HandleMsg::Withdraw {} => withdraw(deps, env),
    }
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::AcceptedTokenAvailable {} => to_binary(&accepted_token_available(deps)?),
        QueryMsg::Config {} => to_binary(&public_config(deps)?),
    }
}

fn accepted_token_available<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<BalanceResponse> {
    let state = config_read(&deps.storage).load()?;
    let balance = snip20::balance_query(
        &deps.querier,
        state.contract_address,
        state.viewing_key,
        RESPONSE_BLOCK_SIZE,
        state.accepted_token.contract_hash,
        state.accepted_token.address,
    )?;
    Ok(BalanceResponse {
        amount: balance.amount,
    })
}

fn change_admin<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    address: HumanAddr,
) -> StdResult<HandleResponse> {
    let mut state = config_read(&deps.storage).load()?;
    // Ensure that admin is calling this
    if env.message.sender != state.admin {
        return Err(StdError::Unauthorized { backtrace: None });
    }

    state.admin = address;
    config(&mut deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: None,
    })
}

fn public_config<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<ConfigResponse> {
    let state = config_read(&deps.storage).load()?;
    Ok(ConfigResponse {
        accepted_token: state.accepted_token,
        admin: state.admin,
        withdrawal_allowed_from: state.withdrawal_allowed_from,
    })
}

fn receive<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    _from: HumanAddr,
    _amount: Uint128,
    _msg: Binary,
) -> StdResult<HandleResponse> {
    // Ensure that the sent tokens are from an expected contract address
    let state = config_read(&deps.storage).load()?;
    if env.message.sender != state.accepted_token.address {
        return Err(StdError::generic_err(format!(
            "This token is not supported. Supported: {}, given: {}",
            state.accepted_token.address, env.message.sender
        )));
    }

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: None,
    })
}

fn withdraw<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let state = config_read(&deps.storage).load()?;
    // Ensure that admin is calling this
    if env.message.sender != state.admin {
        return Err(StdError::Unauthorized { backtrace: None });
    }

    // Ensure that the current time is after the withdrawal_allowed_from
    if env.block.time < state.withdrawal_allowed_from {
        return Err(StdError::generic_err(format!(
            "Withdrawal not allowed yet. Withdrawal allowed from: {}, current time: {}",
            state.withdrawal_allowed_from, env.block.time
        )));
    }

    let amount_of_accepted_token_to_send = accepted_token_available(deps)?.amount;
    // Transfer accepted token to admin
    let messages = vec![snip20::transfer_msg(
        state.admin,
        amount_of_accepted_token_to_send,
        None,
        RESPONSE_BLOCK_SIZE,
        state.accepted_token.contract_hash,
        state.accepted_token.address,
    )?];

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::ReceiveMsg;
    use crate::state::SecretContract;
    use cosmwasm_std::from_binary;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, MockApi, MockQuerier, MockStorage};

    pub const MOCK_ADMIN: &str = "admin";
    pub const MOCK_ACCEPTED_TOKEN_ADDRESS: &str = "sefismartcontractaddress";
    pub const MOCK_ACCEPTED_TOKEN_CONTRACT_HASH: &str = "Buttcoin";

    // === HELPERS ===
    fn init_helper() -> (
        StdResult<InitResponse>,
        Extern<MockStorage, MockApi, MockQuerier>,
    ) {
        let env = mock_env(MOCK_ADMIN, &[]);
        let accepted_token = SecretContract {
            address: HumanAddr::from(MOCK_ACCEPTED_TOKEN_ADDRESS),
            contract_hash: MOCK_ACCEPTED_TOKEN_CONTRACT_HASH.to_string(),
        };
        let mut deps = mock_dependencies(20, &[]);
        let msg = InitMsg {
            accepted_token: accepted_token.clone(),
            viewing_key: "nannofromthegirlfromnowhereisathaidemon?".to_string(),
            withdrawal_allowed_from: 3,
        };
        (init(&mut deps, env.clone(), msg), deps)
    }

    #[test]
    fn test_change_admin() {
        let (init_result, mut deps) = init_helper();

        assert!(
            init_result.is_ok(),
            "Init failed: {}",
            init_result.err().unwrap()
        );

        let handle_msg = HandleMsg::ChangeAdmin {
            address: HumanAddr("bob".to_string()),
            padding: None,
        };
        let handle_result = handle(&mut deps, mock_env(MOCK_ADMIN, &[]), handle_msg);
        assert!(
            handle_result.is_ok(),
            "handle() failed: {}",
            handle_result.err().unwrap()
        );

        let res = query(&deps, QueryMsg::Config {}).unwrap();
        let value: ConfigResponse = from_binary(&res).unwrap();
        assert_eq!(value.admin, HumanAddr("bob".to_string()));
    }

    #[test]
    fn test_public_config() {
        let (_init_result, deps) = init_helper();

        let res = query(&deps, QueryMsg::Config {}).unwrap();
        let value: ConfigResponse = from_binary(&res).unwrap();
        // Test response does not include viewing key.
        // Test that the desired fields are returned.
        let accepted_token = SecretContract {
            address: HumanAddr::from(MOCK_ACCEPTED_TOKEN_ADDRESS),
            contract_hash: MOCK_ACCEPTED_TOKEN_CONTRACT_HASH.to_string(),
        };
        assert_eq!(
            ConfigResponse {
                accepted_token: accepted_token,
                admin: HumanAddr::from(MOCK_ADMIN),
                withdrawal_allowed_from: 3
            },
            value
        );
    }

    #[test]
    fn test_receive_accepted_token_callback() {
        let (_init_result, mut deps) = init_helper();
        let amount: Uint128 = Uint128(333);
        let from: HumanAddr = HumanAddr::from("someuser");

        // Test that only accepted token is accepted
        // Not accepted token
        let msg = HandleMsg::Receive {
            amount: amount,
            from: from.clone(),
            sender: from.clone(),
            msg: to_binary(&ReceiveMsg::Deposit {}).unwrap(),
        };
        let handle_response = handle(&mut deps, mock_env("notasupportedtoken", &[]), msg.clone());
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::GenericErr {
                msg: format!(
                    "This token is not supported. Supported: {}, given: {}",
                    MOCK_ACCEPTED_TOKEN_ADDRESS, "notasupportedtoken"
                ),
                backtrace: None
            }
        );

        // Accepted token
        let msg = HandleMsg::Receive {
            amount: amount,
            from: from.clone(),
            sender: from,
            msg: to_binary(&ReceiveMsg::Deposit {}).unwrap(),
        };
        let handle_response = handle(
            &mut deps,
            mock_env(MOCK_ACCEPTED_TOKEN_ADDRESS, &[]),
            msg.clone(),
        );
        handle_response.unwrap();
    }

    #[test]
    fn test_withdraw() {
        let (_init_result, mut deps) = init_helper();
        let msg = HandleMsg::Withdraw {};
        let env_block_time = mock_env("anyone", &[]).block.time;

        // It raises an error when non-admin calls it
        let handle_response = handle(&mut deps, mock_env("anyone", &[]), msg.clone());
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );

        // It raises an error when admin calls this too early
        let mut state = config_read(&deps.storage).load().unwrap();
        state.withdrawal_allowed_from = env_block_time + 3;
        config(&mut deps.storage).save(&state).unwrap();
        let handle_response = handle(&mut deps, mock_env(MOCK_ADMIN, &[]), msg.clone());
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::generic_err(format!(
                "Withdrawal not allowed yet. Withdrawal allowed from: {}, current time: {}",
                state.withdrawal_allowed_from, env_block_time
            )),
        );

        // Test that a request is sent to the offered token contract address to transfer tokens to admin
        let mut state = config_read(&deps.storage).load().unwrap();
        state.withdrawal_allowed_from = env_block_time - 3;
        config(&mut deps.storage).save(&state).unwrap();
        let handle_response = handle(&mut deps, mock_env(MOCK_ADMIN, &[]), msg.clone());
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::generic_err(format!("Error performing Balance query: Generic error: Querier system error: No such contract: {}", MOCK_ACCEPTED_TOKEN_ADDRESS)),
        );
    }
}
