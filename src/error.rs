use cosmwasm_std::{StdError, Uint128};
use cw0::PaymentError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Base(#[from] cw20_base::ContractError),

    #[error("{0}")]
    Payment(#[from] PaymentError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Validator '{validator}' not in current validator set")]
    NotInValidatorSet { validator: String },

    #[error("Different denominations in bonds: '{denom1}' vs. '{denom2}'")]
    DifferentBondDenom { denom1: String, denom2: String },

    #[error("Stored bonded {stored}, but query bonded {queried}")]
    BondedMismatch { stored: Uint128, queried: Uint128 },

    #[error("No {denom} tokens sent")]
    EmptyBalance { denom: String },

    #[error("Must unbond at least {min_bonded} {denom}")]
    UnbondTooSmall { min_bonded: Uint128, denom: String },

    #[error("Insufficient balance in contract to process claim")]
    BalanceTooSmall {},

    #[error("No claims that can be released currently")]
    NothingToClaim {},

    #[error("Cannot set to own account")]
    CannotSetOwnAccount {},

    #[error("Invalid zero amount")]
    InvalidZeroAmount {},

    #[error("Allowance is expired")]
    Expired {},

    #[error("No allowance for this account")]
    NoAllowance {},

    #[error("Minting cannot exceed the cap")]
    CannotExceedCap {},
}
