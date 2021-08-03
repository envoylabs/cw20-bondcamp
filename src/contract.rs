#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coins, to_binary, Addr, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Response,
    StdError, StdResult, Uint128,
};

use cw2::set_contract_version;
use cw20_base::allowances::{
    deduct_allowance, execute_decrease_allowance, execute_increase_allowance, execute_send_from,
    execute_transfer_from, query_allowance,
};
use cw20_base::contract::{execute_send, execute_transfer, query_balance};
use cw20_base::state::{MinterData, TokenInfo, BALANCES};

use crate::curves::DecimalPlaces;
use crate::error::ContractError;
use crate::msg::{CurveFn, CurveInfoResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::query::TokenInfoResponseWithMeta;
use crate::state::{CurveState, TokenInfoWithMeta, CURVE_STATE, CURVE_TYPE, TOKEN_INFO_WITH_META};
use cw0::{must_pay, nonpayable};
use cw20::TokenInfoResponse;

// version info for migration info
const CONTRACT_NAME: &str = "cw20-bondcamp";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    nonpayable(&info)?;
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // store token info using nested cw20-base format
    let data = TokenInfoWithMeta {
        external_permalink_uri: msg.external_permalink_uri,
        artist: msg.artist,
        work: msg.work,
        description: msg.description,
        asset_uri: msg.asset_uri,
        token_info: TokenInfo {
            name: msg.name,
            symbol: msg.symbol,
            decimals: msg.decimals,
            total_supply: Uint128(0),
            // set self as minter, so we can properly execute mint and burn
            mint: Some(MinterData {
                minter: env.contract.address,
                cap: None,
            }),
        },
    };
    TOKEN_INFO_WITH_META.save(deps.storage, &data)?;

    let places = DecimalPlaces::new(msg.decimals, msg.reserve_decimals);
    let supply = CurveState::new(msg.reserve_denom, places);
    CURVE_STATE.save(deps.storage, &supply)?;

    CURVE_TYPE.save(deps.storage, &msg.curve_type)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    // default implementation stores curve info as enum, you can do something else in a derived
    // contract and just pass in your custom curve to do_execute
    let curve_type = CURVE_TYPE.load(deps.storage)?;
    let curve_fn = curve_type.to_curve_fn();
    do_execute(deps, env, info, msg, curve_fn)
}

/// We pull out logic here, so we can import this from another contract and set a different Curve.
/// This contacts sets a curve with an enum in InstantiateMsg and stored in state, but you may want
/// to use custom math not included - make this easily reusable
pub fn do_execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
    curve_fn: CurveFn,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Buy {} => execute_buy(deps, env, info, curve_fn),

        // we override these from cw20
        // they are defined below
        ExecuteMsg::Burn { amount } => Ok(execute_sell(deps, env, info, curve_fn, amount)?),
        ExecuteMsg::BurnFrom { owner, amount } => {
            Ok(execute_sell_from(deps, env, info, curve_fn, owner, amount)?)
        }

        // these all come from cw20-base to implement the cw20 standard
        ExecuteMsg::Transfer { recipient, amount } => {
            Ok(execute_transfer(deps, env, info, recipient, amount)?)
        }
        ExecuteMsg::Send {
            contract,
            amount,
            msg,
        } => Ok(execute_send(deps, env, info, contract, amount, msg)?),
        ExecuteMsg::IncreaseAllowance {
            spender,
            amount,
            expires,
        } => Ok(execute_increase_allowance(
            deps, env, info, spender, amount, expires,
        )?),
        ExecuteMsg::DecreaseAllowance {
            spender,
            amount,
            expires,
        } => Ok(execute_decrease_allowance(
            deps, env, info, spender, amount, expires,
        )?),
        ExecuteMsg::TransferFrom {
            owner,
            recipient,
            amount,
        } => Ok(execute_transfer_from(
            deps, env, info, owner, recipient, amount,
        )?),
        ExecuteMsg::SendFrom {
            owner,
            contract,
            amount,
            msg,
        } => Ok(execute_send_from(
            deps, env, info, owner, contract, amount, msg,
        )?),
    }
}

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

    let res = Response {
        submessages: vec![],
        messages: vec![],
        attributes: vec![
            attr("action", "burn"),
            attr("from", info.sender),
            attr("amount", amount),
        ],
        data: None,
    };
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
    if let Some(limit) = config.get_cap() {
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

    let res = Response {
        submessages: vec![],
        messages: vec![],
        attributes: vec![
            attr("action", "mint"),
            attr("to", recipient),
            attr("amount", amount),
        ],
        data: None,
    };
    Ok(res)
}

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
    let res = Response {
        submessages: vec![],
        messages: vec![],
        attributes: vec![
            attr("action", "buy"),
            attr("from", info.sender),
            attr("reserve", payment),
            attr("supply", minted),
        ],
        data: None,
    };
    Ok(res)
}

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
    let res = Response {
        submessages: vec![],
        messages: vec![msg.into()],
        attributes: vec![
            attr("from", info.sender),
            attr("supply", amount),
            attr("reserve", released),
        ],
        data: None,
    };
    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    // default implementation stores curve info as enum, you can do something else in a derived
    // contract and just pass in your custom curve to do_execute
    let curve_type = CURVE_TYPE.load(deps.storage)?;
    let curve_fn = curve_type.to_curve_fn();
    do_query(deps, env, msg, curve_fn)
}

/// a light adaptation of the code in cw20-base to include meta
pub fn query_token_info_with_meta(deps: Deps) -> StdResult<TokenInfoResponseWithMeta> {
    let info = TOKEN_INFO_WITH_META.load(deps.storage)?;

    // note that asset uri is an option type
    // which we don't care about but clients might
    let res = TokenInfoResponseWithMeta {
        external_permalink_uri: info.external_permalink_uri,
        artist: info.artist,
        work: info.work,
        description: info.description,
        asset_uri: info.asset_uri,
        token_info_response: TokenInfoResponse {
            name: info.token_info.name,
            symbol: info.token_info.symbol,
            decimals: info.token_info.decimals,
            total_supply: info.token_info.total_supply,
        },
    };
    Ok(res)
}

/// We pull out logic here, so we can import this from another contract and set a different Curve.
/// This contacts sets a curve with an enum in InstantitateMsg and stored in state, but you may want
/// to use custom math not included - make this easily reusable
pub fn do_query(deps: Deps, _env: Env, msg: QueryMsg, curve_fn: CurveFn) -> StdResult<Binary> {
    match msg {
        // custom queries
        QueryMsg::CurveInfo {} => to_binary(&query_curve_info(deps, curve_fn)?),
        // inherited from cw20-base
        QueryMsg::TokenInfo {} => to_binary(&query_token_info_with_meta(deps)?),
        QueryMsg::Balance { address } => to_binary(&query_balance(deps, address)?),
        QueryMsg::Allowance { owner, spender } => {
            to_binary(&query_allowance(deps, owner, spender)?)
        }
    }
}

pub fn query_curve_info(deps: Deps, curve_fn: CurveFn) -> StdResult<CurveInfoResponse> {
    let CurveState {
        reserve,
        supply,
        reserve_denom,
        decimals,
    } = CURVE_STATE.load(deps.storage)?;

    // This we can get from the local digits stored in instantiate
    let curve = curve_fn(decimals);
    let spot_price = curve.spot_price(supply);

    Ok(CurveInfoResponse {
        reserve,
        supply,
        spot_price,
        reserve_denom,
    })
}

// this is poor mans "skip" flag
#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::CurveType;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coin, Decimal, OverflowError, OverflowOperation, StdError};
    use cw0::PaymentError;

    const DENOM: &str = "satoshi";
    const CREATOR: &str = "creator";
    const INVESTOR: &str = "investor";
    const BUYER: &str = "buyer";

    fn default_instantiate(
        asset_uri: Option<String>,
        decimals: u8,
        reserve_decimals: u8,
        curve_type: CurveType,
    ) -> InstantiateMsg {
        InstantiateMsg {
            external_permalink_uri:
                "https://squarepusher.bandcamp.com/album/feed-me-weird-things-remastered"
                    .to_string(),
            artist: "Squarepusher".to_string(),
            work: "Feed Me Weird Things (Remaster)".to_string(),
            description: "Feed Me Weird Things (Remaster) - Bandcamp".to_string(),
            asset_uri: asset_uri,
            name: "Bonded".to_string(),
            symbol: "EPOXY".to_string(),
            decimals,
            reserve_denom: DENOM.to_string(),
            reserve_decimals,
            curve_type,
        }
    }

    fn get_balance<U: Into<String>>(deps: Deps, addr: U) -> Uint128 {
        query_balance(deps, addr.into()).unwrap().balance
    }

    fn setup_test(
        deps: DepsMut,
        asset_uri: Option<String>,
        decimals: u8,
        reserve_decimals: u8,
        curve_type: CurveType,
    ) {
        // this matches `linear_curve` test case from curves.rs
        let creator = String::from(CREATOR);
        let msg = default_instantiate(asset_uri, decimals, reserve_decimals, curve_type);
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps, mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn proper_instantiation() {
        let mut deps = mock_dependencies(&[]);

        // this matches `linear_curve` test case from curves.rs
        let creator = String::from("creator");
        let curve_type = CurveType::SquareRoot {
            slope: Uint128(1),
            scale: 1,
        };
        let msg = default_instantiate(
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
        );
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
        assert_eq!(0, res.messages.len());

        // token info is proper
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(&token.external_permalink_uri, &msg.external_permalink_uri);
        assert_eq!(&token.artist, &msg.artist);
        assert_eq!(&token.work, &msg.work);
        assert_eq!(&token.description, &msg.description);
        assert_eq!(&token.asset_uri, &msg.asset_uri);
        assert_eq!(&token.token_info_response.name, &msg.name);
        assert_eq!(&token.token_info_response.symbol, &msg.symbol);
        assert_eq!(token.token_info_response.decimals, 2);
        assert_eq!(token.token_info_response.total_supply, Uint128(0));

        // curve state is sensible
        let state = query_curve_info(deps.as_ref(), curve_type.to_curve_fn()).unwrap();
        assert_eq!(state.reserve, Uint128(0));
        assert_eq!(state.supply, Uint128(0));
        assert_eq!(state.reserve_denom.as_str(), DENOM);
        // spot price 0 as supply is 0
        assert_eq!(state.spot_price, Decimal::zero());

        // curve type is stored properly
        let curve = CURVE_TYPE.load(&deps.storage).unwrap();
        assert_eq!(curve_type, curve);

        // no balance
        assert_eq!(get_balance(deps.as_ref(), &creator), Uint128(0));
    }

    #[test]
    fn proper_instantiation_even_with_no_asset_uri() {
        let mut deps = mock_dependencies(&[]);

        // this matches `linear_curve` test case from curves.rs
        let creator = String::from("creator");
        let curve_type = CurveType::SquareRoot {
            slope: Uint128(1),
            scale: 1,
        };
        let msg = default_instantiate(None, 2, 8, curve_type.clone());
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
        assert_eq!(0, res.messages.len());

        // token info is proper
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(&token.external_permalink_uri, &msg.external_permalink_uri);
        assert_eq!(&token.artist, &msg.artist);
        assert_eq!(&token.work, &msg.work);
        assert_eq!(&token.description, &msg.description);
        assert_eq!(&token.asset_uri, &msg.asset_uri);
        assert_eq!(&token.token_info_response.name, &msg.name);
        assert_eq!(&token.token_info_response.symbol, &msg.symbol);
        assert_eq!(token.token_info_response.decimals, 2);
        assert_eq!(token.token_info_response.total_supply, Uint128(0));

        // curve state is sensible
        let state = query_curve_info(deps.as_ref(), curve_type.to_curve_fn()).unwrap();
        assert_eq!(state.reserve, Uint128(0));
        assert_eq!(state.supply, Uint128(0));
        assert_eq!(state.reserve_denom.as_str(), DENOM);
        // spot price 0 as supply is 0
        assert_eq!(state.spot_price, Decimal::zero());

        // curve type is stored properly
        let curve = CURVE_TYPE.load(&deps.storage).unwrap();
        assert_eq!(curve_type, curve);

        // no balance
        assert_eq!(get_balance(deps.as_ref(), &creator), Uint128(0));
    }
    #[test]
    fn buy_issues_tokens() {
        let mut deps = mock_dependencies(&[]);
        let curve_type = CurveType::Linear {
            slope: Uint128(1),
            scale: 1,
        };
        setup_test(
            deps.as_mut(),
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
        );

        // succeeds with proper token (5 BTC = 5*10^8 satoshi)
        let info = mock_info(INVESTOR, &coins(500_000_000, DENOM));
        let buy = ExecuteMsg::Buy {};
        execute(deps.as_mut(), mock_env(), info, buy.clone()).unwrap();

        // bob got 1000 EPOXY (10.00)
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128(1000));
        assert_eq!(get_balance(deps.as_ref(), BUYER), Uint128(0));

        // send them all to buyer
        let info = mock_info(INVESTOR, &[]);
        let send = ExecuteMsg::Transfer {
            recipient: BUYER.into(),
            amount: Uint128(1000),
        };
        execute(deps.as_mut(), mock_env(), info, send).unwrap();

        // ensure balances updated
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128(0));
        assert_eq!(get_balance(deps.as_ref(), BUYER), Uint128(1000));

        // second stake needs more to get next 1000 EPOXY
        let info = mock_info(INVESTOR, &coins(1_500_000_000, DENOM));
        execute(deps.as_mut(), mock_env(), info, buy).unwrap();

        // ensure balances updated
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128(1000));
        assert_eq!(get_balance(deps.as_ref(), BUYER), Uint128(1000));

        // check curve info updated
        let curve = query_curve_info(deps.as_ref(), curve_type.to_curve_fn()).unwrap();
        assert_eq!(curve.reserve, Uint128(2_000_000_000));
        assert_eq!(curve.supply, Uint128(2000));
        assert_eq!(curve.spot_price, Decimal::percent(200));

        // check token info updated
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(token.token_info_response.decimals, 2);
        assert_eq!(token.token_info_response.total_supply, Uint128(2000));
    }

    #[test]
    fn bonding_fails_with_wrong_denom() {
        let mut deps = mock_dependencies(&[]);
        let curve_type = CurveType::Linear {
            slope: Uint128(1),
            scale: 1,
        };
        setup_test(
            deps.as_mut(),
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type,
        );

        // fails when no tokens sent
        let info = mock_info(INVESTOR, &[]);
        let buy = ExecuteMsg::Buy {};
        let err = execute(deps.as_mut(), mock_env(), info, buy.clone()).unwrap_err();
        assert_eq!(err, PaymentError::NoFunds {}.into());

        // fails when wrong tokens sent
        let info = mock_info(INVESTOR, &coins(1234567, "wei"));
        let err = execute(deps.as_mut(), mock_env(), info, buy.clone()).unwrap_err();
        assert_eq!(err, PaymentError::MissingDenom(DENOM.into()).into());

        // fails when too many tokens sent
        let info = mock_info(INVESTOR, &[coin(3400022, DENOM), coin(1234567, "wei")]);
        let err = execute(deps.as_mut(), mock_env(), info, buy).unwrap_err();
        assert_eq!(err, PaymentError::MultipleDenoms {}.into());
    }

    #[test]
    fn burning_sends_reserve() {
        let mut deps = mock_dependencies(&[]);
        let curve_type = CurveType::Linear {
            slope: Uint128(1),
            scale: 1,
        };
        setup_test(
            deps.as_mut(),
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
        );

        // succeeds with proper token (20 BTC = 20*10^8 satoshi)
        let info = mock_info(INVESTOR, &coins(2_000_000_000, DENOM));
        let buy = ExecuteMsg::Buy {};
        execute(deps.as_mut(), mock_env(), info, buy).unwrap();

        // bob got 2000 EPOXY (20.00)
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128(2000));

        // cannot burn too much
        let info = mock_info(INVESTOR, &[]);
        let burn = ExecuteMsg::Burn {
            amount: Uint128(3000),
        };
        let err = execute(deps.as_mut(), mock_env(), info, burn).unwrap_err();
        assert_eq!(
            err,
            ContractError::Std(StdError::overflow(OverflowError::new(
                OverflowOperation::Sub,
                2000,
                3000
            )))
        );

        // burn 1000 EPOXY to get back 15BTC (*10^8)
        let info = mock_info(INVESTOR, &[]);
        let burn = ExecuteMsg::Burn {
            amount: Uint128(1000),
        };
        let res = execute(deps.as_mut(), mock_env(), info, burn).unwrap();

        // balance is lower
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128(1000));

        // ensure we got our money back
        assert_eq!(1, res.messages.len());
        assert_eq!(
            &res.messages[0],
            &BankMsg::Send {
                to_address: INVESTOR.into(),
                amount: coins(1_500_000_000, DENOM),
            }
            .into()
        );

        // check curve info updated
        let curve = query_curve_info(deps.as_ref(), curve_type.to_curve_fn()).unwrap();
        assert_eq!(curve.reserve, Uint128(500_000_000));
        assert_eq!(curve.supply, Uint128(1000));
        assert_eq!(curve.spot_price, Decimal::percent(100));

        // check token info updated
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(token.token_info_response.decimals, 2);
        assert_eq!(token.token_info_response.total_supply, Uint128(1000));
    }

    #[test]
    fn cw20_imports_work() {
        let mut deps = mock_dependencies(&[]);
        let curve_type = CurveType::Constant {
            value: Uint128(15),
            scale: 1,
        };
        setup_test(
            deps.as_mut(),
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            9,
            6,
            curve_type,
        );

        let alice: &str = "alice";
        let bob: &str = "bobby";
        let carl: &str = "carl";

        // spend 45_000 uatom for 30_000_000 EPOXY
        let info = mock_info(bob, &coins(45_000, DENOM));
        let buy = ExecuteMsg::Buy {};
        execute(deps.as_mut(), mock_env(), info, buy).unwrap();

        // check balances
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128(30_000_000));
        assert_eq!(get_balance(deps.as_ref(), carl), Uint128(0));

        // send coins to carl
        let bob_info = mock_info(bob, &[]);
        let transfer = ExecuteMsg::Transfer {
            recipient: carl.into(),
            amount: Uint128(2_000_000),
        };
        execute(deps.as_mut(), mock_env(), bob_info.clone(), transfer).unwrap();
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128(28_000_000));
        assert_eq!(get_balance(deps.as_ref(), carl), Uint128(2_000_000));

        // allow alice
        let allow = ExecuteMsg::IncreaseAllowance {
            spender: alice.into(),
            amount: Uint128(35_000_000),
            expires: None,
        };
        execute(deps.as_mut(), mock_env(), bob_info, allow).unwrap();
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128(28_000_000));
        assert_eq!(get_balance(deps.as_ref(), alice), Uint128(0));
        assert_eq!(
            query_allowance(deps.as_ref(), bob.into(), alice.into())
                .unwrap()
                .allowance,
            Uint128(35_000_000)
        );

        // alice takes some for herself
        let self_pay = ExecuteMsg::TransferFrom {
            owner: bob.into(),
            recipient: alice.into(),
            amount: Uint128(25_000_000),
        };
        let alice_info = mock_info(alice, &[]);
        execute(deps.as_mut(), mock_env(), alice_info, self_pay).unwrap();
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128(3_000_000));
        assert_eq!(get_balance(deps.as_ref(), alice), Uint128(25_000_000));
        assert_eq!(get_balance(deps.as_ref(), carl), Uint128(2_000_000));
        assert_eq!(
            query_allowance(deps.as_ref(), bob.into(), alice.into())
                .unwrap()
                .allowance,
            Uint128(10_000_000)
        );

        // test burn from works properly (burn tested in burning_sends_reserve)
        // cannot burn more than they have

        let info = mock_info(alice, &[]);
        let burn_from = ExecuteMsg::BurnFrom {
            owner: bob.into(),
            amount: Uint128(3_300_000),
        };
        let err = execute(deps.as_mut(), mock_env(), info, burn_from).unwrap_err();
        assert_eq!(
            err,
            ContractError::Std(StdError::overflow(OverflowError::new(
                OverflowOperation::Sub,
                3000000,
                3300000
            )))
        );

        // burn 1_000_000 EPOXY to get back 1_500 DENOM (constant curve)
        let info = mock_info(alice, &[]);
        let burn_from = ExecuteMsg::BurnFrom {
            owner: bob.into(),
            amount: Uint128(1_000_000),
        };
        let res = execute(deps.as_mut(), mock_env(), info, burn_from).unwrap();

        // bob balance is lower, not alice
        assert_eq!(get_balance(deps.as_ref(), alice), Uint128(25_000_000));
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128(2_000_000));

        // ensure alice got our money back
        assert_eq!(1, res.messages.len());
        assert_eq!(
            &res.messages[0],
            &BankMsg::Send {
                to_address: alice.into(),
                amount: coins(1_500, DENOM),
            }
            .into()
        );
    }
}
