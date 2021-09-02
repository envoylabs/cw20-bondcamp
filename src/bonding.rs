use cosmwasm_std::{
    attr, coins, Addr, BankMsg, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128,
};

use cw20_base::allowances::deduct_allowance;

use cw20_base::state::BALANCES;

use crate::error::ContractError;

use crate::state::{CURVE_STATE, TOKEN_INFO_WITH_META};
use cw0::{must_pay, nonpayable};

use cw20_bonding::msg::CurveFn;

// the-frey: this is again a slight change to the one defined in cw20-base
// as we have different types and so stuff goes askew
pub fn execute_burn(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response, ContractError> {
    if amount == Uint128::zero() {
        return Err(ContractError::Base(
            cw20_base::ContractError::InvalidZeroAmount {},
        ));
    }

    // lower balance
    BALANCES.update(
        deps.storage,
        &info.sender,
        |balance: Option<Uint128>| -> StdResult<_> {
            Ok(balance.unwrap_or_default().checked_sub(amount)?)
        },
    )?;
    // reduce total_supply
    TOKEN_INFO_WITH_META.update(deps.storage, |mut info| -> StdResult<_> {
        info.token_info.total_supply = info.token_info.total_supply.checked_sub(amount)?;
        Ok(info)
    })?;

    let res = Response::new()
        .add_attribute("action", "burn")
        .add_attribute("from", info.sender)
        .add_attribute("amount", amount);
    Ok(res)
}

// the-frey: this is again a slight change to the one defined in cw20-base
// as we have different types and so stuff goes askew
pub fn execute_mint(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    recipient: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    if amount == Uint128::zero() {
        return Err(ContractError::Base(
            cw20_base::ContractError::InvalidZeroAmount {},
        ));
    }

    let mut config = TOKEN_INFO_WITH_META.load(deps.storage)?;
    if config.token_info.mint.is_none()
        || config.token_info.mint.as_ref().unwrap().minter != info.sender
    {
        return Err(ContractError::Unauthorized {});
    }

    // update supply and enforce cap
    config.token_info.total_supply += amount;
    if let Some(limit) = config.token_info.get_cap() {
        if config.token_info.total_supply > limit {
            return Err(ContractError::Base(
                cw20_base::ContractError::CannotExceedCap {},
            ));
        }
    }
    TOKEN_INFO_WITH_META.save(deps.storage, &config)?;

    // add amount to recipient balance
    let rcpt_addr = deps.api.addr_validate(&recipient)?;
    BALANCES.update(
        deps.storage,
        &rcpt_addr,
        |balance: Option<Uint128>| -> StdResult<_> { Ok(balance.unwrap_or_default() + amount) },
    )?;

    let res = Response::new()
        .add_attribute("action", "mint")
        .add_attribute("to", recipient)
        .add_attribute("amount", amount);
    Ok(res)
}

// the-frey:
// this is verbatim from cw20-bonding, we should probably refactor out
pub fn execute_buy(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    curve_fn: CurveFn,
) -> Result<Response, ContractError> {
    let mut state = CURVE_STATE.load(deps.storage)?;

    let payment = must_pay(&info, &state.reserve_denom)?;

    // calculate how many tokens can be purchased with this and mint them
    let curve = curve_fn(state.decimals);
    state.reserve += payment;

    // curve.supply() calculates native -> CW20
    let new_supply = curve.supply(state.reserve);
    let minted = new_supply
        .checked_sub(state.supply)
        .map_err(StdError::overflow)?;
    state.supply = new_supply;
    CURVE_STATE.save(deps.storage, &state)?;

    // call into cw20-base to mint the token, call as self as no one else is allowed
    let sub_info = MessageInfo {
        sender: env.contract.address.clone(),
        funds: vec![],
    };
    execute_mint(deps, env, sub_info, info.sender.to_string(), minted)?;

    // bond them to the validator
    let res = Response::new()
        .add_attribute("action", "buy")
        .add_attribute("from", info.sender)
        .add_attribute("reserve", payment)
        .add_attribute("supply", minted);
    Ok(res)
}

// the-frey:
// this is verbatim from cw20-bonding, we should probably refactor out
pub fn execute_sell(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    curve_fn: CurveFn,
    amount: Uint128,
) -> Result<Response, ContractError> {
    nonpayable(&info)?;
    let receiver = info.sender.clone();
    // do all the work
    let mut res = do_sell(deps, env, info, curve_fn, receiver, amount)?;

    // add our custom attributes
    res.attributes.push(attr("action", "burn"));
    Ok(res)
}

// the-frey: even though this is the default impl
// not convinced it does exactly what we want here. TBC
pub fn execute_sell_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    curve_fn: CurveFn,
    owner: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    nonpayable(&info)?;
    let owner_addr = deps.api.addr_validate(&owner)?;
    let spender_addr = info.sender.clone();

    // deduct allowance before doing anything else have enough allowance
    deduct_allowance(deps.storage, &owner_addr, &spender_addr, &env.block, amount)?;

    // do all the work in do_sell
    let receiver_addr = info.sender;
    let owner_info = MessageInfo {
        sender: owner_addr,
        funds: info.funds,
    };
    let mut res = do_sell(
        deps,
        env,
        owner_info,
        curve_fn,
        receiver_addr.clone(),
        amount,
    )?;

    // add our custom attributes
    res.attributes.push(attr("action", "burn_from"));
    res.attributes.push(attr("by", receiver_addr));
    Ok(res)
}

fn do_sell(
    mut deps: DepsMut,
    env: Env,
    // info.sender is the one burning tokens
    info: MessageInfo,
    curve_fn: CurveFn,
    // receiver is the one who gains (same for execute_sell, diff for execute_sell_from)
    receiver: Addr,
    amount: Uint128,
) -> Result<Response, ContractError> {
    // burn from the caller, this ensures there are tokens to cover this
    execute_burn(deps.branch(), env, info.clone(), amount)?;

    // calculate how many tokens can be purchased with this and mint them
    let mut state = CURVE_STATE.load(deps.storage)?;
    let curve = curve_fn(state.decimals);
    state.supply = state
        .supply
        .checked_sub(amount)
        .map_err(StdError::overflow)?;

    // curve.reserve() calculates CW20 -> native
    let new_reserve = curve.reserve(state.supply);
    let released = state
        .reserve
        .checked_sub(new_reserve)
        .map_err(StdError::overflow)?;
    state.reserve = new_reserve;
    CURVE_STATE.save(deps.storage, &state)?;

    // now send the tokens to the sender (TODO: for sell_from we do something else, right???)
    let msg = BankMsg::Send {
        to_address: receiver.to_string(),
        amount: coins(released.u128(), state.reserve_denom),
    };
    let res = Response::new()
        .add_message(msg)
        .add_attribute("from", info.sender)
        .add_attribute("supply", amount)
        .add_attribute("reserve", released);
    Ok(res)
}
