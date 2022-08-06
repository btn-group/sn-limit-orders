use crate::constants::{PREFIX_REGISTERED_TOKENS, ROUTE_STATE_KEY};
use cosmwasm_std::{Api, CanonicalAddr, HumanAddr, StdResult, Storage, Uint128};
use cosmwasm_storage::{singleton, singleton_read, PrefixedStorage, ReadonlyPrefixedStorage};
use schemars::JsonSchema;
use secret_toolkit::storage::{TypedStore, TypedStoreMut};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// For tracking cancelled and filled
// activity (0 => cancelled, 1 => filled)
#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, JsonSchema)]
pub struct ActivityRecord {
    pub order_position: u32,
    pub position: u32,
    pub activity: u8,
    pub result_from_amount_filled: Option<Uint128>,
    pub result_net_to_amount_filled: Option<Uint128>,
    pub updated_at_block_height: u64,
    pub updated_at_block_time: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub admin: HumanAddr,
    pub addresses_allowed_to_fill: Vec<HumanAddr>,
    pub butt: SecretContract,
    pub execution_fee: Uint128,
    pub sscrt: SecretContract,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, JsonSchema)]
pub struct SecretContract {
    pub address: HumanAddr,
    pub contract_hash: String,
}

// === Registered tokens ===
#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, JsonSchema)]
pub struct RegisteredToken {
    pub address: HumanAddr,
    pub contract_hash: String,
    pub sum_balance: Uint128,
}

pub fn read_registered_token<S: Storage>(
    storage: &S,
    token_address: &CanonicalAddr,
) -> Option<RegisteredToken> {
    let registered_tokens_storage = ReadonlyPrefixedStorage::new(PREFIX_REGISTERED_TOKENS, storage);
    let registered_tokens_storage = TypedStore::attach(&registered_tokens_storage);
    registered_tokens_storage
        .may_load(token_address.as_slice())
        .unwrap()
}

pub fn write_registered_token<S: Storage>(
    storage: &mut S,
    token_address: &CanonicalAddr,
    registered_token: RegisteredToken,
) -> StdResult<()> {
    let mut registered_tokens_storage = PrefixedStorage::new(PREFIX_REGISTERED_TOKENS, storage);
    let mut registered_tokens_storage = TypedStoreMut::attach(&mut registered_tokens_storage);
    registered_tokens_storage.store(token_address.as_slice(), &registered_token)
}

// === Orders ===
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct HumanizedOrder {
    pub creator: HumanAddr,
    pub execution_fee: Option<Uint128>,
    pub position: u32,
    pub from_token: HumanAddr,
    pub to_token: HumanAddr,
    pub from_amount: Uint128,
    pub from_amount_filled: Uint128,
    pub net_to_amount: Uint128,
    pub net_to_amount_filled: Uint128,
    pub cancelled: bool,
    pub fee: Uint128,
    pub created_at_block_time: u64,
    pub created_at_block_height: u64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Order {
    pub creator: CanonicalAddr,
    pub execution_fee: Option<Uint128>,
    pub position: u32,
    pub other_storage_position: u32,
    pub from_token: HumanAddr,
    pub to_token: HumanAddr,
    pub from_amount: Uint128,
    pub from_amount_filled: Uint128,
    pub net_to_amount: Uint128,
    pub net_to_amount_filled: Uint128,
    pub cancelled: bool,
    pub fee: Uint128,
    pub created_at_block_time: u64,
    pub created_at_block_height: u64,
}
impl Order {
    pub fn into_humanized<A: Api>(self, api: &A) -> StdResult<HumanizedOrder> {
        Ok(HumanizedOrder {
            creator: api.human_address(&self.creator)?,
            execution_fee: self.execution_fee,
            position: self.position,
            from_token: self.from_token,
            to_token: self.to_token,
            from_amount: self.from_amount,
            from_amount_filled: self.from_amount_filled,
            net_to_amount: self.net_to_amount,
            net_to_amount_filled: self.net_to_amount_filled,
            cancelled: self.cancelled,
            fee: self.fee,
            created_at_block_time: self.created_at_block_time,
            created_at_block_height: self.created_at_block_height,
        })
    }
}

// === ROUTE ===
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Hop {
    pub from_token: SecretContract,
    pub trade_smart_contract: SecretContract,
    pub position: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RouteState {
    pub current_hop: Option<Hop>,
    pub remaining_hops: VecDeque<Hop>,
    pub borrow_amount: Uint128,
    pub borrow_token: SecretContract,
    pub minimum_acceptable_amount: Option<Uint128>,
    pub initiator: HumanAddr,
}

pub fn store_route_state<S: Storage>(storage: &mut S, data: &RouteState) -> StdResult<()> {
    singleton(storage, ROUTE_STATE_KEY).save(data)
}

pub fn read_route_state<S: Storage>(storage: &S) -> StdResult<Option<RouteState>> {
    singleton_read(storage, ROUTE_STATE_KEY).may_load()
}

pub fn delete_route_state<S: Storage>(storage: &mut S) {
    singleton::<S, Option<RouteState>>(storage, ROUTE_STATE_KEY).remove();
}
