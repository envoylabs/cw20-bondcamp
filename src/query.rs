use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Coin, Decimal, Uint128};

use cw20::TokenInfoResponse;
pub use cw_controllers::ClaimsResponse;

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
pub struct TokenInfoResponseWithMeta {
    pub external_permalink_uri: String,
    pub creator: String,
    pub work: String,
    pub description: String,
    pub asset_uri: Option<String>,
    pub token_info_response: TokenInfoResponse,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InvestmentResponse {
    pub token_supply: Uint128,
    pub staked_tokens: Coin,
    // ratio of staked_tokens / token_supply (or how many native tokens that one derivative token is nominally worth)
    pub nominal_value: Decimal,

    /// owner created the contract and takes a cut
    pub owner: String,
    /// this is how much the owner takes as a cut when someone unbonds
    pub exit_tax: Decimal,
    /// All tokens are bonded to this validator
    pub validator: String,
    /// This is the minimum amount we will pull out to reinvest, as well as a minimum
    /// that can be unbonded (to avoid needless staking tx)
    pub min_withdrawal: Uint128,
}

// might need to provide this in order to return claims info
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CurveInfoResponse {
    // how many reserve tokens have been received
    pub reserve: Uint128,
    // how many supply tokens have been issued
    pub supply: Uint128,
    pub spot_price: Decimal,
    pub reserve_denom: String,
    pub claims: Uint128,
}
