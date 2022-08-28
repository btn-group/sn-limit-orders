use crate::state::{ActivityRecord, Hop, HumanizedOrder, SecretContract};
use cosmwasm_std::{Binary, HumanAddr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub butt: SecretContract,
    pub execution_fee: Uint128,
    pub sscrt: SecretContract,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    CancelOrder {
        from_token_address: HumanAddr,
        position: Uint128,
    },
    FinalizeRoute {},
    HandleFirstHop {
        borrow_amount: Uint128,
        hops: VecDeque<Hop>,
        minimum_acceptable_amount: Option<Uint128>,
    },
    Receive {
        sender: HumanAddr,
        from: HumanAddr,
        amount: Uint128,
        msg: Option<Binary>,
    },
    RegisterTokens {
        tokens: Vec<SecretContract>,
        viewing_key: String,
    },
    RescueTokens {
        denom: Option<String>,
        key: Option<String>,
        token_address: Option<HumanAddr>,
    },
    UpdateConfig {
        addresses_allowed_to_fill: Option<Vec<HumanAddr>>,
        execution_fee: Option<Uint128>,
    },
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswer {
    ActivityRecords {
        activity_records: Vec<ActivityRecord>,
        total: Option<Uint128>,
    },
    Orders {
        orders: Vec<HumanizedOrder>,
        total: Option<Uint128>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    CancelRecords {
        key: String,
        page: Uint128,
        page_size: Uint128,
    },
    FillRecords {
        key: String,
        page: Uint128,
        page_size: Uint128,
    },
    Config {},
    Orders {
        address: HumanAddr,
        key: String,
        page: Uint128,
        page_size: Uint128,
    },
    OrdersByPositions {
        address: HumanAddr,
        key: String,
        positions: Vec<Uint128>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveMsg {
    SetExecutionFeeForOrder {},
    CreateOrder {
        butt_viewing_key: String,
        to_amount: Uint128,
        to_token: HumanAddr,
    },
    FillOrder {
        position: Uint128,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Snip20Swap {
    Swap {
        expected_return: Option<Uint128>,
        to: Option<HumanAddr>,
    },
}
