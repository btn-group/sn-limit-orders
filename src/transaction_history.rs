use crate::authorize::authorize;
use crate::constants::PREFIX_TXS;
use crate::contract::correct_amount_of_token;
use crate::state::SecretContract;
use cosmwasm_std::{
    Api, CanonicalAddr, HumanAddr, ReadonlyStorage, StdError, StdResult, Storage, Uint128,
};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage};
use schemars::JsonSchema;
use secret_toolkit::storage::{AppendStore, AppendStoreMut};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct HumanizedTx {
    pub position: u32,
    pub from: HumanAddr,
    pub to: HumanAddr,
    pub amount: Uint128,
    pub token: SecretContract,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: u8,
    pub block_time: u64,
    pub block_height: u64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Tx {
    pub position: u32,
    pub other_storage_position: u32,
    pub fee: Uint128,
    pub from: CanonicalAddr,
    pub to: CanonicalAddr,
    pub creator: HumanAddr,
    pub amount: Uint128,
    pub token: SecretContract,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: u8,
    pub block_time: u64,
    pub block_height: u64,
}
impl Tx {
    fn into_humanized<A: Api>(self, api: &A) -> StdResult<HumanizedTx> {
        Ok(HumanizedTx {
            position: self.position,
            from: api.human_address(&self.from)?,
            to: api.human_address(&self.to)?,
            amount: self.amount,
            token: self.token,
            description: self.description,
            status: self.status,
            block_time: self.block_time,
            block_height: self.block_height,
        })
    }
}

// Storage functions:
pub fn get_txs<A: Api, S: ReadonlyStorage>(
    api: &A,
    storage: &S,
    for_address: &CanonicalAddr,
    page: u32,
    page_size: u32,
) -> StdResult<(Vec<HumanizedTx>, u64)> {
    let store = ReadonlyPrefixedStorage::multilevel(&[PREFIX_TXS, for_address.as_slice()], storage);

    // Try to access the storage of txs for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let store = AppendStore::<Tx, _, _>::attach(&store);
    let store = if let Some(result) = store {
        result?
    } else {
        return Ok((vec![], 0));
    };

    // Take `page_size` txs starting from the latest tx, potentially skipping `page * page_size`
    // txs from the start.
    let tx_iter = store
        .iter()
        .rev()
        .skip((page * page_size) as _)
        .take(page_size as _);

    // The `and_then` here flattens the `StdResult<StdResult<RichTx>>` to an `StdResult<RichTx>`
    let txs: StdResult<Vec<HumanizedTx>> = tx_iter
        .map(|tx| tx.map(|tx| tx.into_humanized(api)).and_then(|x| x))
        .collect();
    txs.map(|txs| (txs, store.len() as u64))
}

pub fn store_txs<S: Storage>(
    store: &mut S,
    fee: Uint128,
    from: &CanonicalAddr,
    to: &CanonicalAddr,
    creator: HumanAddr,
    amount: Uint128,
    token: SecretContract,
    description: Option<String>,
    status: u8,
    block: &cosmwasm_std::BlockInfo,
) -> StdResult<()> {
    if from == to {
        return Err(StdError::generic_err(
            "From and to addresses must be different.",
        ));
    }

    let from_position = get_next_position(store, from)?;
    let to_position = get_next_position(store, to)?;
    let from_tx = Tx {
        position: from_position,
        other_storage_position: to_position,
        fee: fee,
        from: from.clone(),
        to: to.clone(),
        creator: creator,
        amount: amount,
        token: token,
        description: description,
        status: status,
        block_time: block.time,
        block_height: block.height,
    };
    append_tx(store, &from_tx, from)?;
    let mut to_tx = from_tx;
    to_tx.position = to_position;
    to_tx.other_storage_position = from_position;
    append_tx(store, &to_tx, to)?;

    Ok(())
}

pub fn tx_at_position<S: Storage>(
    store: &mut S,
    address: &CanonicalAddr,
    position: u32,
) -> StdResult<Tx> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_TXS, address.as_slice()], store);
    // Try to access the storage of txs for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let store = AppendStoreMut::<Tx, _, _>::attach_or_create(&mut store)?;

    Ok(store.get_at(position)?)
}

pub fn update_tx<S: Storage>(store: &mut S, address: &CanonicalAddr, tx: Tx) -> StdResult<()> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_TXS, address.as_slice()], store);
    // Try to access the storage of txs for the account.
    // If it doesn't exist yet, return an empty list of transfers.
    let mut store = AppendStoreMut::<Tx, _, _>::attach_or_create(&mut store)?;
    store.set_at(tx.position, &tx)?;

    Ok(())
}

// Verify the Tx and then verify it's counter Tx
pub fn verify_txs<A: Api, S: Storage>(
    api: &A,
    store: &mut S,
    address: &CanonicalAddr,
    amount: Uint128,
    position: u32,
    status: u8,
    token_address: HumanAddr,
) -> StdResult<(Tx, Tx)> {
    let from_tx = tx_at_position(store, address, position)?;
    let to_tx = tx_at_position(store, &from_tx.to, from_tx.other_storage_position)?;
    correct_amount_of_token(
        amount,
        to_tx.amount,
        token_address,
        to_tx.token.address.clone(),
    )?;
    authorize(api.human_address(&to_tx.from)?, api.human_address(address)?)?;
    if to_tx.status != status {
        return Err(StdError::generic_err(
            "Tx status at that position is incorrect.",
        ));
    }

    Ok((from_tx, to_tx))
}

pub fn verify_txs_for_cancel<S: Storage>(
    store: &mut S,
    address: &CanonicalAddr,
    position: u32,
) -> StdResult<(Tx, Tx)> {
    let from_tx = tx_at_position(store, address, position)?;
    let to_tx = tx_at_position(store, &from_tx.to, from_tx.other_storage_position)?;
    if to_tx.status == 2 {
        return Err(StdError::generic_err("Tx already cancelled."));
    }
    if to_tx.status == 3 {
        return Err(StdError::generic_err("Tx already finalized."));
    }

    Ok((from_tx, to_tx))
}

pub fn verify_txs_for_confirm_address<A: Api, S: Storage>(
    api: &A,
    store: &mut S,
    address: &CanonicalAddr,
    position: u32,
) -> StdResult<(Tx, Tx)> {
    let to_tx = tx_at_position(store, address, position)?;
    let from_tx = tx_at_position(store, &to_tx.from, to_tx.other_storage_position)?;
    authorize(api.human_address(&to_tx.to)?, api.human_address(address)?)?;
    if to_tx.status != 0 {
        return Err(StdError::generic_err(
            "Tx not waiting for address confirmation.",
        ));
    }

    Ok((from_tx, to_tx))
}

fn append_tx<S: Storage>(store: &mut S, tx: &Tx, for_address: &CanonicalAddr) -> StdResult<()> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_TXS, for_address.as_slice()], store);
    let mut store = AppendStoreMut::attach_or_create(&mut store)?;
    store.push(tx)
}

fn get_next_position<S: Storage>(store: &mut S, for_address: &CanonicalAddr) -> StdResult<u32> {
    let mut store = PrefixedStorage::multilevel(&[PREFIX_TXS, for_address.as_slice()], store);
    let store = AppendStoreMut::<Tx, _>::attach_or_create(&mut store)?;
    Ok(store.len())
}
