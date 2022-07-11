use crate::constants::PREFIX_ORDERS;
use crate::state::SecretContract;
use cosmwasm_std::{Api, CanonicalAddr, HumanAddr, ReadonlyStorage, StdResult, Storage, Uint128};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage};
use schemars::JsonSchema;
use secret_toolkit::storage::{AppendStore, AppendStoreMut};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct HumanizedOrder {
    pub creator: HumanAddr,
    pub position: u32,
    pub from_token: SecretContract,
    pub to_token: SecretContract,
    pub amount: Uint128,
    pub to_amount: Uint128,
    pub status: u8,
    pub block_time: u64,
    pub block_height: u64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Order {
    pub creator: CanonicalAddr,
    pub position: u32,
    pub other_storage_position: u32,
    pub from_token: SecretContract,
    pub to_token: SecretContract,
    pub amount: Uint128,
    pub to_amount: Uint128,
    pub status: u8,
    pub block_time: u64,
    pub block_height: u64,
}
impl Order {
    fn into_humanized<A: Api>(self, api: &A) -> StdResult<HumanizedOrder> {
        Ok(HumanizedOrder {
            creator: api.human_address(&self.creator)?,
            position: self.position,
            from_token: self.from_token,
            to_token: self.to_token,
            amount: self.amount,
            to_amount: self.to_amount,
            status: self.status,
            block_time: self.block_time,
            block_height: self.block_height,
        })
    }
}

// Storage functions:
pub fn get_orders<A: Api, S: ReadonlyStorage>(
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

pub fn store_orders<S: Storage>(
    store: &mut S,
    from_token: SecretContract,
    to_token: SecretContract,
    creator: CanonicalAddr,
    amount: Uint128,
    to_amount: Uint128,
    status: u8,
    block: &cosmwasm_std::BlockInfo,
    contract_address: CanonicalAddr,
) -> StdResult<()> {
    let creator_position = get_next_position(store, &creator)?;
    let contract_address_position = get_next_position(store, &contract_address)?;
    let from_order = Order {
        position: creator_position,
        other_storage_position: contract_address_position,
        from_token: from_token,
        to_token: to_token,
        creator: creator.clone(),
        amount: amount,
        to_amount: to_amount,
        status: status,
        block_time: block.time,
        block_height: block.height,
    };
    append_order(store, &from_order, &creator)?;
    let mut to_order = from_order;
    to_order.position = contract_address_position;
    to_order.other_storage_position = creator_position;
    append_order(store, &to_order, &contract_address)?;

    Ok(())
}

pub fn order_at_position<S: Storage>(
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

pub fn update_order<S: Storage>(
    store: &mut S,
    address: &CanonicalAddr,
    order: Order,
) -> StdResult<()> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_ORDERS, address.as_slice()], store);
    // Try to access the storage of orders for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let mut store = AppendStoreMut::<Order, _, _>::attach_or_create(&mut store)?;
    store.set_at(order.position, &order)?;

    Ok(())
}

// // Verify the Order and then verify it's counter Order
// pub fn verify_orders<A: Api, S: Storage>(
//     api: &A,
//     store: &mut S,
//     address: &CanonicalAddr,
//     amount: Uint128,
//     position: u32,
//     status: u8,
//     token_address: HumanAddr,
// ) -> StdResult<(Order, Order)> {
//     let from_order = order_at_position(store, address, position)?;
//     let to_order = order_at_position(store, &from_order.to_token, from_order.other_storage_position)?;
//     correct_amount_of_token(
//         amount,
//         to_order.amount,
//         token_address,
//         to_order.token.address.clone(),
//     )?;
//     authorize(
//         api.human_address(&to_order.from)?,
//         api.human_address(address)?,
//     )?;
//     if to_order.status != status {
//         return Err(StdError::generic_err(
//             "Order status at that position is incorrect.",
//         ));
//     }

//     Ok((from_order, to_order))
// }

// pub fn verify_orders_for_cancel<S: Storage>(
//     store: &mut S,
//     address: &CanonicalAddr,
//     position: u32,
// ) -> StdResult<(Order, Order)> {
//     let from_order = order_at_position(store, address, position)?;
//     let to_order = order_at_position(store, &from_order.to_token, from_order.other_storage_position)?;
//     if to_order.status == 2 {
//         return Err(StdError::generic_err("Order already cancelled."));
//     }
//     if to_order.status == 3 {
//         return Err(StdError::generic_err("Order already finalized."));
//     }

//     Ok((from_order, to_order))
// }

// pub fn verify_orders_for_confirm_address<A: Api, S: Storage>(
//     api: &A,
//     store: &mut S,
//     address: &CanonicalAddr,
//     position: u32,
// ) -> StdResult<(Order, Order)> {
//     let creator_order = order_at_position(store, address, position)?;
//     let contract_order = order_at_position(store, &to_order.from, to_order.other_storage_position)?;
//     authorize(
//         api.human_address(&to_order.to_token)?,
//         api.human_address(address)?,
//     )?;
//     if to_order.status != 0 {
//         return Err(StdError::generic_err(
//             "Order not waiting for address confirmation.",
//         ));
//     }

//     Ok((from_order, to_order))
// }

fn append_order<S: Storage>(
    store: &mut S,
    order: &Order,
    for_address: &CanonicalAddr,
) -> StdResult<()> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_ORDERS, for_address.as_slice()], store);
    let mut store = AppendStoreMut::attach_or_create(&mut store)?;
    store.push(order)
}

fn get_next_position<S: Storage>(store: &mut S, for_address: &CanonicalAddr) -> StdResult<u32> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_ORDERS, for_address.as_slice()], store);
    let store = AppendStoreMut::<Order, _>::attach_or_create(&mut store)?;
    Ok(store.len())
}
