use crate::authorize::authorize;
use crate::constants::{BLOCK_SIZE, CONFIG_KEY};
use crate::msg::{HandleMsg, InitMsg, QueryAnswer, QueryMsg, ReceiveMsg};
use crate::orders::{
    get_orders, store_orders, update_order, verify_orders_for_cancel, verify_orders_for_fill,
};
use crate::state::Config;
use crate::state::SecretContract;
use cosmwasm_std::CosmosMsg;
use cosmwasm_std::{
    from_binary, to_binary, Api, Binary, Env, Extern, HandleResponse, HumanAddr, InitResponse,
    Querier, StdResult, Storage, Uint128,
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
        admin: env.message.sender,
        butt: msg.butt,
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
        HandleMsg::Cancel { position } => cancel_order(deps, &env, position),
        HandleMsg::Receive {
            from, amount, msg, ..
        } => receive(deps, env, from, amount, msg),
        HandleMsg::RegisterTokens { tokens } => register_tokens(&env, tokens),
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
        QueryMsg::Orders {
            address,
            key,
            page,
            page_size,
        } => orders(deps, address, key, page, page_size),
    }
}

fn cancel_order<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    position: u32,
) -> StdResult<HandleResponse> {
    let (mut creator_order, mut contract_order) = verify_orders_for_cancel(
        &mut deps.storage,
        &deps.api.canonical_address(&env.message.sender)?,
        &deps.api.canonical_address(&env.contract.address)?,
        position,
    )?;
    // Send refund to the creator
    let mut messages: Vec<CosmosMsg> = vec![];
    messages.push(snip20::transfer_msg(
        deps.api.human_address(&creator_order.creator)?,
        Uint128(creator_order.amount.u128() - creator_order.filled_amount.u128()),
        None,
        BLOCK_SIZE,
        creator_order.from_token.contract_hash,
        creator_order.from_token.address,
    )?);

    // Update Txs
    creator_order.status = 2;
    contract_order.status = 2;
    update_order(&mut deps.storage, &creator_order.creator, creator_order)?;
    update_order(
        &mut deps.storage,
        &deps.api.canonical_address(&env.contract.address)?,
        contract_order,
    )?;

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: None,
    })
}

fn orders<S: Storage, A: Api, Q: Querier>(
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
    let (orders, total) = get_orders(&deps.api, &deps.storage, &address, page, page_size)?;

    let result = QueryAnswer::Orders {
        orders,
        total: Some(total),
    };
    to_binary(&result)
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

fn receive<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    from: HumanAddr,
    amount: Uint128,
    msg: Binary,
) -> StdResult<HandleResponse> {
    let msg: ReceiveMsg = from_binary(&msg)?;
    let response = match msg {
        ReceiveMsg::CreateOrder {
            to_amount,
            to_token,
        } => create_order(deps, &env, from, amount, to_amount, to_token),
        ReceiveMsg::Fill { position } => fill_order(deps, &env, from, amount, position),
    };
    pad_response(response)
}

fn register_tokens(env: &Env, tokens: Vec<SecretContract>) -> StdResult<HandleResponse> {
    let mut messages = vec![];
    for token in tokens {
        let address = token.address;
        let contract_hash = token.contract_hash;
        messages.push(snip20::register_receive_msg(
            env.contract_code_hash.clone(),
            None,
            BLOCK_SIZE,
            contract_hash.clone(),
            address.clone(),
        )?);
    }

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: None,
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

fn create_order<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    from: HumanAddr,
    amount: Uint128,
    to_amount: Uint128,
    to_token: SecretContract,
) -> StdResult<HandleResponse> {
    store_orders(
        &mut deps.storage,
        SecretContract {
            address: env.message.sender.clone(),
            contract_hash: "CHANGETHISLATER".to_string(),
        },
        to_token,
        deps.api.canonical_address(&from)?,
        amount,
        to_amount,
        0,
        &env.block,
        deps.api.canonical_address(&env.contract.address)?,
    )?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: None,
    })
}

fn fill_order<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    from: HumanAddr,
    amount: Uint128,
    position: u32,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    authorize(from, config.admin)?;

    let (mut creator_order, mut contract_order) = verify_orders_for_fill(
        &deps.api,
        &mut deps.storage,
        &deps.api.canonical_address(&env.contract.address)?,
        amount,
        position,
        env.message.sender.clone(),
    )?;
    // Update filled amount
    // If filled amount is the same as amount set status to finalized
    // But do we even need a status apart from cancelled as we can figured it out from partly_filled
    // Send fee?

    // creator_order.status = 3;
    // contract_order.status = 3;
    // update_tx(
    //     &mut deps.storage,
    //     &creator_order.from.clone(),
    //     creator_order.clone(),
    // )?;
    // update_tx(
    //     &mut deps.storage,
    //     &contract_order.to.clone(),
    //     contract_order,
    // )?;
    // let config: Config = TypedStore::attach(&mut deps.storage)
    //     .load(CONFIG_KEY)
    //     .unwrap();
    let mut messages: Vec<CosmosMsg> = vec![];
    // messages.push(snip20::transfer_msg(
    //     config.treasury_address,
    //     creator_order.fee,
    //     None,
    //     BLOCK_SIZE,
    //     config.sscrt.contract_hash,
    //     config.sscrt.address,
    // )?);
    // messages.push(snip20::transfer_msg(
    //     deps.api.human_address(&creator_order.to)?,
    //     creator_order.amount,
    //     None,
    //     BLOCK_SIZE,
    //     creator_order.token.contract_hash,
    //     env.message.sender.clone(),
    // )?);

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        (init(&mut deps, env.clone(), msg), deps)
    }

    fn mock_butt() -> SecretContract {
        SecretContract {
            address: HumanAddr::from("mock-butt-address"),
            contract_hash: "mock-butt-contract-hash".to_string(),
        }
    }

    fn mock_contract() -> SecretContract {
        let env = mock_env(mock_user_address(), &[]);
        SecretContract {
            address: env.contract.address,
            contract_hash: env.contract_code_hash,
        }
    }

    fn mock_token() -> SecretContract {
        SecretContract {
            address: HumanAddr::from("mock-token-address"),
            contract_hash: "mock-token-contract-hash".to_string(),
        }
    }

    fn mock_user_address() -> HumanAddr {
        HumanAddr::from("gary")
    }

    #[test]
    fn test_config() {
        let (_init_result, deps) = init_helper();

        let res = query(&deps, QueryMsg::Config {}).unwrap();
        let value: Config = from_binary(&res).unwrap();
        assert_eq!(
            Config {
                admin: HumanAddr::from(MOCK_ADMIN),
                butt: mock_butt(),
            },
            value
        );
    }

    #[test]
    fn test_register_tokens() {
        let (_init_result, mut deps) = init_helper();
        let env = mock_env(mock_user_address(), &[]);

        // When tokens are in the parameter
        let handle_msg = HandleMsg::RegisterTokens {
            tokens: vec![mock_butt(), mock_token()],
        };
        let handle_result = handle(&mut deps, env.clone(), handle_msg);
        let handle_result_unwrapped = handle_result.unwrap();
        // * it sends a message to register receive for the token and sets a viewing key
        assert_eq!(
            handle_result_unwrapped.messages,
            vec![
                snip20::register_receive_msg(
                    mock_contract().contract_hash.clone(),
                    None,
                    BLOCK_SIZE,
                    mock_butt().contract_hash,
                    mock_butt().address,
                )
                .unwrap(),
                snip20::register_receive_msg(
                    mock_contract().contract_hash,
                    None,
                    BLOCK_SIZE,
                    mock_token().contract_hash,
                    mock_token().address,
                )
                .unwrap(),
            ]
        );
    }
}
