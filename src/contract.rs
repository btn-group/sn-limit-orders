use crate::constants::{
    BLOCK_SIZE, CONFIG_KEY, MOCK_AMOUNT, MOCK_BUTT_ADDRESS, MOCK_TOKEN_ADDRESS,
    PREFIX_CANCEL_RECORDS, PREFIX_CANCEL_RECORDS_COUNT, PREFIX_FILL_RECORDS,
    PREFIX_FILL_RECORDS_COUNT, PREFIX_ORDERS, PREFIX_ORDERS_COUNT,
};
use crate::msg::{HandleMsg, InitMsg, QueryAnswer, QueryMsg, ReceiveMsg, Snip20Swap};
use crate::state::{
    delete_route_state, read_registered_token, read_route_state, store_route_state,
    write_registered_token, ActivityRecord, Config, Hop, HumanizedOrder, Order, RegisteredToken,
    RouteState, SecretContract,
};
use crate::validations::{authorize, validate_human_addr, validate_uint128};
use cosmwasm_std::{
    from_binary, to_binary, Api, BalanceResponse, BankMsg, BankQuery, Binary, CanonicalAddr, Coin,
    CosmosMsg, Env, Extern, HandleResponse, HumanAddr, InitResponse, Querier, QueryRequest,
    ReadonlyStorage, StdError, StdResult, Storage, Uint128, WasmMsg,
};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage};
use primitive_types::U256;
use secret_toolkit::snip20;
use secret_toolkit::storage::{TypedStore, TypedStoreMut};
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
        execution_fee: msg.execution_fee,
        sscrt: msg.sscrt,
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
        HandleMsg::CancelOrder {
            from_token_address,
            position,
        } => cancel_order(deps, &env, from_token_address, position.u128()),
        HandleMsg::HandleFirstHop {
            borrow_amount,
            hops,
            minimum_acceptable_amount,
        } => handle_first_hop(deps, &env, borrow_amount, hops, minimum_acceptable_amount),
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
        HandleMsg::UpdateConfig {
            addresses_allowed_to_fill,
            execution_fee,
        } => update_config(deps, &env, addresses_allowed_to_fill, execution_fee),
    }
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::CancelRecords {
            key,
            page,
            page_size,
        } => activity_records(
            deps,
            key,
            page.u128(),
            page_size.u128(),
            PREFIX_CANCEL_RECORDS,
        ),
        QueryMsg::FillRecords {
            key,
            page,
            page_size,
        } => activity_records(
            deps,
            key,
            page.u128(),
            page_size.u128(),
            PREFIX_FILL_RECORDS,
        ),
        QueryMsg::Config {} => {
            let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;
            Ok(to_binary(&config)?)
        }
        QueryMsg::Orders {
            address,
            key,
            page,
            page_size,
        } => orders(deps, address, key, page.u128(), page_size.u128()),
        QueryMsg::OrdersByPositions {
            address,
            key,
            positions,
        } => orders_by_positions(deps, address, key, positions),
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
            ReceiveMsg::SetExecutionFeeForOrder {} => {
                set_execution_fee_for_order(deps, &env, from, amount)
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
            ReceiveMsg::FillOrder { position } => {
                fill_order(deps, &env, from, amount, position.u128())
            }
        }
    } else {
        handle_hop(deps, &env, from, amount)
    };
    pad_response(response)
}

fn activity_records<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    key: String,
    page: u128,
    page_size: u128,
    storage_prefix: &[u8],
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    // This is here to check the admin's viewing key
    query_balance_of_token(deps, config.admin.clone(), config.butt, key)?;

    let address = deps.api.canonical_address(&config.admin)?;
    let (activity_records, total) =
        get_activity_records(&deps.storage, &address, page, page_size, storage_prefix)?;
    let result = QueryAnswer::ActivityRecords {
        activity_records,
        total: Some(total),
    };
    to_binary(&result)
}

fn prefix_activity_records_count(activity_records_storage_prefix: &[u8]) -> &[u8] {
    if activity_records_storage_prefix == PREFIX_CANCEL_RECORDS {
        PREFIX_CANCEL_RECORDS_COUNT
    } else {
        PREFIX_FILL_RECORDS_COUNT
    }
}

fn set_execution_fee_for_order<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    from: HumanAddr,
    amount: Uint128,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    validate_human_addr(
        &config.sscrt.address,
        &env.message.sender,
        "Execution fee token must be SSCRT.",
    )?;
    validate_uint128(
        config.execution_fee,
        amount,
        "Amount sent in must equal execution fee.",
    )?;

    let contract_canonical_address: CanonicalAddr =
        deps.api.canonical_address(&env.contract.address)?;
    let user_canonical_address: CanonicalAddr = deps.api.canonical_address(&from)?;
    let next_order_position: u128 = storage_count(
        &mut deps.storage,
        &user_canonical_address,
        PREFIX_ORDERS_COUNT,
    )?;
    let order_position: u128 = if next_order_position == 0 {
        return Err(StdError::generic_err("Order does not exist."));
    } else {
        next_order_position - 1
    };
    let mut creator_order =
        order_at_position(&mut deps.storage, &user_canonical_address, order_position)?;
    validate_uint128(
        Uint128::from(creator_order.created_at_block_height),
        Uint128::from(env.block.height),
        "Execution fee must be set at the same block as when order is created.",
    )?;

    if creator_order.execution_fee.is_some() {
        return Err(StdError::generic_err(
            "Execution fee already set for order.",
        ));
    }
    if creator_order.cancelled {
        return Err(StdError::generic_err("Order already cancelled."));
    }
    if creator_order.from_amount == creator_order.from_amount_filled {
        return Err(StdError::generic_err("Order already filled."));
    }

    creator_order.execution_fee = Some(amount);
    update_creator_order_and_associated_contract_order(
        &mut deps.storage,
        &user_canonical_address,
        creator_order.clone(),
        &contract_canonical_address,
    )?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&creator_order.into_humanized(&deps.api)?)?),
    })
}

fn append_activity_record<S: Storage>(
    store: &mut S,
    activity_record: &ActivityRecord,
    for_address: &CanonicalAddr,
    storage_prefix: &[u8],
) -> StdResult<()> {
    let mut prefixed_store =
        PrefixedStorage::multilevel(&[storage_prefix, for_address.as_slice()], store);
    let mut activity_record_store = TypedStoreMut::<ActivityRecord, _>::attach(&mut prefixed_store);
    activity_record_store.store(
        &activity_record.position.u128().to_le_bytes(),
        activity_record,
    )?;
    set_count(
        store,
        for_address,
        prefix_activity_records_count(storage_prefix),
        activity_record.position.u128() + 1,
    )
}

fn set_count<S: Storage>(
    store: &mut S,
    for_address: &CanonicalAddr,
    storage_prefix: &[u8],
    count: u128,
) -> StdResult<()> {
    let mut prefixed_store = PrefixedStorage::new(storage_prefix, store);
    let mut count_store = TypedStoreMut::<u128, _>::attach(&mut prefixed_store);
    count_store.store(for_address.as_slice(), &count)
}

fn append_order<S: Storage>(
    store: &mut S,
    order: &Order,
    for_address: &CanonicalAddr,
) -> StdResult<()> {
    let mut prefixed_store =
        PrefixedStorage::multilevel(&[PREFIX_ORDERS, for_address.as_slice()], store);
    let mut order_store = TypedStoreMut::<Order, _>::attach(&mut prefixed_store);
    order_store.store(&order.position.u128().to_le_bytes(), order)?;
    set_count(
        store,
        for_address,
        PREFIX_ORDERS_COUNT,
        order.position.u128() + 1,
    )
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
    let fee: u128 = if nom == 0 {
        0
    } else {
        (U256::from(to_amount.u128()) * U256::from(nom) / U256::from(10_000)).as_u128()
    };

    return Ok(Uint128(fee));
}

fn cancel_order<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    from_token_address: HumanAddr,
    position: u128,
) -> StdResult<HandleResponse> {
    let contract_canonical_address: CanonicalAddr =
        deps.api.canonical_address(&env.contract.address)?;
    let mut creator_order = order_at_position(
        &mut deps.storage,
        &deps.api.canonical_address(&env.message.sender)?,
        position,
    )?;
    validate_human_addr(
        &creator_order.from_token,
        &from_token_address,
        "From token address does not match the order's from token.",
    )?;
    if creator_order.cancelled {
        return Err(StdError::generic_err("Order already cancelled."));
    }
    if creator_order.from_amount == creator_order.from_amount_filled {
        return Err(StdError::generic_err("Order already filled."));
    }

    let from_token_address_canonical: CanonicalAddr =
        deps.api.canonical_address(&creator_order.from_token)?;
    let mut from_registered_token: RegisteredToken =
        read_registered_token(&deps.storage, &from_token_address_canonical).unwrap();
    let unfilled_amount: Uint128 = (creator_order.from_amount - creator_order.from_amount_filled)?;

    // Update from_registered_token balance
    from_registered_token.sum_balance = (from_registered_token.sum_balance - unfilled_amount)?;
    write_registered_token(
        &mut deps.storage,
        &from_token_address_canonical,
        from_registered_token.clone(),
    )?;

    // Send refund to the creator
    let mut messages: Vec<CosmosMsg> = vec![];
    messages.push(snip20::transfer_msg(
        env.message.sender.clone(),
        unfilled_amount,
        None,
        BLOCK_SIZE,
        from_registered_token.contract_hash,
        from_registered_token.address,
    )?);

    // Update Txs
    creator_order.cancelled = true;
    update_creator_order_and_associated_contract_order(
        &mut deps.storage,
        &creator_order.creator.clone(),
        creator_order.clone(),
        &contract_canonical_address,
    )?;
    // Create activity record
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    let admin_canonical_address: CanonicalAddr = deps.api.canonical_address(&config.admin)?;
    let activity_record: ActivityRecord = ActivityRecord {
        position: Uint128(storage_count(
            &mut deps.storage,
            &admin_canonical_address,
            PREFIX_CANCEL_RECORDS_COUNT,
        )?),
        order_position: creator_order.other_storage_position,
        activity: 0,
        result_from_amount_filled: None,
        result_net_to_amount_filled: None,
        updated_at_block_height: env.block.height,
        updated_at_block_time: env.block.time,
    };
    append_activity_record(
        &mut deps.storage,
        &activity_record,
        &admin_canonical_address,
        PREFIX_CANCEL_RECORDS,
    )?;

    // If order has an execution fee and it has not been spent, send it back to the user
    if creator_order.execution_fee.is_some() && creator_order.from_amount_filled.is_zero() {
        messages.push(snip20::transfer_msg(
            env.message.sender.clone(),
            creator_order.execution_fee.unwrap(),
            None,
            BLOCK_SIZE,
            config.sscrt.contract_hash,
            config.sscrt.address,
        )?);
    }

    pad_response(Ok(HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&creator_order.into_humanized(&deps.api)?)?),
    }))
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
    let contract_order_position =
        storage_count(&mut deps.storage, &contract_address, PREFIX_ORDERS_COUNT)?;
    let creator_order_position =
        storage_count(&mut deps.storage, &creator_address, PREFIX_ORDERS_COUNT)?;
    // Store contract order first
    let mut order = Order {
        position: Uint128(contract_order_position),
        execution_fee: None,
        other_storage_position: Uint128(creator_order_position),
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
    append_order(&mut deps.storage, &order, &contract_address)?;
    // Store creator order next
    order.position = Uint128(creator_order_position);
    order.other_storage_position = Uint128(contract_order_position);
    append_order(&mut deps.storage, &order, &creator_address)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&order.into_humanized(&deps.api)?)?),
    })
}

fn fill_order<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    from: HumanAddr,
    amount: Uint128,
    position: u128,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    authorize(config.addresses_allowed_to_fill, &from)?;
    if amount.is_zero() {
        return Err(StdError::generic_err("Amount must be greater than zero."));
    }

    let contract_canonical_address: CanonicalAddr =
        deps.api.canonical_address(&env.contract.address)?;
    let contract_order =
        order_at_position(&mut deps.storage, &contract_canonical_address, position)?;
    let mut creator_order = order_at_position(
        &mut deps.storage,
        &contract_order.creator,
        contract_order.other_storage_position.u128(),
    )?;
    // Check the token is the same at the to_token
    validate_human_addr(
        &creator_order.to_token,
        &env.message.sender,
        "To token does not match the token sent in.",
    )?;
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

    let mut address_to_send_execution_fee_to: Option<HumanAddr> = None;
    if creator_order.from_amount_filled.is_zero() {
        if creator_order.execution_fee.is_some() {
            address_to_send_execution_fee_to = match read_route_state(&deps.storage)? {
                Some(RouteState { initiator, .. }) => Some(initiator),
                None => Some(from.clone()),
            }
        }
    }
    // Update net_to_amount_filled and from_amount_filled
    creator_order.net_to_amount_filled += amount;
    let from_filled_amount: Uint128 =
        if creator_order.net_to_amount_filled == creator_order.net_to_amount {
            (creator_order.from_amount - creator_order.from_amount_filled)?
        } else {
            Uint128::from(
                (U256::from(creator_order.from_amount.u128()) * U256::from(amount.u128())
                    / U256::from(creator_order.net_to_amount.u128()))
                .as_u128(),
            )
        };
    creator_order.from_amount_filled += from_filled_amount;
    update_creator_order_and_associated_contract_order(
        &mut deps.storage,
        &creator_order.creator,
        creator_order.clone(),
        &contract_canonical_address,
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
    let mut messages: Vec<CosmosMsg> = vec![
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
            deps.api.human_address(&creator_order.creator)?,
            amount,
            None,
            BLOCK_SIZE,
            to_registered_token.contract_hash,
            to_registered_token.address,
        )?,
    ];
    if address_to_send_execution_fee_to.is_some() {
        messages.push(snip20::transfer_msg(
            address_to_send_execution_fee_to.unwrap(),
            creator_order.execution_fee.unwrap(),
            None,
            BLOCK_SIZE,
            config.sscrt.contract_hash,
            config.sscrt.address,
        )?)
    }

    // Update from_token balance
    from_registered_token.sum_balance = (from_registered_token.sum_balance - from_filled_amount)?;
    write_registered_token(
        &mut deps.storage,
        &deps.api.canonical_address(&from_registered_token.address)?,
        from_registered_token,
    )?;

    // Create activity record
    let admin_canonical_address: CanonicalAddr = deps.api.canonical_address(&config.admin)?;
    let activity_record: ActivityRecord = ActivityRecord {
        position: Uint128(storage_count(
            &mut deps.storage,
            &admin_canonical_address,
            PREFIX_FILL_RECORDS_COUNT,
        )?),
        order_position: creator_order.position,
        activity: 1,
        result_from_amount_filled: Some(creator_order.from_amount_filled),
        result_net_to_amount_filled: Some(creator_order.net_to_amount_filled),
        updated_at_block_height: env.block.height,
        updated_at_block_time: env.block.time,
    };
    append_activity_record(
        &mut deps.storage,
        &activity_record,
        &admin_canonical_address,
        PREFIX_FILL_RECORDS,
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
            current_hop,
            remaining_hops,
            ..
        }) => {
            // this function is called only by the route creation function
            // it is intended to always make sure that the route was completed successfully
            // otherwise we revert the transaction
            authorize(vec![env.contract.address.clone()], &env.message.sender)?;
            if remaining_hops.len() != 0 || current_hop.is_some() {
                return Err(StdError::generic_err(format!(
                    "Cannot finalize: route still contains hops."
                )));
            }
            delete_route_state(&mut deps.storage);
            Ok(HandleResponse::default())
        }
        None => Err(StdError::generic_err("No route to finalize")),
    }
}

fn get_activity_records<S: ReadonlyStorage>(
    storage: &S,
    for_address: &CanonicalAddr,
    page: u128,
    page_size: u128,
    storage_prefix: &[u8],
) -> StdResult<(Vec<ActivityRecord>, Uint128)> {
    let total: u128 = storage_count(
        storage,
        for_address,
        prefix_activity_records_count(storage_prefix),
    )?;
    if total == 0 {
        return Ok((vec![], Uint128(0)));
    }
    let max_position: u128 = total - 1;
    let option_highest_position: Option<u128> = max_position.checked_sub((page * page_size).into());
    if option_highest_position.is_none() {
        return Ok((vec![], Uint128(0)));
    }
    let highest_position: u128 = option_highest_position.unwrap();
    let lowest_position: u128 = highest_position
        .checked_sub((page_size - 1).into())
        .unwrap_or(0);
    let store =
        ReadonlyPrefixedStorage::multilevel(&[storage_prefix, for_address.as_slice()], storage);
    let mut activity_records: Vec<ActivityRecord> = Vec::new();
    let store = TypedStore::<ActivityRecord, _>::attach(&store);
    for position in highest_position..=lowest_position {
        activity_records.push(store.load(&position.to_le_bytes())?);
    }

    Ok((activity_records, Uint128(total)))
}

fn get_orders<A: Api, S: ReadonlyStorage>(
    api: &A,
    storage: &S,
    for_address: &CanonicalAddr,
    page: u128,
    page_size: u128,
) -> StdResult<(Vec<HumanizedOrder>, u128)> {
    let total: u128 = storage_count(storage, for_address, PREFIX_ORDERS_COUNT)?;
    if total == 0 {
        return Ok((vec![], 0));
    }

    let max_position: u128 = total - 1;
    let option_highest_position: Option<u128> = max_position.checked_sub((page * page_size).into());
    if option_highest_position.is_none() {
        return Ok((vec![], 0));
    }
    let highest_position: u128 = option_highest_position.unwrap();
    let lowest_position: u128 = highest_position
        .checked_sub((page_size - 1).into())
        .unwrap_or(0);

    let store =
        ReadonlyPrefixedStorage::multilevel(&[PREFIX_ORDERS, for_address.as_slice()], storage);
    let mut orders: Vec<HumanizedOrder> = Vec::new();
    let store = TypedStore::<Order, _>::attach(&store);
    for position in highest_position..=lowest_position {
        orders.push(store.load(&position.to_le_bytes())?.into_humanized(api)?);
    }

    Ok((orders, total))
}

fn storage_count<S: ReadonlyStorage>(
    store: &S,
    for_address: &CanonicalAddr,
    storage_prefix: &[u8],
) -> StdResult<u128> {
    let store = ReadonlyPrefixedStorage::new(storage_prefix, store);
    let store = TypedStore::<u128, _>::attach(&store);
    let position: Option<u128> = store.may_load(for_address.as_slice())?;

    Ok(position.unwrap_or(0))
}

fn handle_first_hop<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    borrow_amount: Uint128,
    mut hops: VecDeque<Hop>,
    minimum_acceptable_amount: Option<Uint128>,
) -> StdResult<HandleResponse> {
    // This is the first msg from the user, with the entire route details
    // 1. save the remaining route to state (e.g. if the route is X/Y -> Y/Z -> Z->W then save Y/Z -> Z/W to state)
    // 2. send `amount` X to pair X/Y
    // 3. call FinalizeRoute to make sure everything went ok, otherwise revert the tx
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    authorize(config.addresses_allowed_to_fill, &env.message.sender)?;
    if hops.len() < 2 {
        return Err(StdError::generic_err("Route must be at least 2 hops."));
    }

    // unwrap is cool because `hops.len() >= 2`
    let first_hop: Hop = hops.pop_front().unwrap();
    let route_state: RouteState = RouteState {
        current_hop: Some(first_hop.clone()),
        remaining_hops: hops,
        borrow_amount,
        borrow_token: first_hop.from_token.clone(),
        initiator: env.message.sender.clone(),
        minimum_acceptable_amount,
    };
    store_route_state(&mut deps.storage, &route_state)?;
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

fn handle_hop<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    from: HumanAddr,
    mut amount: Uint128,
) -> StdResult<HandleResponse> {
    match read_route_state(&deps.storage)? {
        Some(RouteState {
            current_hop,
            remaining_hops: mut hops,
            borrow_amount,
            borrow_token,
            minimum_acceptable_amount,
            initiator,
        }) => {
            validate_human_addr(
                &current_hop.unwrap().trade_smart_contract.address,
                &from,
                "Route called from wrong trade smart contract.",
            )?;

            let mut messages = vec![];
            let popped_hop: Option<Hop> = hops.pop_front();
            if popped_hop.is_some() {
                let next_hop: Hop = popped_hop.clone().unwrap();
                validate_human_addr(
                    &next_hop.from_token.address,
                    &env.message.sender,
                    "Route called by wrong token.",
                )?;

                // if the next hop is this contract
                // check that the amount is less than or equal to that order's unfilled amount
                // only send in the unfilled amount
                // we can rescue the dust later when worthwhile while making gas more predictable
                if next_hop.trade_smart_contract.address == env.contract.address {
                    let next_trade_order = order_at_position(
                        &mut deps.storage,
                        &deps.api.canonical_address(&env.contract.address)?,
                        next_hop.position.unwrap().u128(),
                    )?;
                    let unfilled_amount =
                        (next_trade_order.net_to_amount - next_trade_order.net_to_amount_filled)?;
                    if amount.gt(&unfilled_amount) {
                        amount = unfilled_amount
                    }
                }
                messages.push(snip20::send_msg(
                    next_hop.trade_smart_contract.address.clone(),
                    amount,
                    Some(swap_msg(env.contract.address.clone(), next_hop.clone())?),
                    None,
                    BLOCK_SIZE,
                    next_hop.from_token.contract_hash,
                    next_hop.from_token.address,
                )?);
            } else {
                validate_human_addr(
                    &borrow_token.address,
                    &env.message.sender,
                    "Route called by wrong token.",
                )?;
                if amount.lt(&borrow_amount) {
                    return Err(StdError::generic_err(
                        "Operation fell short of borrow_amount.",
                    ));
                }
                if minimum_acceptable_amount.is_some()
                    && amount.lt(&minimum_acceptable_amount.unwrap())
                {
                    return Err(StdError::generic_err(
                        "Operation fell short of minimum_acceptable_amount.",
                    ));
                }
                // Send fee to initiator
                if amount.gt(&borrow_amount) {
                    messages.push(snip20::transfer_msg(
                        initiator.clone(),
                        (amount - borrow_amount).unwrap(),
                        None,
                        BLOCK_SIZE,
                        borrow_token.contract_hash.clone(),
                        borrow_token.address.clone(),
                    )?);
                }
            }
            store_route_state(
                &mut deps.storage,
                &RouteState {
                    current_hop: popped_hop,
                    remaining_hops: hops,
                    borrow_amount,
                    borrow_token,
                    initiator,
                    minimum_acceptable_amount,
                },
            )?;

            Ok(HandleResponse {
                messages,
                log: vec![],
                data: None,
            })
        }
        None => Err(StdError::generic_err("cannot find route")),
    }
}

fn order_at_position<S: Storage>(
    store: &S,
    address: &CanonicalAddr,
    position: u128,
) -> StdResult<Order> {
    let store = ReadonlyPrefixedStorage::multilevel(&[PREFIX_ORDERS, address.as_slice()], store);
    // Try to access the storage of orders for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let store = TypedStore::<Order, _>::attach(&store);

    store.load(&position.to_le_bytes())
}

fn orders<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: HumanAddr,
    key: String,
    page: u128,
    page_size: u128,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    // This is here so that the user can use their viewing key for butt for this
    query_balance_of_token(deps, address.clone(), config.butt, key)?;

    let (orders, total) = get_orders(
        &deps.api,
        &deps.storage,
        &deps.api.canonical_address(&address)?,
        page,
        page_size,
    )?;

    let result = QueryAnswer::Orders {
        orders,
        total: Some(Uint128(total)),
    };
    to_binary(&result)
}

fn orders_by_positions<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: HumanAddr,
    key: String,
    positions: Vec<Uint128>,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
    query_balance_of_token(deps, address.clone(), config.butt, key)?;

    let address = deps.api.canonical_address(&address)?;
    let mut orders: Vec<HumanizedOrder> = vec![];
    for position in positions.iter() {
        let order = order_at_position(&deps.storage, &address, position.u128())?;
        orders.push(order.into_humanized(&deps.api)?)
    }

    let result = QueryAnswer::Orders {
        orders,
        total: None,
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
    authorize(vec![config.admin], &env.message.sender)?;
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
    authorize(vec![config.admin.clone()], &env.message.sender)?;

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

fn update_config<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    addresses_allowed_to_fill: Option<Vec<HumanAddr>>,
    execution_fee: Option<Uint128>,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY).unwrap();
    authorize(vec![config.admin.clone()], &env.message.sender)?;

    if addresses_allowed_to_fill.is_some() {
        let new_addresses_allowed_to_fill = addresses_allowed_to_fill.unwrap();
        config.addresses_allowed_to_fill = new_addresses_allowed_to_fill;
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
    }
    if execution_fee.is_some() {
        config.execution_fee = execution_fee.unwrap();
    }
    config_store.store(CONFIG_KEY, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: None,
    })
}

fn update_creator_order_and_associated_contract_order<S: Storage>(
    store: &mut S,
    user_address: &CanonicalAddr,
    creator_order: Order,
    contract_address: &CanonicalAddr,
) -> StdResult<()> {
    let mut user_store =
        PrefixedStorage::multilevel(&[PREFIX_ORDERS, user_address.as_slice()], store);
    // Try to access the storage of orders for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let mut user_store = TypedStoreMut::<Order, _, _>::attach(&mut user_store);
    user_store.store(&creator_order.position.u128().to_le_bytes(), &creator_order)?;
    let mut contract_store =
        PrefixedStorage::multilevel(&[PREFIX_ORDERS, contract_address.as_slice()], store);
    let mut contract_store = TypedStoreMut::<Order, _, _>::attach(&mut contract_store);
    let mut contract_order: Order = creator_order.clone();
    contract_order.position = creator_order.other_storage_position;
    contract_order.other_storage_position = creator_order.position;
    contract_store.store(
        &contract_order.position.u128().to_le_bytes(),
        &contract_order,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SecretContract;
    use cosmwasm_std::from_binary;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::StdError::NotFound;

    pub const MOCK_ADMIN: &str = "admin";
    pub const MOCK_VIEWING_KEY: &str = "DELIGHTFUL";
    pub const MOCK_SSCRT_ADDRESS: &str = "mock-sscrt-address";

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
        let msg = InitMsg {
            butt: mock_butt(),
            execution_fee: mock_execution_fee(),
            sscrt: mock_sscrt(),
        };
        let init_result = init(&mut deps, env.clone(), msg);
        if register_tokens {
            let handle_msg = HandleMsg::RegisterTokens {
                tokens: vec![mock_butt(), mock_token()],
                viewing_key: MOCK_VIEWING_KEY.to_string(),
            };
            handle(&mut deps, env, handle_msg.clone()).unwrap();
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

    fn mock_execution_fee() -> Uint128 {
        Uint128(5_555)
    }

    fn mock_sscrt() -> SecretContract {
        SecretContract {
            address: HumanAddr::from(MOCK_SSCRT_ADDRESS),
            contract_hash: "mock-sscrt-contract-hash".to_string(),
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
    fn test_set_execution_fee_for_order() {
        let (_init_result, mut deps) = init_helper(true);
        let mut env = mock_env(mock_butt().address, &[]);

        // when token sent in is not sscrt
        let receive_msg = ReceiveMsg::SetExecutionFeeForOrder {};
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: Uint128(1),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        // * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Execution fee token must be SSCRT.")
        );

        // when token sent in is sscrt
        env = mock_env(mock_sscrt().address, &[]);
        // = when amount sent in is not equal to execution fee
        let handle_result = handle(&mut deps, env.clone(), handle_msg);
        // * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Amount sent in must equal execution fee.")
        );
        // = when amount sent in is equal to execution fee
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: mock_execution_fee(),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(&mut deps, env.clone(), handle_msg);
        // == when user does not have any orders
        // === * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Order does not exist.")
        );
        // == when user has at least one order
        create_order_helper(&mut deps);
        // === when current block is the same as the block when the order is created
        // ==== when order has fee set already
        let mut creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        creator_order.execution_fee = Some(Uint128(1));
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        // ==== * it raises an error
        let handle_msg = HandleMsg::Receive {
            sender: mock_user_address(),
            from: mock_user_address(),
            amount: mock_execution_fee(),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Execution fee already set for order.")
        );
        // ==== when order does not have execution fee set already
        // ===== when order is cancelled
        creator_order.execution_fee = None;
        creator_order.cancelled = true;
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        // ===== * it raises an error
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Order already cancelled.")
        );
        // ===== when order is filled
        creator_order.cancelled = false;
        creator_order.from_amount_filled = creator_order.from_amount;
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        // ===== * it raises an error
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Order already filled.")
        );
        // ===== when order is open
        creator_order.from_amount_filled = Uint128(0);
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        // ===== * it sets the execution fee for the user order
        let handle_unwrapped = handle(&mut deps, env.clone(), handle_msg.clone()).unwrap();
        creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        assert_eq!(creator_order.execution_fee, Some(mock_execution_fee()));
        // ===== * it sets the execution fee for the contract order
        let contract_order = order_at_position(
            &mut deps.storage,
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
            creator_order.other_storage_position.u128(),
        )
        .unwrap();
        assert_eq!(contract_order.execution_fee, Some(mock_execution_fee()));
        // ===== * it sends the humanized creator order back as data
        assert_eq!(
            handle_unwrapped.data,
            pad_response(Ok(HandleResponse {
                messages: vec![],
                log: vec![],
                data: Some(
                    to_binary(&creator_order.clone().into_humanized(&deps.api).unwrap()).unwrap()
                ),
            }))
            .unwrap()
            .data
        );

        // === when current block is different from the block when the order is created
        let mut creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        creator_order.created_at_block_height = 1;
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        // === * it raises an error
        let handle_result = handle(&mut deps, env.clone(), handle_msg);
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err(
                "Execution fee must be set at the same block as when order is created."
            )
        );
    }

    #[test]
    fn test_cancel_order() {
        let (_init_result, mut deps) = init_helper(true);
        let env = mock_env(mock_user_address(), &[]);

        // = when order at position does not exist
        let mut handle_msg = HandleMsg::CancelOrder {
            from_token_address: mock_token().address,
            position: Uint128(0),
        };
        let mut handle_result = handle(&mut deps, env.clone(), handle_msg.clone());

        // = * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            NotFound {
                kind: "cw_secret_network_limit_orders::state::Order".to_string(),
                backtrace: None
            }
        );

        // = when order at position exists
        create_order_helper(&mut deps);
        // == when token used to cancel doesn't match the from_token
        handle_result = handle(&mut deps, env.clone(), handle_msg);
        // == * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("From token address does not match the order's from token.")
        );
        // == when token used to cancel matches the from_token
        handle_msg = HandleMsg::CancelOrder {
            from_token_address: mock_butt().address,
            position: Uint128(0),
        };
        // === when order is cancelled
        let mut creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        creator_order.cancelled = true;
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        // === * it raises an error
        handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Order already cancelled.")
        );
        // === when order is filled
        creator_order.cancelled = false;
        creator_order.from_amount_filled = creator_order.from_amount;
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        // === * it raises an error
        handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Order already filled.")
        );
        // === when order can be cancelled
        creator_order.from_amount_filled = Uint128(5);
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        let from_registered_token: RegisteredToken = read_registered_token(
            &deps.storage,
            &deps
                .api
                .canonical_address(&creator_order.from_token)
                .unwrap(),
        )
        .unwrap();
        let sum_balance_before_cancel: Uint128 = from_registered_token.sum_balance;
        handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        // === * it reduces the from token's sum_balance by the unfilled amount
        let from_registered_token: RegisteredToken = read_registered_token(
            &deps.storage,
            &deps
                .api
                .canonical_address(&creator_order.from_token)
                .unwrap(),
        )
        .unwrap();
        let unfilled_amount: Uint128 =
            (creator_order.from_amount - creator_order.from_amount_filled).unwrap();
        assert_eq!(
            from_registered_token.sum_balance,
            (sum_balance_before_cancel - unfilled_amount).unwrap()
        );

        // === * it sends the unfilled from token amount back to the creator
        let handle_result_unwrapped = handle_result.unwrap();
        assert_eq!(
            handle_result_unwrapped.messages,
            vec![snip20::transfer_msg(
                deps.api.human_address(&creator_order.creator).unwrap(),
                unfilled_amount,
                None,
                BLOCK_SIZE,
                from_registered_token.contract_hash.clone(),
                from_registered_token.address.clone(),
            )
            .unwrap()]
        );
        // === * it sends the creator order as humanized back as data
        let creator_order = order_at_position(
            &mut deps.storage,
            &deps.api.canonical_address(&mock_user_address()).unwrap(),
            0,
        )
        .unwrap();
        assert_eq!(
            handle_result_unwrapped.data,
            pad_response(Ok(HandleResponse {
                messages: vec![],
                log: vec![],
                data: Some(
                    to_binary(&creator_order.clone().into_humanized(&deps.api).unwrap()).unwrap()
                ),
            }))
            .unwrap()
            .data
        );

        // === * it sets cancelled to true
        let mut creator_order = order_at_position(
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
            creator_order.other_storage_position.u128(),
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
            PREFIX_CANCEL_RECORDS,
        )
        .unwrap();
        assert_eq!(total, Uint128(1));
        assert_eq!(
            activity_records[0],
            ActivityRecord {
                position: Uint128(0),
                order_position: contract_order.position,
                activity: 0,
                result_from_amount_filled: None,
                result_net_to_amount_filled: None,
                updated_at_block_height: env.block.height.clone(),
                updated_at_block_time: env.block.time
            }
        );
        // ==== when order has an execution fee
        creator_order.execution_fee = Some(Uint128(1));
        creator_order.cancelled = false;
        creator_order.from_amount_filled = Uint128(999999999999);
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();
        // ===== when order is partially filled
        // ===== * it does not send the execution fee back to the creator
        handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap().messages,
            vec![snip20::transfer_msg(
                deps.api.human_address(&creator_order.creator).unwrap(),
                (creator_order.from_amount - creator_order.from_amount_filled).unwrap(),
                None,
                BLOCK_SIZE,
                from_registered_token.contract_hash.clone(),
                from_registered_token.address.clone(),
            )
            .unwrap()]
        );

        // ===== when order has not been partially filled
        creator_order.from_amount = Uint128(1);
        creator_order.execution_fee = Some(Uint128(1));
        creator_order.cancelled = false;
        creator_order.from_amount_filled = Uint128(0);
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();

        // ===== * it sends the execution fee back to the creator
        handle_result = handle(&mut deps, env.clone(), handle_msg);
        assert_eq!(
            handle_result.unwrap().messages,
            vec![
                snip20::transfer_msg(
                    deps.api.human_address(&creator_order.creator).unwrap(),
                    (creator_order.from_amount - creator_order.from_amount_filled).unwrap(),
                    None,
                    BLOCK_SIZE,
                    from_registered_token.contract_hash,
                    from_registered_token.address,
                )
                .unwrap(),
                snip20::transfer_msg(
                    deps.api.human_address(&creator_order.creator).unwrap(),
                    creator_order.execution_fee.unwrap(),
                    None,
                    BLOCK_SIZE,
                    mock_sscrt().contract_hash,
                    mock_sscrt().address,
                )
                .unwrap()
            ]
        );
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
                execution_fee: mock_execution_fee(),
                sscrt: mock_sscrt(),
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

        // == when order is created
        // === * it increases the sum balance for the from_token
        assert_eq!(
            read_registered_token(
                &deps.storage,
                &deps.api.canonical_address(&mock_butt().address).unwrap()
            )
            .unwrap()
            .sum_balance,
            Uint128(0)
        );
        let handle_unwrapped = handle(
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
        // === * it sends the humanized creator order back as data
        let order: Order = Order {
            position: Uint128(0),
            execution_fee: None,
            other_storage_position: Uint128(0),
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
            handle_unwrapped.data,
            pad_response(Ok(HandleResponse {
                messages: vec![],
                log: vec![],
                data: Some(to_binary(&order.clone().into_humanized(&deps.api).unwrap()).unwrap()),
            }))
            .unwrap()
            .data
        );

        // === * it stores the order for the creator
        // === * it stores the order for the smart_contract
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
        let receive_msg = ReceiveMsg::FillOrder {
            position: Uint128(0),
        };
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
            NotFound {
                kind: "cw_secret_network_limit_orders::state::Order".to_string(),
                backtrace: None
            }
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
        creator_order.cancelled = true;
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
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
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
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
            amount: Uint128(MOCK_AMOUNT / 2),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );
        // ===== * it updates the from amount filled for both orders
        // ===== * it updates the net to amount filled
        let mut creator_order = order_at_position(
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
            creator_order.other_storage_position.u128(),
        )
        .unwrap();
        assert_eq!(
            creator_order.from_amount_filled,
            creator_order
                .from_amount
                .multiply_ratio(Uint128(1), Uint128(2))
        );
        assert_eq!(
            contract_order.from_amount_filled,
            contract_order
                .from_amount
                .multiply_ratio(Uint128(1), Uint128(2))
        );
        assert_eq!(
            creator_order.net_to_amount_filled,
            creator_order
                .net_to_amount
                .multiply_ratio(Uint128(1), Uint128(2))
        );
        assert_eq!(
            contract_order.net_to_amount_filled,
            contract_order
                .net_to_amount
                .multiply_ratio(Uint128(1), Uint128(2))
        );

        // ===== * it sends the correct ratio of the from_token to the admin
        // ===== * it sends the amount to the creator
        assert_eq!(
            handle_result.unwrap().messages,
            vec![
                snip20::send_msg(
                    config.admin.clone(),
                    Uint128(MOCK_AMOUNT / 2),
                    None,
                    None,
                    BLOCK_SIZE,
                    mock_butt().contract_hash,
                    mock_butt().address,
                )
                .unwrap(),
                snip20::transfer_msg(
                    mock_user_address(),
                    Uint128(MOCK_AMOUNT / 2),
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
        assert_eq!(
            from_registered_token.sum_balance,
            creator_order
                .net_to_amount
                .multiply_ratio(Uint128(1), Uint128(2))
        );
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
            QueryMsg::FillRecords {
                key: MOCK_VIEWING_KEY.to_string(),
                page: Uint128(0),
                page_size: Uint128(50),
            },
        )
        .unwrap();
        let query_answer: QueryAnswer = from_binary(&res).unwrap();
        match query_answer {
            QueryAnswer::ActivityRecords {
                activity_records,
                total,
            } => {
                assert_eq!(total, Some(Uint128(1)));
                assert_eq!(
                    activity_records[0],
                    ActivityRecord {
                        position: Uint128(0),
                        order_position: contract_order.position,
                        activity: 1,
                        result_from_amount_filled: Some(creator_order.from_amount_filled),
                        result_net_to_amount_filled: Some(creator_order.net_to_amount_filled),
                        updated_at_block_height: env.block.height.clone(),
                        updated_at_block_time: env.block.time
                    }
                )
            }
            _ => panic!("unexpected"),
        };
        // ====== when order has an execution fee
        creator_order.execution_fee = Some(Uint128(1));
        // ======= when order is partially filled
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();

        // ====== * it does not send the execution fee
        let handle_msg = HandleMsg::Receive {
            sender: config.admin.clone(),
            from: config.admin.clone(),
            amount: Uint128(MOCK_AMOUNT / 4),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(&mut deps, mock_env(mock_token().address, &[]), handle_msg);
        assert_eq!(
            handle_result.unwrap().messages,
            vec![
                snip20::send_msg(
                    config.admin.clone(),
                    Uint128(MOCK_AMOUNT / 4),
                    None,
                    None,
                    BLOCK_SIZE,
                    mock_butt().contract_hash,
                    mock_butt().address,
                )
                .unwrap(),
                snip20::transfer_msg(
                    mock_user_address(),
                    Uint128(MOCK_AMOUNT / 4),
                    None,
                    BLOCK_SIZE,
                    mock_token().contract_hash,
                    mock_token().address,
                )
                .unwrap(),
            ]
        );

        // ======= when order is completely unfilled
        creator_order.from_amount_filled = Uint128(0);
        creator_order.net_to_amount_filled = Uint128(0);
        creator_order.execution_fee = Some(Uint128(1));
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();

        // ======== when route state exists
        let route_state: RouteState = RouteState {
            current_hop: None,
            remaining_hops: VecDeque::new(),
            borrow_amount: Uint128(5),
            borrow_token: mock_sscrt(),
            initiator: mock_contract().address,
            minimum_acceptable_amount: Some(Uint128(5)),
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();
        // ======== * it sends the execution fee to the route initiator
        let handle_msg = HandleMsg::Receive {
            sender: config.admin.clone(),
            from: config.admin.clone(),
            amount: Uint128(MOCK_AMOUNT / 8),
            msg: Some(to_binary(&receive_msg).unwrap()),
        };
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap().messages,
            vec![
                snip20::send_msg(
                    config.admin.clone(),
                    Uint128(MOCK_AMOUNT / 8),
                    None,
                    None,
                    BLOCK_SIZE,
                    mock_butt().contract_hash,
                    mock_butt().address,
                )
                .unwrap(),
                snip20::transfer_msg(
                    mock_user_address(),
                    Uint128(MOCK_AMOUNT / 8),
                    None,
                    BLOCK_SIZE,
                    mock_token().contract_hash,
                    mock_token().address,
                )
                .unwrap(),
                snip20::transfer_msg(
                    mock_contract().address,
                    creator_order.execution_fee.unwrap(),
                    None,
                    BLOCK_SIZE,
                    mock_sscrt().contract_hash,
                    mock_sscrt().address,
                )
                .unwrap(),
            ]
        );

        // ======== when route state does not exist
        creator_order.from_amount_filled = Uint128(0);
        creator_order.net_to_amount_filled = Uint128(0);
        creator_order.execution_fee = Some(Uint128(1));
        update_creator_order_and_associated_contract_order(
            &mut deps.storage,
            &creator_order.creator,
            creator_order.clone(),
            &deps
                .api
                .canonical_address(&mock_contract().address)
                .unwrap(),
        )
        .unwrap();

        delete_route_state(&mut deps.storage);
        // ======== * it sends the execution fee to the user calling the function
        let handle_result = handle(&mut deps, mock_env(mock_token().address, &[]), handle_msg);
        assert_eq!(
            handle_result.unwrap().messages,
            vec![
                snip20::send_msg(
                    config.admin.clone(),
                    Uint128(MOCK_AMOUNT / 8),
                    None,
                    None,
                    BLOCK_SIZE,
                    mock_butt().contract_hash,
                    mock_butt().address,
                )
                .unwrap(),
                snip20::transfer_msg(
                    mock_user_address(),
                    Uint128(MOCK_AMOUNT / 8),
                    None,
                    BLOCK_SIZE,
                    mock_token().contract_hash,
                    mock_token().address,
                )
                .unwrap(),
                snip20::transfer_msg(
                    config.admin,
                    creator_order.execution_fee.unwrap(),
                    None,
                    BLOCK_SIZE,
                    mock_sscrt().contract_hash,
                    mock_sscrt().address,
                )
                .unwrap(),
            ]
        );
    }

    #[test]
    fn test_finalize_route() {
        let (_init_result, mut deps) = init_helper(true);
        let env = mock_env(mock_user_address(), &[]);

        // when route state does not exist
        // * it raises an error
        let handle_msg = HandleMsg::FinalizeRoute {};
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("No route to finalize")
        );

        // when route state exists
        // = when there are hops
        let mut hops: VecDeque<Hop> = VecDeque::new();
        hops.push_back(Hop {
            from_token: mock_token(),
            trade_smart_contract: mock_contract(),
            position: Some(Uint128(2)),
        });
        let route_state: RouteState = RouteState {
            current_hop: Some(Hop {
                from_token: mock_token(),
                trade_smart_contract: mock_contract(),
                position: Some(Uint128(1)),
            }),
            remaining_hops: hops,
            borrow_token: mock_token(),
            borrow_amount: Uint128(1_000_000),
            initiator: mock_user_address(),
            minimum_acceptable_amount: None,
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();
        // == when it isn't called by the contract
        // == * it raises an error
        let handle_result = handle(&mut deps, env.clone(), handle_msg.clone());
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::Unauthorized { backtrace: None }
        );
        // == when it's called by the contract
        // == * it raises an error
        let handle_result = handle(
            &mut deps,
            mock_env(mock_contract().address, &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err(format!("Cannot finalize: route still contains hops."))
        );

        // === when there are no hops but there is a current_hop
        let hops: VecDeque<Hop> = VecDeque::new();
        let route_state: RouteState = RouteState {
            current_hop: Some(Hop {
                from_token: mock_token(),
                trade_smart_contract: mock_contract(),
                position: Some(Uint128(1)),
            }),
            remaining_hops: hops,
            borrow_token: mock_token(),
            borrow_amount: Uint128(1_000_000),
            initiator: mock_user_address(),
            minimum_acceptable_amount: None,
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();
        // === * it raises an error
        let handle_result = handle(
            &mut deps,
            mock_env(mock_contract().address, &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err(format!("Cannot finalize: route still contains hops."))
        );
        // ==== when there are no hops and no current_hop
        let hops: VecDeque<Hop> = VecDeque::new();
        let route_state: RouteState = RouteState {
            current_hop: None,
            remaining_hops: hops,
            borrow_token: mock_token(),
            borrow_amount: Uint128(1_000_000),
            initiator: mock_user_address(),
            minimum_acceptable_amount: None,
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();

        // ==== * it returns an Ok response
        handle(
            &mut deps,
            mock_env(mock_contract().address, &[]),
            handle_msg.clone(),
        )
        .unwrap();
        // ==== * it deletes the route state
        assert_eq!(read_route_state(&deps.storage).unwrap().is_none(), true);
    }

    #[test]
    fn test_handle_first_hop() {
        let (_init_result, mut deps) = init_helper(true);
        let borrow_amount: Uint128 = Uint128(555);
        let mut hops: VecDeque<Hop> = VecDeque::new();
        let first_hop = Hop {
            from_token: mock_butt(),
            trade_smart_contract: mock_contract(),
            position: Some(Uint128(0)),
        };
        hops.push_back(first_hop.clone());
        let handle_msg = HandleMsg::HandleFirstHop {
            borrow_amount,
            hops: hops.clone(),
            minimum_acceptable_amount: Some(borrow_amount),
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
            position: Some(Uint128(1)),
        });
        let handle_msg = HandleMsg::HandleFirstHop {
            borrow_amount,
            hops: hops.clone(),
            minimum_acceptable_amount: Some(borrow_amount),
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
        assert_eq!(route_state.borrow_amount, borrow_amount);
        // == * it stores the borrow token as the first hops from_token
        assert_eq!(route_state.borrow_token, first_hop.from_token);
        // == * it stores the address to send left over amount after paying back debt
        assert_eq!(route_state.initiator, HumanAddr::from(MOCK_ADMIN));
        // == * it stores the remaining hops
        hops.pop_front();
        assert_eq!(route_state.remaining_hops, hops);
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
            position: Some(Uint128(1)),
        });
        let handle_msg = HandleMsg::HandleFirstHop {
            borrow_amount,
            hops: hops.clone(),
            minimum_acceptable_amount: Some(borrow_amount),
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
    fn test_handle_hop() {
        let (_init_result, mut deps) = init_helper(true);
        let borrow_amount: Uint128 = Uint128(MOCK_AMOUNT);
        let borrow_token: SecretContract = mock_butt();
        let minimum_acceptable_amount: Uint128 = borrow_amount + Uint128(1);
        create_order_helper(&mut deps);
        create_order_helper(&mut deps);

        // when route state does not exist
        // * it raises an error
        let handle_msg = HandleMsg::Receive {
            sender: mock_contract().address,
            from: mock_contract().address,
            amount: Uint128(MOCK_AMOUNT),
            msg: None,
        };
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("cannot find route")
        );

        // when route state exists
        let mut hops: VecDeque<Hop> = VecDeque::new();
        hops.push_back(Hop {
            from_token: mock_token(),
            trade_smart_contract: mock_contract(),
            position: Some(Uint128(1)),
        });
        let route_state: RouteState = RouteState {
            current_hop: Some(Hop {
                from_token: mock_butt(),
                trade_smart_contract: mock_contract(),
                position: Some(Uint128(2)),
            }),
            remaining_hops: hops,
            borrow_token: borrow_token.clone(),
            borrow_amount,
            initiator: mock_user_address(),
            minimum_acceptable_amount: Some(borrow_amount),
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();

        // = when not called by the current hop's trade smart contract
        // = * it raises an error
        let handle_msg = HandleMsg::Receive {
            sender: mock_butt().address,
            from: mock_butt().address,
            amount: Uint128(MOCK_AMOUNT),
            msg: None,
        };
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Route called from wrong trade smart contract.")
        );

        // = when from current hop's trade smart contract
        let handle_msg = HandleMsg::Receive {
            sender: mock_contract().address,
            from: mock_contract().address,
            amount: Uint128(MOCK_AMOUNT),
            msg: None,
        };
        // == when there are hops
        // === when called by a token different from the next hops from token
        // === * it raises an error
        let handle_result = handle(
            &mut deps,
            mock_env(mock_butt().address, &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Route called by wrong token.")
        );
        // === when called by the from token of the next hop
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );
        // === * it stores the route state with the appropriate info
        let route_state: RouteState = read_route_state(&deps.storage).unwrap().unwrap();
        assert_eq!(
            route_state,
            RouteState {
                current_hop: Some(Hop {
                    from_token: mock_token(),
                    trade_smart_contract: mock_contract(),
                    position: Some(Uint128(1)),
                }),
                borrow_token: borrow_token.clone(),
                remaining_hops: VecDeque::new(),
                borrow_amount,
                initiator: mock_user_address(),
                minimum_acceptable_amount: Some(borrow_amount),
            }
        );

        // ==== when the next hop has a position value
        // ===== when amount received is equal to or less than the next order's unfilled amount
        // ===== * it sends the amount received to the next hop trade smart contract with the correct details
        assert_eq!(
            handle_result.unwrap().messages,
            vec![snip20::send_msg(
                mock_contract().address,
                Uint128(MOCK_AMOUNT),
                Some(
                    to_binary(&ReceiveMsg::FillOrder {
                        position: Uint128(1)
                    })
                    .unwrap()
                ),
                None,
                BLOCK_SIZE,
                mock_token().contract_hash,
                mock_token().address,
            )
            .unwrap()]
        );

        // ===== when amount received is greater than the next order's unfilled amount
        let mut hops: VecDeque<Hop> = VecDeque::new();
        hops.push_back(Hop {
            from_token: mock_token(),
            trade_smart_contract: mock_contract(),
            position: Some(Uint128(1)),
        });
        let route_state: RouteState = RouteState {
            current_hop: Some(Hop {
                from_token: mock_butt(),
                trade_smart_contract: mock_contract(),
                position: Some(Uint128(2)),
            }),
            remaining_hops: hops,
            borrow_token: borrow_token.clone(),
            borrow_amount,
            initiator: mock_user_address(),
            minimum_acceptable_amount: Some(borrow_amount),
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();
        let handle_msg = HandleMsg::Receive {
            sender: mock_contract().address,
            from: mock_contract().address,
            amount: Uint128(MOCK_AMOUNT + 1),
            msg: None,
        };
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );

        // ===== * it sends the next order's unfilled amount
        assert_eq!(
            handle_result.unwrap().messages,
            vec![snip20::send_msg(
                mock_contract().address,
                Uint128(MOCK_AMOUNT),
                Some(
                    to_binary(&ReceiveMsg::FillOrder {
                        position: Uint128(1)
                    })
                    .unwrap()
                ),
                None,
                BLOCK_SIZE,
                mock_token().contract_hash,
                mock_token().address,
            )
            .unwrap()]
        );

        // ==== when the next hop does not have a position value
        let mut hops: VecDeque<Hop> = VecDeque::new();
        hops.push_back(Hop {
            from_token: mock_token(),
            trade_smart_contract: mock_butt(),
            position: None,
        });
        let route_state: RouteState = RouteState {
            current_hop: Some(Hop {
                from_token: mock_butt(),
                trade_smart_contract: mock_contract(),
                position: Some(Uint128(2)),
            }),
            remaining_hops: hops,
            borrow_token: borrow_token.clone(),
            borrow_amount,
            initiator: mock_user_address(),
            minimum_acceptable_amount: Some(borrow_amount),
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();
        // ==== * it sends the amount received to the next hop trade smart contract with the correct details
        let handle_msg = HandleMsg::Receive {
            sender: mock_contract().address,
            from: mock_contract().address,
            amount: Uint128(MOCK_AMOUNT),
            msg: None,
        };
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );
        assert_eq!(
            handle_result.unwrap().messages,
            vec![snip20::send_msg(
                mock_butt().address,
                Uint128(MOCK_AMOUNT),
                Some(
                    to_binary(&Snip20Swap::Swap {
                        expected_return: None,
                        to: Some(mock_contract().address)
                    })
                    .unwrap()
                ),
                None,
                BLOCK_SIZE,
                mock_token().contract_hash,
                mock_token().address,
            )
            .unwrap()]
        );

        // == when there are are no hops
        let hops: VecDeque<Hop> = VecDeque::new();
        let route_state: RouteState = RouteState {
            current_hop: Some(Hop {
                from_token: mock_butt(),
                trade_smart_contract: mock_contract(),
                position: Some(Uint128(2)),
            }),
            remaining_hops: hops.clone(),
            borrow_token: borrow_token.clone(),
            borrow_amount,
            initiator: mock_user_address(),
            minimum_acceptable_amount: Some(minimum_acceptable_amount),
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();
        // === when not called by the borrowed token
        let handle_result = handle(
            &mut deps,
            mock_env(mock_token().address, &[]),
            handle_msg.clone(),
        );
        // === * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Route called by wrong token.")
        );
        // === when called by the borrowed token
        // ==== when amount sent in is less than the minimum acceptable amount
        let handle_msg = HandleMsg::Receive {
            sender: mock_contract().address,
            from: mock_contract().address,
            amount: borrow_amount,
            msg: None,
        };
        let handle_result = handle(
            &mut deps,
            mock_env(borrow_token.address.clone(), &[]),
            handle_msg.clone(),
        );
        // ==== * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Operation fell short of minimum_acceptable_amount.")
        );

        // ==== when amount sent in is less than the borrowed amount
        let handle_msg = HandleMsg::Receive {
            sender: mock_contract().address,
            from: mock_contract().address,
            amount: (borrow_amount - Uint128(1)).unwrap(),
            msg: None,
        };
        let handle_result = handle(
            &mut deps,
            mock_env(borrow_token.address.clone(), &[]),
            handle_msg.clone(),
        );
        // ==== * it raises an error
        assert_eq!(
            handle_result.unwrap_err(),
            StdError::generic_err("Operation fell short of borrow_amount.")
        );
        // ==== when amount sent in is equal to the borrowed amount
        let hops: VecDeque<Hop> = VecDeque::new();
        let route_state: RouteState = RouteState {
            current_hop: Some(Hop {
                from_token: mock_butt(),
                trade_smart_contract: mock_contract(),
                position: Some(Uint128(2)),
            }),
            remaining_hops: hops.clone(),
            borrow_token: borrow_token.clone(),
            borrow_amount,
            initiator: mock_user_address(),
            minimum_acceptable_amount: None,
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();
        let handle_msg = HandleMsg::Receive {
            sender: mock_contract().address,
            from: mock_contract().address,
            amount: borrow_amount,
            msg: None,
        };
        let handle_result = handle(
            &mut deps,
            mock_env(borrow_token.address.clone(), &[]),
            handle_msg.clone(),
        );
        // ==== * it stores the current hop as None
        let route_state: RouteState = read_route_state(&deps.storage).unwrap().unwrap();
        // ==== * it stores the rest of the route state appropriately
        assert_eq!(
            route_state,
            RouteState {
                current_hop: None,
                borrow_token: borrow_token.clone(),
                remaining_hops: hops,
                borrow_amount,
                initiator: mock_user_address(),
                minimum_acceptable_amount: None,
            }
        );
        // ==== * it does not send any messages
        assert_eq!(handle_result.unwrap().messages, vec![]);
        // ==== when amount sent in is greater than the borrowed amount
        let hops: VecDeque<Hop> = VecDeque::new();
        let route_state: RouteState = RouteState {
            current_hop: Some(Hop {
                from_token: mock_butt(),
                trade_smart_contract: mock_contract(),
                position: Some(Uint128(2)),
            }),
            remaining_hops: hops.clone(),
            borrow_token: borrow_token.clone(),
            borrow_amount,
            initiator: mock_user_address(),
            minimum_acceptable_amount: Some(borrow_amount),
        };
        store_route_state(&mut deps.storage, &route_state).unwrap();
        let handle_msg = HandleMsg::Receive {
            sender: mock_contract().address,
            from: mock_contract().address,
            amount: borrow_amount + Uint128(1),
            msg: None,
        };
        let handle_result = handle(
            &mut deps,
            mock_env(borrow_token.address.clone(), &[]),
            handle_msg.clone(),
        );
        // ==== * it sends the excess after paying the borrowed amount to the to address
        assert_eq!(
            handle_result.unwrap().messages,
            vec![snip20::transfer_msg(
                route_state.initiator,
                Uint128(1),
                None,
                BLOCK_SIZE,
                borrow_token.contract_hash.clone(),
                borrow_token.address.clone(),
            )
            .unwrap()]
        );
    }

    #[test]
    fn test_orders_by_positions() {
        let (_init_result, mut deps) = init_helper(true);

        // when user's address and butt viewing key combo is correct
        // = when user does not have any orders yet
        // = * it raises an error
        let mut res = query(
            &deps,
            QueryMsg::OrdersByPositions {
                address: mock_user_address(),
                key: MOCK_VIEWING_KEY.to_string(),
                positions: vec![Uint128(0)],
            },
        );
        assert_eq!(
            res.unwrap_err(),
            NotFound {
                kind: "cw_secret_network_limit_orders::state::Order".to_string(),
                backtrace: None
            }
        );

        // = when user has orders
        create_order_helper(&mut deps);
        create_order_helper(&mut deps);
        create_order_helper(&mut deps);
        create_order_helper(&mut deps);
        create_order_helper(&mut deps);
        // == when position requested is unavailable
        res = query(
            &deps,
            QueryMsg::OrdersByPositions {
                address: mock_user_address(),
                key: MOCK_VIEWING_KEY.to_string(),
                positions: vec![Uint128(1), Uint128(2), Uint128(3), Uint128(5)],
            },
        );
        assert_eq!(
            res.unwrap_err(),
            NotFound {
                kind: "cw_secret_network_limit_orders::state::Order".to_string(),
                backtrace: None
            }
        );
        // == when position requested is available
        res = query(
            &deps,
            QueryMsg::OrdersByPositions {
                address: mock_user_address(),
                key: MOCK_VIEWING_KEY.to_string(),
                positions: vec![Uint128(1), Uint128(3), Uint128(4)],
            },
        );
        // == * it returns the humanized orders at those positions
        let query_answer: QueryAnswer = from_binary(&res.unwrap()).unwrap();
        match query_answer {
            QueryAnswer::Orders { orders, total } => {
                assert_eq!(total, None);
                assert_eq!(orders[0].creator, mock_user_address());
                assert_eq!(orders[0].position, Uint128(1));
                assert_eq!(orders[1].position, Uint128(3));
                assert_eq!(orders[2].position, Uint128(4));
            }
            _ => panic!("unexpected"),
        };
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
    fn test_update_config() {
        let (_init_result, mut deps) = init_helper(false);
        let new_addresses_allowed_to_fill = vec![mock_user_address()];
        let handle_msg = HandleMsg::UpdateConfig {
            addresses_allowed_to_fill: Some(new_addresses_allowed_to_fill.clone()),
            execution_fee: Some(Uint128(MOCK_AMOUNT)),
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
        let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
        assert_eq!(
            config.addresses_allowed_to_fill,
            vec![config.admin, env.contract.address.clone()]
        );
        assert_eq!(config.execution_fee, mock_execution_fee());
        handle(
            &mut deps,
            mock_env(HumanAddr::from(MOCK_ADMIN), &[]),
            handle_msg,
        )
        .unwrap();
        // = * it updates the addresses_allowed_to_fill and adds admin and contract address
        let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
        assert_eq!(
            config.addresses_allowed_to_fill,
            vec![mock_user_address(), env.contract.address, config.admin]
        );
        // = * it updates the execution_fee
        assert_eq!(config.execution_fee, Uint128(MOCK_AMOUNT))
    }
}
