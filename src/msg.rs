use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Binary, Decimal, Uint128};
use cw0::Duration;
use cw20::Expiration;
pub use cw20_bonding::msg::CurveType;
pub use cw_controllers::ClaimsResponse;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct StakingParams {
    /// This is the validator that all tokens will be bonded to
    pub validator: String,
    /// This is the unbonding period of the native staking module
    /// We need this to only allow claims to be redeemed after the money has arrived
    pub unbonding_period: Duration,

    /// this is how much the owner takes as a cut when someone unbonds
    pub exit_tax: Decimal,
    /// This is the minimum amount we will pull out to reinvest, as well as a minimum
    /// that can be unbonded (to avoid needless staking tx)
    pub min_withdrawal: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    /// meta: external link
    /// this should be a bandcamp URI, spotify URI, apple, youtube etc
    /// it is somewhat up to the artist to decide how to manage this
    /// a suggestion would be they set something up themselves or use a link
    /// aggregator to collect all the relevant links for a release
    /// it seems undesirable in a contract to have multiple URIs
    pub external_permalink_uri: String,

    /// the name of the artist, entity or creator. Should be unique, but obv this is tricky IRL
    pub creator: String,

    /// the name of the work. one would hope artist + work would at least be unique
    pub work: String,

    /// a free text description of the work. this is mainly for UI and interaction purposes
    /// though for this reason it is also required for the unlikely event that a work needs differentiating
    pub description: String,

    /// (optional) an asset URI to store. Maybe this should be updateable in future?
    pub asset_uri: Option<String>,

    /// name of the supply token
    pub name: String,
    /// symbol / ticker of the supply token
    pub symbol: String,
    /// number of decimal places of the supply token, needed for proper curve math.
    /// If it is eg. BTC, where a balance of 10^8 means 1 BTC, then use 8 here.
    pub decimals: u8,

    /// this is the reserve token denom (only support native for now)
    // think this probably shouldn't even be settable thanks to staking constraints
    // which mean staking is native
    // pub reserve_denom: String,
    /// number of decimal places for the reserve token, needed for proper curve math.
    /// Same format as decimals above, eg. if it is uatom, where 1 unit is 10^-6 ATOM, use 6 here
    pub reserve_decimals: u8,

    /// enum to store the curve parameters used for this contract
    /// if you want to add a custom Curve, you should make a new contract that imports this one.
    /// write a custom `instantiate`, and then dispatch `your::execute` -> `cw20_bonding::do_execute`
    /// with your custom curve as a parameter (and same with `query` -> `do_query`)
    pub curve_type: CurveType,

    /// put all the staking params into a basket
    pub staking_params: StakingParams,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Buy will attempt to purchase as many supply tokens as possible.
    /// You must send only reserve tokens in that message
    Buy {},

    /// Implements CW20. Transfer is a base message to move tokens to another account without triggering actions
    Transfer { recipient: String, amount: Uint128 },
    /// Implements CW20. Burn is a base message to destroy tokens forever
    Burn { amount: Uint128 },
    /// Implements CW20.  Send is a base message to transfer tokens to a contract and trigger an action
    /// on the receiving contract.
    Send {
        contract: String,
        amount: Uint128,
        msg: Binary,
    },
    /// Implements CW20 "approval" extension. Allows spender to access an additional amount tokens
    /// from the owner's (env.sender) account. If expires is Some(), overwrites current allowance
    /// expiration with this one.
    IncreaseAllowance {
        spender: String,
        amount: Uint128,
        expires: Option<Expiration>,
    },
    /// Implements CW20 "approval" extension. Lowers the spender's access of tokens
    /// from the owner's (env.sender) account by amount. If expires is Some(), overwrites current
    /// allowance expiration with this one.
    DecreaseAllowance {
        spender: String,
        amount: Uint128,
        expires: Option<Expiration>,
    },
    /// Implements CW20 "approval" extension. Transfers amount tokens from owner -> recipient
    /// if `env.sender` has sufficient pre-approval.
    TransferFrom {
        owner: String,
        recipient: String,
        amount: Uint128,
    },
    /// Implements CW20 "approval" extension. Sends amount tokens from owner -> contract
    /// if `env.sender` has sufficient pre-approval.
    SendFrom {
        owner: String,
        contract: String,
        amount: Uint128,
        msg: Binary,
    },
    /// Implements CW20 "approval" extension. Destroys tokens forever
    BurnFrom { owner: String, amount: Uint128 },
    /// Here be staking dragons
    /// Bond will bond all staking tokens sent with the message and release derivative tokens
    Bond {},
    /// Unbond will "burn" the given amount of derivative tokens and send the unbonded
    /// staking tokens to the message sender (after exit tax is deducted)
    Unbond { amount: Uint128 },
    /// Claim is used to claim your native tokens that you previously "unbonded"
    /// after the chain-defined waiting period (eg. 3 weeks)
    Claim {},
    /// Reinvest will check for all accumulated rewards, withdraw them, and
    /// re-bond them to the same validator. Anyone can call this, which updates
    /// the value of the token (how much under custody).
    Reinvest {},
    /// _BondAllTokens can only be called by the contract itself, after all rewards have been
    /// withdrawn. This is an example of using "callbacks" in message flows.
    /// This can only be invoked by the contract itself as a return from Reinvest
    _BondAllTokens {},
    // /// TODO
    // /// essentially a DR feature.
    // /// imagine a validator you've configured falls out of the validator set
    // /// this will update the configured validator and rebond all the tokens
    // RebondAllTokens { validator_address: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Claims shows the number of tokens this address can access when they are done unbonding
    Claims { address: String },
    /// Investment shows metadata on the staking info of the contract
    Investment {},
    /// Returns the reserve and supply quantities, as well as the spot price to buy 1 token
    CurveInfo {},

    /// Implements CW20. Returns the current balance of the given address, 0 if unset.
    Balance { address: String },
    /// Implements CW20. Returns metadata on the contract - name, decimals, supply, etc.
    TokenInfo {},
    /// Implements CW20 "allowance" extension.
    /// Returns how much spender can use from owner account, 0 if unset.
    Allowance { owner: String, spender: String },
}
