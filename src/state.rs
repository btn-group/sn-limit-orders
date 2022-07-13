use crate::constants::PREFIX_REGISTERED_TOKENS;
use cosmwasm_std::{Api, CanonicalAddr, HumanAddr, StdResult, Storage, Uint128};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage};
use schemars::JsonSchema;
use secret_toolkit::storage::{TypedStore, TypedStoreMut};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub admin: HumanAddr,
    pub butt: SecretContract,
}

// === Registered tokens ===
#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, JsonSchema)]
pub struct SecretContract {
    pub address: HumanAddr,
    pub contract_hash: String,
}

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
    pub position: u32,
    pub from_token: HumanAddr,
    pub to_token: HumanAddr,
    pub from_amount: Uint128,
    pub from_amount_filled: Uint128,
    pub net_to_amount: Uint128,
    pub net_to_amount_filled: Uint128,
    pub cancelled: bool,
    pub fee: Uint128,
    pub block_time: u64,
    pub block_height: u64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Order {
    pub creator: CanonicalAddr,
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
    pub block_time: u64,
    pub block_height: u64,
}
impl Order {
    pub fn into_humanized<A: Api>(self, api: &A) -> StdResult<HumanizedOrder> {
        Ok(HumanizedOrder {
            creator: api.human_address(&self.creator)?,
            position: self.position,
            from_token: self.from_token,
            to_token: self.to_token,
            from_amount: self.from_amount,
            from_amount_filled: self.from_amount_filled,
            net_to_amount: self.net_to_amount,
            net_to_amount_filled: self.net_to_amount_filled,
            cancelled: self.cancelled,
            fee: self.fee,
            block_time: self.block_time,
            block_height: self.block_height,
        })
    }
}
