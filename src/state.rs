use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::Uint128;
use cw_storage_plus::Item;

use cw20_bonding::curves::DecimalPlaces;
use cw20_bonding::msg::CurveType;

use cw20_base::state::TokenInfo;

/// Supply is dynamic and tracks the current supply of staked and ERC20 tokens.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct CurveState {
    /// reserve is how many native tokens exist bonded to the validator
    pub reserve: Uint128,
    /// supply is how many tokens this contract has issued
    pub supply: Uint128,

    // the denom of the reserve token
    pub reserve_denom: String,

    // how to normalize reserve and supply
    pub decimals: DecimalPlaces,
}

impl CurveState {
    pub fn new(reserve_denom: String, decimals: DecimalPlaces) -> Self {
        CurveState {
            reserve: Uint128::new(0),
            supply: Uint128::new(0),
            reserve_denom,
            decimals,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TokenInfoWithMeta {
    pub external_permalink_uri: String,
    pub creator: String,
    pub work: String,
    pub description: String,
    pub asset_uri: Option<String>,
    pub token_info: TokenInfo,
}

impl TokenInfoWithMeta {
    pub fn get_cap(&self) -> Option<Uint128> {
        self.token_info.mint.as_ref().and_then(|v| v.cap)
    }
}

pub const CURVE_STATE: Item<CurveState> = Item::new("curve_state");

pub const CURVE_TYPE: Item<CurveType> = Item::new("curve_type");

pub const TOKEN_INFO_WITH_META: Item<TokenInfoWithMeta> = Item::new("token_info_with_meta");
