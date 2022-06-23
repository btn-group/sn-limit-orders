use crate::authorize::authorize;
use crate::constants::{BLOCK_SIZE, CONFIG_KEY};
use crate::msg::{HandleMsg, InitMsg, QueryAnswer, QueryMsg, ReceiveMsg};
use crate::state::Config;
use crate::transaction_history::{
    get_txs, store_txs, update_tx, verify_txs, verify_txs_for_cancel,
    verify_txs_for_confirm_address,
};
use cosmwasm_std::{
    from_binary, to_binary, Api, Binary, Env, Extern, HandleResponse, HumanAddr, InitResponse,
    Querier, StdError, StdResult, Storage, Uint128,
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
        enabled: false,
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
        HandleMsg::Disable {} => disable(deps, env),
        HandleMsg::Enable {} => enable(deps, env),
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

fn disable<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY)?;
    authorize(env.message.sender, config.admin.clone())?;

    config.enabled = false;
    config_store.store(CONFIG_KEY, &config)?;
    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: None,
    })
}

fn enable<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY)?;
    authorize(env.message.sender, config.admin.clone())?;

    config.enabled = true;
    config_store.store(CONFIG_KEY, &config)?;
    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: None,
    })
}

fn pad_response(response: StdResult<HandleResponse>) -> StdResult<HandleResponse> {
    response.map(|mut response| {
        response.data = response.data.map(|mut data| {
            space_pad(BLOCK_SIZE, &mut data.0);
            data
        });
        response
    })
}

// Take a Vec<u8> and pad it up to a multiple of `block_size`, using spaces at the end.
fn space_pad(block_size: usize, message: &mut Vec<u8>) -> &mut Vec<u8> {
    let len = message.len();
    let surplus = len % block_size;
    if surplus == 0 {
        return message;
    }

    let missing = block_size - surplus;
    message.reserve(missing);
    message.extend(std::iter::repeat(b' ').take(missing));
    message
}

fn receive<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    from: HumanAddr,
    amount: Uint128,
    msg: Binary,
) -> StdResult<HandleResponse> {
    // // Ensure that the sent tokens are from an expected contract address
    // let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;
    // if env.message.sender != config.accepted_token.address {
    //     return Err(StdError::generic_err(format!(
    //         "This token is not supported. Supported: {}, given: {}",
    //         config.accepted_token.address, env.message.sender
    //     )));
    // }

    // Ok(HandleResponse {
    //     messages: vec![],
    //     log: vec![],
    //     data: None,
    // })

    let msg: ReceiveMsg = from_binary(&msg)?;
    let response = match msg {
        // ReceiveMsg::Cancel { position } => cancel(deps, &env, from, amount, position),
        ReceiveMsg::CreateReceiveRequest {
            address,
            send_amount,
            description,
            token,
        } => create_receive_request(
            deps,
            &env,
            from,
            amount,
            address,
            send_amount,
            description,
            token,
        ),
        // ReceiveMsg::SendPayment { position } => send_payment(deps, &env, from, amount, position),
    };
    pad_response(response)
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
                enabled: false,
                withdrawal_allowed_from: 3
            },
            value
        );
    }

    #[test]
    fn test_disable() {
        let (_init_result, mut deps) = init_helper();

        // Initially false
        let res = query(&deps, QueryMsg::Config {}).unwrap();
        let mut config: Config = from_binary(&res).unwrap();

        // when enabled
        let mut msg = HandleMsg::Enable {};
        handle(&mut deps, mock_env(config.admin.clone(), &[]), msg.clone()).unwrap();

        // = when disabled
        msg = HandleMsg::Disable {};
        // == when called by a non-admin
        // == * it raises an error
        assert_eq!(
            handle(&mut deps, mock_env("non-admin", &[]), msg.clone()).unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );

        // == when called by an admin
        // == * it disables the contract
        handle(&mut deps, mock_env(config.admin, &[]), msg.clone()).unwrap();
        config = from_binary(&res).unwrap();
        assert_eq!(false, config.enabled);
    }

    #[test]
    fn test_enable() {
        let (_init_result, mut deps) = init_helper();

        // Initially false
        let mut res = query(&deps, QueryMsg::Config {}).unwrap();
        let mut config: Config = from_binary(&res).unwrap();
        assert_eq!(false, config.enabled);

        let msg = HandleMsg::Enable {};
        // when called by a non admin
        // * it raises an error
        let handle_response = handle(&mut deps, mock_env("non-admin", &[]), msg.clone());
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );

        // when called by the admin
        handle(&mut deps, mock_env(config.admin, &[]), msg.clone()).unwrap();
        // * it enables the contract
        res = query(&deps, QueryMsg::Config {}).unwrap();
        config = from_binary(&res).unwrap();
        assert_eq!(true, config.enabled);
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
