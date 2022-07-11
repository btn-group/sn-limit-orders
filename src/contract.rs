use crate::constants::{BLOCK_SIZE, CONFIG_KEY};
use crate::msg::{HandleMsg, InitMsg, QueryAnswer, QueryMsg};
use crate::state::Config;
use crate::transaction_history::get_txs;
use cosmwasm_std::{
    to_binary, Api, Binary, Env, Extern, HandleResponse, HumanAddr, InitResponse, Querier,
    StdError, StdResult, Storage, Uint128,
};
use secret_toolkit::snip20;
use secret_toolkit::storage::{TypedStore, TypedStoreMut};

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let config: Config = Config {
        accepted_token: msg.accepted_token.clone(),
        admin: env.message.sender,
        butt: msg.butt,
        withdrawal_allowed_from: msg.withdrawal_allowed_from,
    };
    config_store.store(CONFIG_KEY, &config)?;

    Ok(InitResponse {
        messages: vec![],
        log: vec![],
    })
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    match msg {
        HandleMsg::Receive {
            from, amount, msg, ..
        } => receive(deps, env, from, amount, msg),
    }
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => {
            let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;
            Ok(to_binary(&config)?)
        }
        QueryMsg::Txs {
            address,
            key,
            page,
            page_size,
        } => txs(deps, address, key, page, page_size),
    }
}

pub fn correct_amount_of_token(
    amount_received: Uint128,
    amount_wanted: Uint128,
    token_received: HumanAddr,
    token_wanted: HumanAddr,
) -> StdResult<()> {
    if amount_received != amount_wanted {
        return Err(StdError::generic_err("Wrong amount received."));
    }
    if token_received != token_wanted {
        return Err(StdError::generic_err("Wrong token received."));
    }

    Ok(())
}

fn receive<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    _from: HumanAddr,
    _amount: Uint128,
    _msg: Binary,
) -> StdResult<HandleResponse> {
    // Ensure that the sent tokens are from an expected contract address
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;
    if env.message.sender != config.accepted_token.address {
        return Err(StdError::generic_err(format!(
            "This token is not supported. Supported: {}, given: {}",
            config.accepted_token.address, env.message.sender
        )));
    }

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: None,
    })
}

fn txs<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: HumanAddr,
    key: String,
    page: u32,
    page_size: u32,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();

    // This is here so that the user can use their viewing key for butt for this
    snip20::balance_query(
        &deps.querier,
        address.clone(),
        key.to_string(),
        BLOCK_SIZE,
        config.butt.contract_hash,
        config.butt.address,
    )?;

    let address = deps.api.canonical_address(&address)?;
    let (txs, total) = get_txs(&deps.api, &deps.storage, &address, page, page_size)?;

    let result = QueryAnswer::Txs {
        txs,
        total: Some(total),
    };
    to_binary(&result)
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
            butt: mock_butt(),
            withdrawal_allowed_from: 3,
        };
        (init(&mut deps, env.clone(), msg), deps)
    }

    fn mock_butt() -> SecretContract {
        SecretContract {
            address: HumanAddr::from("mock-butt-address"),
            contract_hash: "mock-butt-contract-hash".to_string(),
        }
    }

    #[test]
    fn test_config() {
        let (_init_result, deps) = init_helper();

        let res = query(&deps, QueryMsg::Config {}).unwrap();
        let value: Config = from_binary(&res).unwrap();
        let accepted_token = SecretContract {
            address: HumanAddr::from(MOCK_ACCEPTED_TOKEN_ADDRESS),
            contract_hash: MOCK_ACCEPTED_TOKEN_CONTRACT_HASH.to_string(),
        };
        assert_eq!(
            Config {
                accepted_token: accepted_token,
                admin: HumanAddr::from(MOCK_ADMIN),
                butt: mock_butt(),
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
}
