use crate::state::{ActivityRecord, HumanizedOrder, SecretContract};
use cosmwasm_std::{Binary, HumanAddr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub butt: SecretContract,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    Receive {
        sender: HumanAddr,
        from: HumanAddr,
        amount: Uint128,
        msg: Binary,
    },
    RegisterTokens {
        tokens: Vec<SecretContract>,
        viewing_key: String,
    },
    RescueTokens {
        denom: Option<String>,
        token_address: Option<HumanAddr>,
    },
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswer {
    ActivityRecords {
        activity_records: Vec<ActivityRecord>,
        total: Option<u64>,
    },
    Orders {
        orders: Vec<HumanizedOrder>,
        total: Option<u64>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    ActivityRecords {
        key: String,
        page: u32,
        page_size: u32,
    },
    Config {},
    Orders {
        address: HumanAddr,
        key: String,
        page: u32,
        page_size: u32,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveMsg {
    CancelOrder {
        position: u32,
    },
    CreateOrder {
        butt_viewing_key: String,
        to_amount: Uint128,
        to_token: HumanAddr,
    },
    FillOrder {
        position: u32,
    },
}
