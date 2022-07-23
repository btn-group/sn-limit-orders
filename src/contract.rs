use crate::authorize::authorize;
use crate::constants::{
    BLOCK_SIZE, CONFIG_KEY, MOCK_AMOUNT, MOCK_BUTT_ADDRESS, MOCK_TOKEN_ADDRESS,
    PREFIX_ACTIVITY_RECORDS, PREFIX_ORDERS,
};
use crate::msg::{HandleMsg, InitMsg, QueryAnswer, QueryMsg, ReceiveMsg, Snip20Swap};
use crate::state::{
    delete_route_state, read_registered_token, read_route_state, store_route_state,
    write_registered_token, ActivityRecord, Config, Hop, HumanizedOrder, Order, RegisteredToken,
    Route, RouteState, SecretContract,
};
use cosmwasm_std::{
    from_binary, to_binary, Api, BalanceResponse, BankMsg, BankQuery, Binary, CanonicalAddr, Coin,
    CosmosMsg, Env, Extern, HandleResponse, HumanAddr, InitResponse, Querier, QueryRequest,
    ReadonlyStorage, StdError, StdResult, Storage, Uint128, WasmMsg,
};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage};
use primitive_types::U256;
use secret_toolkit::snip20;
use secret_toolkit::storage::{AppendStore, AppendStoreMut, TypedStore, TypedStoreMut};
use std::collections::VecDeque;

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let config: Config = Config {
        addresses_allowed_to_fill: vec![env.message.sender.clone(), env.contract.address],
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
        HandleMsg::HandleFirstHop {
            borrow_amount,
            hops,
        } => handle_first_hop(deps, &env, borrow_amount, hops),
        HandleMsg::FinalizeRoute {} => finalize_route(deps, &env),
        HandleMsg::Receive {
            from, amount, msg, ..
        } => receive(deps, env, from, amount, msg),
        HandleMsg::RegisterTokens {
            tokens,
            viewing_key,
        } => register_tokens(deps, &env, tokens, viewing_key),
        HandleMsg::RescueTokens {
            denom,
            key,
            token_address,
        } => rescue_tokens(deps, &env, denom, key, token_address),
        HandleMsg::UpdateAddressesAllowedToFill {
            addresses_allowed_to_fill,
        } => update_addresses_allowed_to_fill(deps, &env, addresses_allowed_to_fill),
    }
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::ActivityRecords {
            key,
            page,
            page_size,
        } => activity_records(deps, key, page, page_size),
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

fn receive<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    from: HumanAddr,
    amount: Uint128,
    msg: Option<Binary>,
) -> StdResult<HandleResponse> {
    let response = if msg.is_some() {
        let msg: ReceiveMsg = from_binary(&msg.unwrap())?;
        match msg {
            ReceiveMsg::CancelOrder { position } => {
                cancel_order(deps, &env, from, amount, position)
            }
            ReceiveMsg::CreateOrder {
                butt_viewing_key,
                to_amount,
                to_token,
            } => create_order(
                deps,
                &env,
                from,
                amount,
                butt_viewing_key,
                to_amount,
                to_token,
            ),
            ReceiveMsg::FillOrder { position } => fill_order(deps, &env, from, amount, position),
        }
    } else {
        handle_hop(deps, &env, from, amount)
    };
    pad_response(response)
}

fn activity_records<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    key: String,
    page: u32,
    page_size: u32,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    // This is here to check the admin's viewing key
    query_balance_of_token(deps, config.admin.clone(), config.butt, key.to_string())?;

    let address = deps.api.canonical_address(&config.admin)?;
    let (activity_records, total) = get_activity_records(&deps.storage, &address, page, page_size)?;
    let result = QueryAnswer::ActivityRecords {
        activity_records,
        total: Some(total),
    };
    to_binary(&result)
}

fn append_activity_record<S: Storage>(
    store: &mut S,
    activity_record: &ActivityRecord,
    for_address: &CanonicalAddr,
) -> StdResult<()> {
    let mut store =
        PrefixedStorage::multilevel(&[PREFIX_ACTIVITY_RECORDS, for_address.as_slice()], store);
    let mut store = AppendStoreMut::attach_or_create(&mut store)?;
    store.push(activity_record)
}

fn append_order<S: Storage>(
    store: &mut S,
    order: &Order,
    for_address: &CanonicalAddr,
) -> StdResult<()> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_ORDERS, for_address.as_slice()], store);
    let mut store = AppendStoreMut::attach_or_create(&mut store)?;
    store.push(order)
}

fn calculate_fee(user_butt_balance: Uint128, to_amount: Uint128) -> StdResult<Uint128> {
    let user_butt_balance_as_u128: u128 = user_butt_balance.u128();
    let nom = if user_butt_balance_as_u128 >= 100_000_000_000 {
        0
    } else if user_butt_balance_as_u128 >= 50_000_000_000 {
        6
    } else if user_butt_balance_as_u128 >= 25_000_000_000 {
        12
    } else if user_butt_balance_as_u128 >= 12_500_000_000 {
        18
    } else if user_butt_balance_as_u128 >= 6_250_000_000 {
        24
    } else {
        30
    };
    if nom == 0 {
        return Ok(Uint128(0));
    }

    let f = U256::from(to_amount.u128()).checked_mul(U256::from(nom));
    if f.is_none() {
        return Err(StdError::generic_err(
            "Overflow error while calculating fee.",
        ));
    }

    return Ok(Uint128::from((f.unwrap() / U256::from(10_000)).as_u128()));
}

fn cancel_order<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    from: HumanAddr,
    amount: Uint128,
    position: u32,
) -> StdResult<HandleResponse> {
    if amount.u128() > 0 {
        return Err(StdError::generic_err("Amount sent in must be zero."));
    };

    let mut creator_order = order_at_position(
        &mut deps.storage,
        &deps.api.canonical_address(&from)?,
        position,
    )?;
    let mut contract_order = order_at_position(
        &mut deps.storage,
        &deps.api.canonical_address(&env.contract.address)?,
        creator_order.other_storage_position,
    )?;
    if creator_order.from_token != env.message.sender {
        return Err(StdError::generic_err(
            "Token used to cancel does not match the from token of order.",
        ));
    }
    if creator_order.cancelled {
        return Err(StdError::generic_err("Order already cancelled."));
    }
    if creator_order.from_amount == creator_order.from_amount_filled {
        return Err(StdError::generic_err("Order already filled."));
    }

    let from_token: RegisteredToken = read_registered_token(
        &deps.storage,
        &deps.api.canonical_address(&creator_order.from_token)?,
    )
    .unwrap();
    // Send refund to the creator
    let mut messages: Vec<CosmosMsg> = vec![];
    messages.push(snip20::transfer_msg(
        deps.api.human_address(&creator_order.creator)?,
        (creator_order.from_amount - creator_order.from_amount_filled)?,
        None,
        BLOCK_SIZE,
        from_token.contract_hash,
        from_token.address,
    )?);

    // Update Txs
    creator_order.cancelled = true;
    contract_order.cancelled = true;
    update_order(
        &mut deps.storage,
        &creator_order.creator.clone(),
        creator_order,
    )?;
    update_order(
        &mut deps.storage,
        &deps.api.canonical_address(&env.contract.address)?,
        contract_order.clone(),
    )?;

    // Create activity record
    let activity_record: ActivityRecord = ActivityRecord {
        position: contract_order.position,
        activity: 0,
        result_from_amount_filled: None,
        result_net_to_amount_filled: None,
        updated_at_block_height: env.block.height,
        updated_at_block_time: env.block.time,
    };
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    append_activity_record(
        &mut deps.storage,
        &activity_record,
        &deps.api.canonical_address(&config.admin)?,
    )?;

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: None,
    })
}

fn create_order<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    from: HumanAddr,
    from_amount: Uint128,
    butt_viewing_key: String,
    to_amount: Uint128,
    to_token: HumanAddr,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    let to_token_address_canonical = deps.api.canonical_address(&to_token)?;
    let to_token_details: Option<RegisteredToken> =
        read_registered_token(&deps.storage, &to_token_address_canonical);
    if to_token_details.is_none() {
        return Err(StdError::generic_err("To token is not registered."));
    }

    // Calculate fee
    let user_butt_balance: Uint128 =
        query_balance_of_token(deps, from.clone(), config.butt, butt_viewing_key)?;
    let fee = calculate_fee(user_butt_balance, to_amount)?;

    // Increase sum balance for from_token
    let from_token_address_canonical = deps.api.canonical_address(&env.message.sender)?;
    let mut from_token_details: RegisteredToken =
        read_registered_token(&deps.storage, &from_token_address_canonical).unwrap();
    from_token_details.sum_balance += from_amount;
    write_registered_token(
        &mut deps.storage,
        &from_token_address_canonical,
        from_token_details,
    )?;

    // Store order
    let contract_address: CanonicalAddr = deps.api.canonical_address(&env.contract.address)?;
    let creator_address: CanonicalAddr = deps.api.canonical_address(&from)?;
    let contract_order_position = get_next_position(&mut deps.storage, &contract_address)?;
    let creator_order_position = get_next_position(&mut deps.storage, &creator_address)?;
    let creator_order = Order {
        position: creator_order_position,
        other_storage_position: contract_order_position,
        from_token: env.message.sender.clone(),
        to_token: to_token,
        creator: creator_address.clone(),
        from_amount,
        from_amount_filled: Uint128(0),
        net_to_amount: (to_amount - fee)?,
        net_to_amount_filled: Uint128(0),
        cancelled: false,
        fee: fee,
        created_at_block_time: env.block.time,
        created_at_block_height: env.block.height,
    };
    let mut contract_order = creator_order.clone();
    contract_order.position = contract_order_position;
    contract_order.other_storage_position = creator_order_position;
    append_order(&mut deps.storage, &contract_order, &contract_address)?;
    append_order(&mut deps.storage, &creator_order, &creator_address)?;

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
    if !config.addresses_allowed_to_fill.contains(&from) {
        return Err(StdError::Unauthorized { backtrace: None });
    }
    if amount.is_zero() {
        return Err(StdError::generic_err("Amount must be greater than zero."));
    }

    let mut contract_order = order_at_position(
        &mut deps.storage,
        &deps.api.canonical_address(&env.contract.address)?,
        position,
    )?;
    let mut creator_order = order_at_position(
        &mut deps.storage,
        &contract_order.creator,
        contract_order.other_storage_position,
    )?;
    // Check the token is the same at the to_token
    if creator_order.to_token != env.message.sender {
        return Err(StdError::generic_err(
            "To token does not match the token sent in.",
        ));
    }
    // Check the amount + filled amount is less than or equal to amount
    if creator_order.cancelled {
        return Err(StdError::generic_err("Order already cancelled."));
    }
    let unfilled_amount: Uint128 =
        (creator_order.net_to_amount - creator_order.net_to_amount_filled)?;
    if amount > unfilled_amount {
        return Err(StdError::generic_err(
            "Amount is greater than unfilled amount.",
        ));
    }

    // Update net_to_amount_filled and from_amount_filled
    contract_order.net_to_amount_filled += amount;
    creator_order.net_to_amount_filled += amount;
    let from_filled_amount: Uint128 =
        if contract_order.net_to_amount_filled == contract_order.net_to_amount {
            (contract_order.from_amount - contract_order.from_amount_filled)?
        } else {
            let f = U256::from(contract_order.from_amount.u128())
                .checked_mul(U256::from(amount.u128()));
            if f.is_none() {
                return Err(StdError::generic_err(
                    "Overflow error while calculating from_filled_amount.",
                ));
            }

            Uint128::from((f.unwrap() / U256::from(contract_order.net_to_amount.u128())).as_u128())
        };
    contract_order.from_amount_filled += from_filled_amount;
    creator_order.from_amount_filled += from_filled_amount;
    update_order(
        &mut deps.storage,
        &creator_order.creator,
        creator_order.clone(),
    )?;
    update_order(
        &mut deps.storage,
        &deps.api.canonical_address(&env.contract.address)?,
        contract_order.clone(),
    )?;

    // Send from token to admin
    // Send to token to creator
    let mut from_registered_token: RegisteredToken = read_registered_token(
        &deps.storage,
        &deps.api.canonical_address(&creator_order.from_token)?,
    )
    .unwrap();
    let to_registered_token: RegisteredToken = read_registered_token(
        &deps.storage,
        &deps.api.canonical_address(&creator_order.to_token)?,
    )
    .unwrap();
    let messages: Vec<CosmosMsg> = vec![
        snip20::send_msg(
            from,
            from_filled_amount,
            None,
            None,
            BLOCK_SIZE,
            from_registered_token.contract_hash.clone(),
            from_registered_token.address.clone(),
        )?,
        snip20::transfer_msg(
            deps.api.human_address(&contract_order.creator)?,
            amount,
            None,
            BLOCK_SIZE,
            to_registered_token.contract_hash,
            to_registered_token.address,
        )?,
    ];

    // Update from_token balance
    from_registered_token.sum_balance = (from_registered_token.sum_balance - from_filled_amount)?;
    write_registered_token(
        &mut deps.storage,
        &deps.api.canonical_address(&from_registered_token.address)?,
        from_registered_token,
    )?;

    // Create activity record
    let activity_record: ActivityRecord = ActivityRecord {
        position: contract_order.position,
        activity: 1,
        result_from_amount_filled: Some(contract_order.from_amount_filled),
        result_net_to_amount_filled: Some(contract_order.net_to_amount_filled),
        updated_at_block_height: env.block.height,
        updated_at_block_time: env.block.time,
    };
    append_activity_record(
        &mut deps.storage,
        &activity_record,
        &deps.api.canonical_address(&config.admin)?,
    )?;

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: None,
    })
}

fn finalize_route<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
) -> StdResult<HandleResponse> {
    match read_route_state(&deps.storage)? {
        Some(RouteState {
            remaining_route, ..
        }) => {
            // this function is called only by the route creation function
            // it is intended to always make sure that the route was completed successfully
            // otherwise we revert the transaction
            authorize(env.contract.address.clone(), env.message.sender.clone())?;
            if remaining_route.hops.len() != 0 {
                return Err(StdError::generic_err(format!(
                    "cannot finalize: route still contains hops: {:?}",
                    remaining_route
                )));
            }
            delete_route_state(&mut deps.storage);
            Ok(HandleResponse::default())
        }
        None => Err(StdError::generic_err("no route to finalize")),
    }
}

fn get_activity_records<S: ReadonlyStorage>(
    storage: &S,
    for_address: &CanonicalAddr,
    page: u32,
    page_size: u32,
) -> StdResult<(Vec<ActivityRecord>, u64)> {
    let store = ReadonlyPrefixedStorage::multilevel(
        &[PREFIX_ACTIVITY_RECORDS, for_address.as_slice()],
        storage,
    );

    // Try to access the storage of activity_records for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let store = AppendStore::<ActivityRecord, _, _>::attach(&store);
    let store = if let Some(result) = store {
        result?
    } else {
        return Ok((vec![], 0));
    };

    // Take `page_size` activity_records starting from the latest ActivityRecord, potentially skipping `page * page_size`
    // activity_records from the start.
    let activity_record_iter = store
        .iter()
        .rev()
        .skip((page * page_size) as _)
        .take(page_size as _);

    let activity_records: StdResult<Vec<ActivityRecord>> = activity_record_iter.collect();
    activity_records.map(|activity_records| (activity_records, store.len() as u64))
}

fn get_next_position<S: Storage>(store: &mut S, for_address: &CanonicalAddr) -> StdResult<u32> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_ORDERS, for_address.as_slice()], store);
    let store = AppendStoreMut::<Order, _>::attach_or_create(&mut store)?;
    Ok(store.len())
}

fn get_orders<A: Api, S: ReadonlyStorage>(
    api: &A,
    storage: &S,
    for_address: &CanonicalAddr,
    page: u32,
    page_size: u32,
) -> StdResult<(Vec<HumanizedOrder>, u64)> {
    let store =
        ReadonlyPrefixedStorage::multilevel(&[PREFIX_ORDERS, for_address.as_slice()], storage);

    // Try to access the storage of orders for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let store = AppendStore::<Order, _, _>::attach(&store);
    let store = if let Some(result) = store {
        result?
    } else {
        return Ok((vec![], 0));
    };

    // Take `page_size` orders starting from the latest Order, potentially skipping `page * page_size`
    // orders from the start.
    let order_iter = store
        .iter()
        .rev()
        .skip((page * page_size) as _)
        .take(page_size as _);

    // The `and_then` here flattens the `StdResult<StdResult<RichOrder>>` to an `StdResult<RichOrder>`
    let orders: StdResult<Vec<HumanizedOrder>> = order_iter
        .map(|order| order.map(|order| order.into_humanized(api)).and_then(|x| x))
        .collect();
    orders.map(|orders| (orders, store.len() as u64))
}

fn handle_first_hop<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    borrow_amount: Uint128,
    mut hops: VecDeque<Hop>,
) -> StdResult<HandleResponse> {
    // This is the first msg from the user, with the entire route details
    // 1. save the remaining route to state (e.g. if the route is X/Y -> Y/Z -> Z->W then save Y/Z -> Z/W to state)
    // 2. send `amount` X to pair X/Y
    // 3. call FinalizeRoute to make sure everything went ok, otherwise revert the tx
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    if !config
        .addresses_allowed_to_fill
        .contains(&env.message.sender)
    {
        return Err(StdError::Unauthorized { backtrace: None });
    }
    if hops.len() < 2 {
        return Err(StdError::generic_err("Route must be at least 2 hops."));
    }

    // unwrap is cool because `hops.len() >= 2`
    let first_hop: Hop = hops.pop_front().unwrap();
    let route: Route = Route {
        hops,
        borrow_amount,
        borrow_token: first_hop.from_token.clone(),
        to: env.message.sender.clone(),
    };
    store_route_state(
        &mut deps.storage,
        &RouteState {
            current_hop: Some(first_hop.clone()),
            remaining_route: route,
        },
    )?;
    let mut msgs = vec![snip20::send_msg(
        first_hop.trade_smart_contract.address.clone(),
        borrow_amount,
        Some(swap_msg(env.contract.address.clone(), first_hop.clone())?),
        None,
        BLOCK_SIZE,
        first_hop.from_token.contract_hash,
        first_hop.from_token.address,
    )?];

    msgs.push(
        // finalize the route at the end, to make sure the route was completed successfully
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.clone(),
            callback_code_hash: env.contract_code_hash.clone(),
            msg: to_binary(&HandleMsg::FinalizeRoute {})?,
            send: vec![],
        }),
    );

    Ok(HandleResponse {
        messages: msgs,
        log: vec![],
        data: None,
    })
}

fn swap_msg(contract_address: HumanAddr, hop: Hop) -> StdResult<Binary> {
    let swap_msg = if hop.position.is_some() {
        to_binary(&ReceiveMsg::FillOrder {
            position: hop.position.unwrap(),
        })?
    } else {
        to_binary(&Snip20Swap::Swap {
            // set expected_return to None because we don't care about slippage mid-route
            expected_return: None,
            // set the recepient of the swap to be this contract (the router)
            to: Some(contract_address),
        })?
    };
    Ok(swap_msg)
}

fn handle_hop<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    _from: HumanAddr,
    amount: Uint128,
) -> StdResult<HandleResponse> {
    // This is a receive msg somewhere along the route
    // 1. load route from state (Y/Z -> Z/W)
    // 2. save the remaining route to state (Z/W)
    // 3. send `amount` Y to pair Y/Z

    // 1'. load route from state (Z/W)
    // 2'. this is the last hop so delete the entire route state
    // 3'. send `amount` Z to pair Z/W with recepient `to`
    match read_route_state(&deps.storage)? {
        Some(RouteState {
            current_hop,
            remaining_route:
                Route {
                    mut hops,
                    borrow_amount,
                    borrow_token,
                    to,
                },
        }) => {
            let next_hop: Hop = match hops.pop_front() {
                Some(next_hop) => next_hop,
                None => return Err(StdError::generic_err("Route must be at least 1 hop.")),
            };
            if env.message.sender != next_hop.from_token.address || current_hop.is_none() {
                return Err(StdError::generic_err(
                    "Route can only be called by receiving the token of the next hop from the previous pair.",
                ));
            }

            let mut msgs = vec![];
            let current_hop = Some(next_hop.clone());
            if hops.len() == 0 {
                // last hop
                // 1. set is_done to true for FinalizeRoute
                // 2. set expected_return for the final swap
                // 3. set the recipient of the final swap to be the user
                if amount.lt(&borrow_amount) {
                    return Err(StdError::generic_err(
                        "Operation fell short of minimum_acceptable_amount",
                    ));
                }
                // Send fee to appropriate person
                if amount.gt(&borrow_amount) {
                    msgs.push(snip20::transfer_msg(
                        to.clone(),
                        (amount - borrow_amount).unwrap(),
                        None,
                        BLOCK_SIZE,
                        borrow_token.contract_hash.clone(),
                        borrow_token.address.clone(),
                    )?);
                }
            } else {
                // not last hop
                // 1. set expected_return to None because we don't care about slippage mid-route
                // 2. set the recipient of the swap to be this contract (the router)
                msgs.push(snip20::send_msg(
                    next_hop.trade_smart_contract.address.clone(),
                    amount,
                    Some(swap_msg(env.contract.address.clone(), next_hop.clone())?),
                    None,
                    BLOCK_SIZE,
                    next_hop.from_token.contract_hash,
                    next_hop.from_token.address,
                )?);
            }
            store_route_state(
                &mut deps.storage,
                &RouteState {
                    current_hop,
                    remaining_route: Route {
                        hops, // hops was mutated earlier when we did `hops.pop_front()`
                        borrow_amount,
                        borrow_token,
                        to,
                    },
                },
            )?;

            Ok(HandleResponse {
                messages: msgs,
                log: vec![],
                data: None,
            })
        }
        None => Err(StdError::generic_err("cannot find route")),
    }
}

fn order_at_position<S: Storage>(
    store: &mut S,
    address: &CanonicalAddr,
    position: u32,
) -> StdResult<Order> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_ORDERS, address.as_slice()], store);
    // Try to access the storage of orders for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let store = AppendStoreMut::<Order, _, _>::attach_or_create(&mut store)?;

    Ok(store.get_at(position)?)
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

fn query_balance_of_token<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: HumanAddr,
    token: SecretContract,
    viewing_key: String,
) -> StdResult<Uint128> {
    if token.address == HumanAddr::from(MOCK_TOKEN_ADDRESS)
        || token.address == HumanAddr::from(MOCK_BUTT_ADDRESS)
    {
        Ok(Uint128(MOCK_AMOUNT))
    } else {
        let balance = snip20::balance_query(
            &deps.querier,
            address,
            viewing_key,
            BLOCK_SIZE,
            token.contract_hash,
            token.address,
        )?;
        Ok(balance.amount)
    }
}

fn register_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    tokens: Vec<SecretContract>,
    viewing_key: String,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    authorize(env.message.sender.clone(), config.admin)?;
    let mut messages = vec![];
    for token in tokens {
        let token_address_canonical = deps.api.canonical_address(&token.address)?;
        let token_details: Option<RegisteredToken> =
            read_registered_token(&deps.storage, &token_address_canonical);
        if token_details.is_none() {
            let token_details: RegisteredToken = RegisteredToken {
                address: token.address.clone(),
                contract_hash: token.contract_hash.clone(),
                sum_balance: Uint128(0),
            };
            write_registered_token(&mut deps.storage, &token_address_canonical, token_details)?;
            messages.push(snip20::register_receive_msg(
                env.contract_code_hash.clone(),
                None,
                BLOCK_SIZE,
                token.contract_hash.clone(),
                token.address.clone(),
            )?);
        }
        messages.push(snip20::set_viewing_key_msg(
            viewing_key.clone(),
            None,
            BLOCK_SIZE,
            token.contract_hash,
            token.address,
        )?);
    }

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: None,
    })
}

fn rescue_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    denom: Option<String>,
    key: Option<String>,
    token_address: Option<HumanAddr>,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    authorize(env.message.sender.clone(), config.admin.clone())?;

    let mut messages: Vec<CosmosMsg> = vec![];
    if denom.is_some() {
        let balance_response: BalanceResponse =
            deps.querier.query(&QueryRequest::Bank(BankQuery::Balance {
                address: env.contract.address.clone(),
                denom: denom.unwrap(),
            }))?;

        let withdrawal_coins: Vec<Coin> = vec![balance_response.amount];
        messages.push(CosmosMsg::Bank(BankMsg::Send {
            from_address: env.contract.address.clone(),
            to_address: config.admin.clone(),
            amount: withdrawal_coins,
        }));
    }

    if token_address.is_some() && key.is_some() {
        let key: String = key.unwrap();
        let token_address: HumanAddr = token_address.unwrap();
        let registered_token: RegisteredToken =
            read_registered_token(&deps.storage, &deps.api.canonical_address(&token_address)?)
                .unwrap();
        let balance: Uint128 = query_balance_of_token(
            deps,
            env.contract.address.clone(),
            SecretContract {
                address: token_address,
                contract_hash: registered_token.contract_hash.clone(),
            },
            key.to_string(),
        )?;
        let sum_balance: Uint128 = registered_token.sum_balance;
        let difference: Uint128 = (balance - sum_balance)?;
        if !difference.is_zero() {
            messages.push(snip20::transfer_msg(
                config.admin,
                difference,
                None,
                BLOCK_SIZE,
                registered_token.contract_hash,
                registered_token.address,
            )?)
        }
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

fn update_addresses_allowed_to_fill<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    addresses_allowed_to_fill: Vec<HumanAddr>,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY).unwrap();
    authorize(env.message.sender.clone(), config.admin.clone())?;

    config.addresses_allowed_to_fill = addresses_allowed_to_fill;
    if !config
        .addresses_allowed_to_fill
        .contains(&env.contract.address)
    {
        config
            .addresses_allowed_to_fill
            .push(env.contract.address.clone())
    }
    if !config
        .addresses_allowed_to_fill
        .contains(&config.admin.clone())
    {
        config.addresses_allowed_to_fill.push(config.admin.clone())
    }
    config_store.store(CONFIG_KEY, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: None,
    })
}

fn update_order<S: Storage>(store: &mut S, address: &CanonicalAddr, order: Order) -> StdResult<()> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_ORDERS, address.as_slice()], store);
    // Try to access the storage of orders for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let mut store = AppendStoreMut::<Order, _, _>::attach_or_create(&mut store)?;
    store.set_at(order.position, &order)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SecretContract;
    use cosmwasm_std::from_binary;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, MockApi, MockQuerier, MockStorage};

    pub const MOCK_ADMIN: &str = "admin";
    pub const MOCK_VIEWING_KEY: &str = "DELIGHTFUL";

    // === HELPERS ===
    fn create_order_helper<S: Storage, A: Api, Q: Querier>(deps: &mut Extern<S, A, Q>) {
        let receive_msg = ReceiveMsg::CreateOrder {
            butt_viewing_key: MOCK_VIEWING_KEY.to_string(),
            to_amount: Uint128(MOCK_AMOUNT),
            to_token: mock_token().address,
        };
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: Uint128(MOCK_AMOUNT),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        handle(deps, mock_env(mock_butt().address, &[]), handle_msg.clone()).unwrap();
    }

    fn init_helper(
        register_tokens: bool,
    ) -> (
        StdResult<InitResponse>,
        Extern<MockStorage, MockApi, MockQuerier>,
    ) {
        let env = mock_env(MOCK_ADMIN, &[]);
        let mut deps = mock_dependencies(20, &[]);
        let msg = InitMsg { butt: mock_butt() };
        let init_result = init(&mut deps, env.clone(), msg);
        if register_tokens {
            let handle_msg = HandleMsg::RegisterTokens {
                tokens: vec![mock_butt(), mock_token()],
                viewing_key: MOCK_VIEWING_KEY.to_string(),
            };
            handle(&mut deps, mock_env(MOCK_ADMIN, &[]), handle_msg.clone()).unwrap();
        }
        (init_result, deps)
    }

    fn mock_butt() -> SecretContract {
        SecretContract {
            address: HumanAddr::from(MOCK_BUTT_ADDRESS),
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
            address: HumanAddr::from(MOCK_TOKEN_ADDRESS),
            contract_hash: "mock-token-contract-hash".to_string(),
        }
    }

    fn mock_user_address() -> HumanAddr {
        HumanAddr::from("gary")
    }

    // === UNIT TESTS ===
    #[test]
    fn test_cancel_order() {
        let (_init_result, mut deps) = init_helper(true);
        let env = mock_env(mock_butt().address, &[]);

        // when amount sent in is positive
        let receive_msg = ReceiveMsg::CancelOrder { position: 0 };
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: Uint128(1),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(&mut deps, env.clone(), handle_msg);
        // * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Amount sent in must be zero.")
        );

        // when amount sent in is zero
        // = when order at position does not exist
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: Uint128(0),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(&mut deps, env.clone(), handle_msg);

        // = * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("AppendStorage access out of bounds")
        );

        // = when order at position exists
        create_order_helper(&mut deps);
        // == when token used to cancel doesn't match the from_token
        let receive_msg = ReceiveMsg::CancelOrder { position: 0 };
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: Uint128(0),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );
        // == * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Token used to cancel does not match the from token of order.")
        );
        // == when token used to cancel matches the from_token
        // === when order is cancelled
        let mut creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        let mut contract_order = order_at_position(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            creator_order.other_storage_position,
        )
        .unwrap();
        creator_order.cancelled = true;
        contract_order.cancelled = true;
        update_order(
            &mut deps.storage,
            &creator_order.creator.clone(),
            creator_order.clone(),
        )
        .unwrap();
        update_order(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            contract_order.clone(),
        )
        .unwrap();
        // === * it raises an error
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Order already cancelled.")
        );
        // === when order is filled
        creator_order.cancelled = false;
        contract_order.cancelled = false;
        creator_order.from_amount_filled = creator_order.from_amount;
        contract_order.from_amount_filled = contract_order.from_amount;
        update_order(
            &mut deps.storage,
            &creator_order.creator.clone(),
            creator_order.clone(),
        )
        .unwrap();
        update_order(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            contract_order.clone(),
        )
        .unwrap();
        // === * it raises an error
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Order already filled.")
        );
        // === when order can be cancelled
        creator_order.from_amount_filled = Uint128(1);
        contract_order.from_amount_filled = Uint128(1);
        update_order(
            &mut deps.storage,
            &creator_order.creator.clone(),
            creator_order.clone(),
        )
        .unwrap();
        update_order(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            contract_order,
        )
        .unwrap();
        // === * it sends the unfilled from token amount back to the creator
        let from_registered_token: RegisteredToken = read_registered_token(
            &deps.storage,
            &deps
                .api
                .canonical_address(&creator_order.from_token)
                .unwrap(),
        )
        .unwrap();
        let handle_result = handle(&mut deps, env.clone(), handle_msg);
        assert_eq!(
            handle_result.unwrap().messages,
            vec![snip20::transfer_msg(
                deps.api.human_address(&creator_order.creator).unwrap(),
                (creator_order.from_amount - creator_order.from_amount_filled).unwrap(),
                None,
                BLOCK_SIZE,
                from_registered_token.contract_hash,
                from_registered_token.address,
            )
            .unwrap()]
        );
        // === * it sets cancelled to true
        let creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        let contract_order = order_at_position(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            creator_order.other_storage_position,
        )
        .unwrap();
        assert_eq!(creator_order.cancelled, true);
        assert_eq!(contract_order.cancelled, true);

        // ===== * it creates an activity record
        let (activity_records, total) = get_activity_records(
            &deps.storage,
            &deps
                .api
                .canonical_address(&HumanAddr::from(MOCK_ADMIN))
                .unwrap(),
            0,
            50,
        )
        .unwrap();
        assert_eq!(total, 1);
        assert_eq!(
            activity_records[0],
            ActivityRecord {
                position: contract_order.position,
                activity: 0,
                result_from_amount_filled: None,
                result_net_to_amount_filled: None,
                updated_at_block_height: env.block.height.clone(),
                updated_at_block_time: env.block.time
            }
        )
    }

    #[test]
    fn test_config() {
        let (_init_result, deps) = init_helper(false);

        let res = query(&deps, QueryMsg::Config {}).unwrap();
        let value: Config = from_binary(&res).unwrap();
        assert_eq!(
            Config {
                addresses_allowed_to_fill: vec![
                    HumanAddr::from(MOCK_ADMIN),
                    mock_contract().address
                ],
                admin: HumanAddr::from(MOCK_ADMIN),
                butt: mock_butt(),
            },
            value
        );
    }

    #[test]
    fn test_calculate_fee() {
        let amount: Uint128 = Uint128(MOCK_AMOUNT);

        // = when user has a BUTT balance over or equal to 100_000_000_000
        let mut butt_balance: Uint128 = Uint128(100_000_000_000);
        // = * it returns a zero fee
        assert_eq!(calculate_fee(butt_balance, amount).unwrap(), Uint128(0));
        // = when user has a BUTT balance over or equal to 50_000_000_000 and under 100_000_000_000
        butt_balance = Uint128(99_999_999_999);
        let denom: Uint128 = Uint128(10_000);
        // = * it returns the appropriate fee
        assert_eq!(
            calculate_fee(butt_balance, amount).unwrap(),
            amount.multiply_ratio(Uint128(6), denom)
        );
        // = when user has a BUTT balance over or equal to 25_000_000_000 and under 50_000_000_000
        butt_balance = Uint128(49_999_999_999);
        // = * it returns the appropriate fee
        assert_eq!(
            calculate_fee(butt_balance, amount).unwrap(),
            amount.multiply_ratio(Uint128(12), denom)
        );
        // = when user has a BUTT balance over or equal to 12_500_000_000 and under 25_000_000_000
        butt_balance = Uint128(24_999_999_999);
        // = * it returns the appropriate fee
        assert_eq!(
            calculate_fee(butt_balance, amount).unwrap(),
            amount.multiply_ratio(Uint128(18), denom)
        );
        // = when user has a BUTT balance over or equal to 6_250_000_000 and under 12_500_000_000
        butt_balance = Uint128(12_499_999_999);
        // = * it returns the appropriate fee
        assert_eq!(
            calculate_fee(butt_balance, amount).unwrap(),
            amount.multiply_ratio(Uint128(24), denom)
        );
        // = when user has a BUTT balance under 6_250_000_000
        butt_balance = Uint128(6_249_999_999);
        // = * it returns the appropriate fee
        assert_eq!(
            calculate_fee(butt_balance, amount).unwrap(),
            amount.multiply_ratio(Uint128(30), denom)
        );
    }

    #[test]
    fn test_create_order() {
        let (_init_result, mut deps) = init_helper(true);

        // = when to_token isn't registered
        let receive_msg = ReceiveMsg::CreateOrder {
            butt_viewing_key: MOCK_VIEWING_KEY.to_string(),
            to_amount: Uint128(MOCK_AMOUNT),
            to_token: mock_user_address(),
        };
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: Uint128(MOCK_AMOUNT),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        // = * it raises an error
        let handle_result = handle(
            &mut deps,
            mock_env(mock_butt().address, &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("To token is not registered.")
        );

        // = when to_token is registered
        let receive_msg = ReceiveMsg::CreateOrder {
            butt_viewing_key: MOCK_VIEWING_KEY.to_string(),
            to_amount: Uint128(MOCK_AMOUNT),
            to_token: mock_token().address,
        };
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: Uint128(MOCK_AMOUNT),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        // == when user's butt_viewing_key isn't correct
        // -- > Will have to test this live

        // == when user's butt_viewing_key is correct
        // == * it increases the sum_balance for the from_token
        assert_eq!(
            read_registered_token(
                &deps.storage,
                &deps.api.canonical_address(&mock_butt().address).unwrap()
            )
            .unwrap()
            .sum_balance,
            Uint128(0)
        );
        handle(
            &mut deps,
            mock_env(mock_butt().address, &[]),
            handle_msg.clone(),
        )
        .unwrap();
        assert_eq!(
            read_registered_token(
                &deps.storage,
                &deps.api.canonical_address(&mock_butt().address).unwrap()
            )
            .unwrap()
            .sum_balance,
            Uint128(MOCK_AMOUNT)
        );

        // == * it stores the order for the creator
        // == * it stores the order for the smart_contract
        let order: Order = Order {
            position: 0,
            other_storage_position: 0,
            from_token: mock_butt().address,
            to_token: mock_token().address,
            creator: deps.api.canonical_address(&mock_user_address()).unwrap(),
            from_amount: Uint128(MOCK_AMOUNT),
            from_amount_filled: Uint128(0),
            net_to_amount: Uint128(MOCK_AMOUNT),
            net_to_amount_filled: Uint128(0),
            cancelled: false,
            fee: calculate_fee(Uint128(MOCK_AMOUNT), Uint128(MOCK_AMOUNT)).unwrap(),
            created_at_block_time: mock_env(MOCK_ADMIN, &[]).block.time,
            created_at_block_height: mock_env(MOCK_ADMIN, &[]).block.height,
        };
        assert_eq!(
            order_at_position(
                &mut deps.storage,
                &deps.api.canonical_address(&mock_user_address()).unwrap(),
                0
            )
            .unwrap(),
            order
        );
        assert_eq!(
            order_at_position(
                &mut deps.storage,
                &deps
                    .api
                    .canonical_address(&mock_contract().address)
                    .unwrap(),
                0
            )
            .unwrap(),
            order
        )
    }

    #[test]
    fn test_fill_order() {
        let (_init_result, mut deps) = init_helper(true);
        let env = mock_env(mock_butt().address, &[]);

        // when called by a non-admin
        let receive_msg = ReceiveMsg::FillOrder { position: 0 };
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: Uint128(MOCK_AMOUNT),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        // * it raises an error
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );

        // when called by an address that's allowed
        let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
        // = when amount sent in is zero
        let handle_msg = HandleMsg::Receive {
            sender: config.admin.clone(),
            from: env.contract.address.clone(),
            amount: Uint128(0),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(&mut deps, env.clone(), handle_msg);
        // = * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Amount must be greater than zero.")
        );
        // = when amount sent in is positive
        let handle_msg = HandleMsg::Receive {
            sender: config.admin.clone(),
            from: config.admin.clone(),
            amount: Uint128(1),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        // == when order does not exist
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        // == * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("AppendStorage access out of bounds")
        );
        // == when order exists
        create_order_helper(&mut deps);
        // === when to_token does not match the token sent in
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        // === * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("To token does not match the token sent in.")
        );
        // === when to token matches the token sent in
        // ==== when order is cancelled
        let mut creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        let mut contract_order = order_at_position(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            creator_order.other_storage_position,
        )
        .unwrap();
        creator_order.cancelled = true;
        contract_order.cancelled = true;
        update_order(
            &mut deps.storage,
            &creator_order.creator.clone(),
            creator_order.clone(),
        )
        .unwrap();
        update_order(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            contract_order.clone(),
        )
        .unwrap();
        // ==== * it raises an error
        let handle_result = handle(&mut deps, mock_env(mock_token().address, &[]), handle_msg);
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Order already cancelled.")
        );
        // ==== when order is not cancelled
        creator_order.cancelled = false;
        contract_order.cancelled = false;
        update_order(
            &mut deps.storage,
            &creator_order.creator.clone(),
            creator_order.clone(),
        )
        .unwrap();
        update_order(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            contract_order.clone(),
        )
        .unwrap();
        // ===== when amount sent in is greater than unfilled amount
        let handle_msg = HandleMsg::Receive {
            sender: config.admin.clone(),
            from: config.admin.clone(),
            amount: Uint128(MOCK_AMOUNT + 1),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        // ===== * it raises an error
        let handle_result = handle(&mut deps, mock_env(mock_token().address, &[]), handle_msg);
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Amount is greater than unfilled amount.")
        );

        // ===== when amount sent in is less than or equal to the net unfilled to amount
        let handle_msg = HandleMsg::Receive {
            sender: config.admin.clone(),
            from: config.admin.clone(),
            amount: Uint128(MOCK_AMOUNT),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(&mut deps, mock_env(mock_token().address, &[]), handle_msg);
        // ===== * it updates the from amount filled for both orders
        // ===== * it updates the net to amount filled
        let creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        let contract_order = order_at_position(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            creator_order.other_storage_position,
        )
        .unwrap();
        assert_eq!(creator_order.from_amount_filled, creator_order.from_amount);
        assert_eq!(
            contract_order.from_amount_filled,
            contract_order.from_amount
        );
        assert_eq!(
            creator_order.net_to_amount_filled,
            creator_order.net_to_amount
        );
        assert_eq!(
            contract_order.net_to_amount_filled,
            contract_order.net_to_amount
        );

        // ===== * it sends the correct ratio of the from_token to the admin
        // ===== * it sends the amount to the creator
        assert_eq!(
            handle_result.unwrap().messages,
            vec![
                snip20::send_msg(
                    config.admin,
                    Uint128(MOCK_AMOUNT),
                    None,
                    None,
                    BLOCK_SIZE,
                    mock_butt().contract_hash,
                    mock_butt().address,
                )
                .unwrap(),
                snip20::transfer_msg(
                    mock_user_address(),
                    Uint128(MOCK_AMOUNT),
                    None,
                    BLOCK_SIZE,
                    mock_token().contract_hash,
                    mock_token().address,
                )
                .unwrap(),
            ]
        );

        // ===== * it updates the from tokens sum balance
        let from_registered_token: RegisteredToken = read_registered_token(
            &deps.storage,
            &deps
                .api
                .canonical_address(&creator_order.from_token)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(from_registered_token.sum_balance, Uint128(0));
        // ===== * it does not update the to tokens sum balance
        let to_registered_token: RegisteredToken = read_registered_token(
            &deps.storage,
            &deps.api.canonical_address(&creator_order.to_token).unwrap(),
        )
        .unwrap();
        assert_eq!(to_registered_token.sum_balance, Uint128(0));
        // ===== * it creates an activity record
        let res = query(
            &deps,
            QueryMsg::ActivityRecords {
                key: MOCK_VIEWING_KEY.to_string(),
                page: 0,
                page_size: 50,
            },
        )
        .unwrap();
        let query_answer: QueryAnswer = from_binary(&res).unwrap();
        match query_answer {
            QueryAnswer::ActivityRecords {
                activity_records,
                total,
            } => {
                assert_eq!(total, Some(1));
                assert_eq!(
                    activity_records[0],
                    ActivityRecord {
                        position: contract_order.position,
                        activity: 1,
                        result_from_amount_filled: Some(creator_order.from_amount_filled),
                        result_net_to_amount_filled: Some(creator_order.net_to_amount_filled),
                        updated_at_block_height: env.block.height.clone(),
                        updated_at_block_time: env.block.time
                    }
                )
            }
            _ => panic!("unexpected"),
        }
    }

    #[test]
    fn test_handle_first_hop() {
        let (_init_result, mut deps) = init_helper(true);
        let borrow_amount: Uint128 = Uint128(555);
        let mut hops: VecDeque<Hop> = VecDeque::new();
        let first_hop = Hop {
            from_token: mock_butt(),
            trade_smart_contract: mock_contract(),
            position: Some(0),
        };
        hops.push_back(first_hop.clone());
        let handle_msg = HandleMsg::HandleFirstHop {
            borrow_amount: Uint128(555),
            hops: hops.clone(),
        };
        // when called by an address that is not in the addresses allowed to fill
        // * it raises an Unauthorized error
        let handle_result = handle(
            &mut deps,
            mock_env(mock_user_address(), &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );
        // when called by an address that is allowed to fill
        // = when hops is less than 2
        // = * it raises an error
        let handle_result = handle(
            &mut deps,
            mock_env(HumanAddr::from(MOCK_ADMIN), &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Route must be at least 2 hops.")
        );
        // == when there are 2 or more hops
        hops.push_back(Hop {
            from_token: mock_butt(),
            trade_smart_contract: mock_contract(),
            position: Some(1),
        });
        let handle_msg = HandleMsg::HandleFirstHop {
            borrow_amount,
            hops: hops.clone(),
        };
        let handle_unwrapped = handle(
            &mut deps,
            mock_env(HumanAddr::from(MOCK_ADMIN), &[]),
            handle_msg.clone(),
        )
        .unwrap();
        let route_state: RouteState = read_route_state(&deps.storage).unwrap().unwrap();
        // == * it stores the current hop
        assert_eq!(route_state.current_hop.unwrap(), first_hop);
        // == * it stores the borrow amount
        assert_eq!(route_state.remaining_route.borrow_amount, borrow_amount);
        // == * it stores the borrow token as the first hops from_token
        assert_eq!(
            route_state.remaining_route.borrow_token,
            first_hop.from_token
        );
        // == * it stores the address to send left over amount after paying back debt
        assert_eq!(route_state.remaining_route.to, HumanAddr::from(MOCK_ADMIN));
        // == * it stores the remaining hops
        hops.pop_front();
        assert_eq!(route_state.remaining_route.hops, hops);
        // === when first hop is to limit order smart contract
        // === * it sends the token with the right message to the swap contract
        // === * it sends a message to finalize the contract
        assert_eq!(
            handle_unwrapped.messages,
            vec![
                snip20::send_msg(
                    first_hop.trade_smart_contract.address,
                    borrow_amount,
                    Some(
                        to_binary(&ReceiveMsg::FillOrder {
                            position: first_hop.position.unwrap(),
                        })
                        .unwrap()
                    ),
                    None,
                    BLOCK_SIZE,
                    first_hop.from_token.contract_hash,
                    first_hop.from_token.address,
                )
                .unwrap(),
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: mock_contract().address,
                    callback_code_hash: mock_contract().contract_hash,
                    msg: to_binary(&HandleMsg::FinalizeRoute {}).unwrap(),
                    send: vec![],
                })
            ]
        );
        // === when first hop is to swap contract
        let mut hops: VecDeque<Hop> = VecDeque::new();
        let first_hop = Hop {
            from_token: mock_butt(),
            position: None,
            trade_smart_contract: mock_contract(),
        };
        hops.push_back(first_hop.clone());
        hops.push_back(Hop {
            from_token: mock_butt(),
            trade_smart_contract: mock_contract(),
            position: Some(1),
        });
        let handle_msg = HandleMsg::HandleFirstHop {
            borrow_amount,
            hops: hops.clone(),
        };
        let handle_unwrapped = handle(
            &mut deps,
            mock_env(HumanAddr::from(MOCK_ADMIN), &[]),
            handle_msg.clone(),
        )
        .unwrap();
        // === * it sends the token with the right message to the swap contract
        // === * it sends a message to finalize the contract
        assert_eq!(
            handle_unwrapped.messages,
            vec![
                snip20::send_msg(
                    first_hop.trade_smart_contract.address,
                    borrow_amount,
                    Some(
                        to_binary(&Snip20Swap::Swap {
                            expected_return: None,
                            to: Some(mock_contract().address),
                        })
                        .unwrap()
                    ),
                    None,
                    BLOCK_SIZE,
                    first_hop.from_token.contract_hash,
                    first_hop.from_token.address,
                )
                .unwrap(),
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: mock_contract().address,
                    callback_code_hash: mock_contract().contract_hash,
                    msg: to_binary(&HandleMsg::FinalizeRoute {}).unwrap(),
                    send: vec![],
                })
            ]
        );
    }

    #[test]
    fn test_register_tokens() {
        let (_init_result, mut deps) = init_helper(false);

        // When tokens are in the parameter
        let handle_msg = HandleMsg::RegisterTokens {
            tokens: vec![mock_butt(), mock_token()],
            viewing_key: MOCK_VIEWING_KEY.to_string(),
        };
        // = when called by a non-admin
        // = * it raises an Unauthorized error
        let handle_result = handle(
            &mut deps,
            mock_env(mock_user_address(), &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );

        // = when called by the admin
        let handle_result = handle(&mut deps, mock_env(MOCK_ADMIN, &[]), handle_msg.clone());
        let handle_result_unwrapped = handle_result.unwrap();
        // == when tokens are not registered
        // == * it stores the registered tokens
        assert_eq!(
            read_registered_token(
                &deps.storage,
                &deps.api.canonical_address(&mock_butt().address).unwrap()
            )
            .is_some(),
            true
        );
        assert_eq!(
            read_registered_token(
                &deps.storage,
                &deps.api.canonical_address(&mock_token().address).unwrap()
            )
            .is_some(),
            true
        );

        // == * it registers the contract with the tokens
        // == * it sets the viewing key for the contract with the tokens
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
                snip20::set_viewing_key_msg(
                    MOCK_VIEWING_KEY.to_string(),
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
                snip20::set_viewing_key_msg(
                    MOCK_VIEWING_KEY.to_string(),
                    None,
                    BLOCK_SIZE,
                    mock_token().contract_hash,
                    mock_token().address,
                )
                .unwrap()
            ]
        );

        // === context when tokens are registered
        let mut registered_token: RegisteredToken = read_registered_token(
            &deps.storage,
            &deps.api.canonical_address(&mock_token().address).unwrap(),
        )
        .unwrap();
        registered_token.sum_balance = Uint128(MOCK_AMOUNT);
        write_registered_token(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_token().address).unwrap(),
            registered_token,
        )
        .unwrap();
        let handle_result = handle(&mut deps, mock_env(MOCK_ADMIN, &[]), handle_msg);
        let handle_result_unwrapped = handle_result.unwrap();
        // === * it does not change the registered token's sum_balance
        let registered_token: RegisteredToken = read_registered_token(
            &deps.storage,
            &deps.api.canonical_address(&mock_token().address).unwrap(),
        )
        .unwrap();
        assert_eq!(registered_token.sum_balance, Uint128(MOCK_AMOUNT));
        // === * it sets the viewing key for the contract with the tokens
        assert_eq!(
            handle_result_unwrapped.messages,
            vec![
                snip20::set_viewing_key_msg(
                    MOCK_VIEWING_KEY.to_string(),
                    None,
                    BLOCK_SIZE,
                    mock_butt().contract_hash,
                    mock_butt().address,
                )
                .unwrap(),
                snip20::set_viewing_key_msg(
                    MOCK_VIEWING_KEY.to_string(),
                    None,
                    BLOCK_SIZE,
                    mock_token().contract_hash,
                    mock_token().address,
                )
                .unwrap()
            ]
        );
    }

    #[test]
    fn test_rescue_tokens() {
        let (_init_result, mut deps) = init_helper(true);
        let handle_msg = HandleMsg::RescueTokens {
            denom: Some("uscrt".to_string()),
            key: Some(MOCK_VIEWING_KEY.to_string()),
            token_address: Some(mock_butt().address),
        };
        // = when called by a non-admin
        // = * it raises an Unauthorized error
        let handle_result = handle(
            &mut deps,
            mock_env(mock_user_address(), &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );

        // = when called by the admin
        // == when only denom is specified
        let handle_msg = HandleMsg::RescueTokens {
            denom: Some("uscrt".to_string()),
            key: None,
            token_address: None,
        };
        // === when the contract does not have the coin in it
        // === * it sends a transfer with the balance of the coin for the contract
        let handle_result = handle(&mut deps, mock_env(MOCK_ADMIN, &[]), handle_msg.clone());
        let handle_result_unwrapped = handle_result.unwrap();
        assert_eq!(
            handle_result_unwrapped.messages,
            vec![CosmosMsg::Bank(BankMsg::Send {
                from_address: mock_contract().address,
                to_address: HumanAddr(MOCK_ADMIN.to_string()),
                amount: vec![Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(0)
                }],
            })]
        );

        // == when only token address and key are specified
        let handle_msg = HandleMsg::RescueTokens {
            denom: None,
            key: Some(MOCK_VIEWING_KEY.to_string()),
            token_address: Some(mock_butt().address),
        };
        // == * it sends the excess amount of token
        let handle_result = handle(&mut deps, mock_env(MOCK_ADMIN, &[]), handle_msg.clone());
        let handle_result_unwrapped = handle_result.unwrap();
        assert_eq!(
            handle_result_unwrapped.messages,
            vec![snip20::transfer_msg(
                HumanAddr::from(MOCK_ADMIN),
                Uint128(MOCK_AMOUNT),
                None,
                BLOCK_SIZE,
                mock_butt().contract_hash,
                mock_butt().address,
            )
            .unwrap()]
        );
    }

    #[test]
    fn test_update_addresses_allowed_to_fill() {
        let (_init_result, mut deps) = init_helper(false);
        let new_addresses_allowed_to_fill = vec![mock_user_address()];
        let handle_msg = HandleMsg::UpdateAddressesAllowedToFill {
            addresses_allowed_to_fill: new_addresses_allowed_to_fill.clone(),
        };
        let env = mock_env(mock_user_address(), &[]);
        // = when called by a non-admin
        // = * it raises an Unauthorized error
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );

        // = when called by the admin
        // = * it updates the addresses_allowed_to_fill and adds admin and contract address
        let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
        assert_eq!(
            config.addresses_allowed_to_fill,
            vec![config.admin, env.contract.address]
        );
        handle(
            &mut deps,
            mock_env(HumanAddr::from(MOCK_ADMIN), &[]),
            handle_msg,
        )
        .unwrap();
        let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
        let mut adjusted_addresses_allowed_to_fill = new_addresses_allowed_to_fill;
        adjusted_addresses_allowed_to_fill.push(mock_contract().address);
        adjusted_addresses_allowed_to_fill.push(HumanAddr(MOCK_ADMIN.to_string()));
        assert_eq!(
            config.addresses_allowed_to_fill,
            adjusted_addresses_allowed_to_fill
        )
    }
}
