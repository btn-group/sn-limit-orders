use crate::constants::PREFIX_REGISTERED_TOKENS;
use cosmwasm_std::{CanonicalAddr, HumanAddr, StdResult, Storage, Uint128};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage};
use schemars::JsonSchema;
use secret_toolkit::storage::{TypedStore, TypedStoreMut};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub admin: HumanAddr,
    pub butt: SecretContract,
}

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

// === RegisteredTokens Storage ===
pub fn read_registered_token<S: Storage>(
    store: &S,
    token_address: &CanonicalAddr,
) -> Option<RegisteredToken> {
    let registered_tokens_store = ReadonlyPrefixedStorage::new(PREFIX_REGISTERED_TOKENS, store);
    let registered_tokens_store = TypedStore::attach(&registered_tokens_store);
    registered_tokens_store
        .may_load(token_address.as_slice())
        .unwrap()
}

pub fn write_registered_token<S: Storage>(
    store: &mut S,
    token_address: &CanonicalAddr,
    registered_token: RegisteredToken,
) -> StdResult<()> {
    let mut registered_tokens_store = PrefixedStorage::new(PREFIX_REGISTERED_TOKENS, store);
    let mut registered_tokens_store = TypedStoreMut::attach(&mut registered_tokens_store);
    registered_tokens_store.store(token_address.as_slice(), &registered_token)
}
