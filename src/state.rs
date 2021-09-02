use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::Item;

use crate::msg::CurveType;
use cw20_bonding::curves::DecimalPlaces;

use cw20_base::state::TokenInfo;

use cw0::Duration;
use cw_controllers::Claims;

type ValidatorAddress = String;

/// Supply is dynamic and tracks the current supply of staked and cw20 tokens.
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

    /// claims is how many tokens need to be reserved for paying back those who unbonded
    pub claims: Uint128,
}

impl CurveState {
    pub fn new(reserve_denom: String, decimals: DecimalPlaces) -> Self {
        CurveState {
            reserve: Uint128::new(0),
            supply: Uint128::new(0),
            reserve_denom,
            decimals,
            claims: Uint128::new(0),
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

/// Investment info is fixed at instantiation, and is used to control the function of the contract
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InvestmentInfo {
    /// Owner created the contract and takes a cut
    pub owner: Addr,
    /// This is the denomination we can stake (and only one we accept for payments)
    pub bond_denom: String,
    /// This is the unbonding period of the native staking module
    /// We need this to only allow claims to be redeemed after the money has arrived
    pub unbonding_period: Duration,
    /// This is how much the owner takes as a cut when someone unbonds
    pub exit_tax: Decimal,
    /// All tokens are bonded to this validator
    /// FIXME: address validation doesn't work for validator addresses
    pub validator: ValidatorAddress,
    /// This is the minimum amount we will pull out to reinvest, as well as a minimum
    /// that can be unbonded (to avoid needless staking tx)
    pub min_withdrawal: Uint128,
}

pub const CLAIMS: Claims = Claims::new("claims");

pub const INVESTMENT: Item<InvestmentInfo> = Item::new("invest");

pub const CURVE_STATE: Item<CurveState> = Item::new("curve_state");

pub const CURVE_TYPE: Item<CurveType> = Item::new("curve_type");

pub const TOKEN_INFO_WITH_META: Item<TokenInfoWithMeta> = Item::new("token_info_with_meta");
