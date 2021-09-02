#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, Uint128,
};

use cw2::set_contract_version;
use cw20_base::allowances::{
    execute_decrease_allowance, execute_increase_allowance, execute_send_from,
    execute_transfer_from, query_allowance,
};
use cw20_base::contract::{execute_send, execute_transfer, query_balance};
use cw20_base::state::{MinterData, TokenInfo};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::query::{CurveInfoResponse, TokenInfoResponseWithMeta};
use crate::state::{
    CurveState, InvestmentInfo, TokenInfoWithMeta, CLAIMS, CURVE_STATE, CURVE_TYPE, INVESTMENT,
    TOKEN_INFO_WITH_META,
};
use cw0::nonpayable;
use cw20::TokenInfoResponse;
use cw20_bonding::msg::CurveFn;

use cw20_bonding::curves::DecimalPlaces;

use crate::bonding::{execute_buy, execute_sell, execute_sell_from};
use crate::staking::{_bond_all_tokens, bond, claim, query_investment, reinvest, unbond};

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

    // ensure the validator is registered
    let vals = deps.querier.query_all_validators()?;
    if !vals
        .iter()
        .any(|v| v.address == msg.staking_params.validator)
    {
        return Err(ContractError::NotInValidatorSet {
            validator: msg.staking_params.validator,
        });
    }

    // store token info using nested cw20-base format
    let data = TokenInfoWithMeta {
        external_permalink_uri: msg.external_permalink_uri,
        creator: msg.creator,
        work: msg.work,
        description: msg.description,
        asset_uri: msg.asset_uri,
        token_info: TokenInfo {
            name: msg.name,
            symbol: msg.symbol,
            decimals: msg.decimals,
            total_supply: Uint128::new(0),
            // set self as minter, so we can properly execute mint and burn
            mint: Some(MinterData {
                minter: env.contract.address,
                cap: None,
            }),
        },
    };
    TOKEN_INFO_WITH_META.save(deps.storage, &data)?;

    // marshal data for investment info
    let denom = deps.querier.query_bonded_denom()?;
    let investment_info = InvestmentInfo {
        owner: info.sender,
        exit_tax: msg.staking_params.exit_tax,
        unbonding_period: msg.staking_params.unbonding_period,
        bond_denom: denom,
        validator: msg.staking_params.validator,
        min_withdrawal: msg.staking_params.min_withdrawal,
    };
    INVESTMENT.save(deps.storage, &investment_info)?;

    // set supply to 0
    // let supply = Supply::default();
    // TOTAL_SUPPLY.save(deps.storage, &supply)?;

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

        // this is the staking logic
        ExecuteMsg::Bond {} => bond(deps, env, info, curve_fn),
        ExecuteMsg::Unbond { amount } => unbond(deps, env, info, curve_fn, amount),
        ExecuteMsg::Claim {} => claim(deps, env, info),
        ExecuteMsg::Reinvest {} => reinvest(deps, env, info),
        ExecuteMsg::_BondAllTokens {} => _bond_all_tokens(deps, env, info),

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
        creator: info.creator,
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
        // // custom queries for staking
        QueryMsg::Claims { address } => {
            to_binary(&CLAIMS.query_claims(deps, &deps.api.addr_validate(&address)?)?)
        }
        QueryMsg::Investment {} => to_binary(&query_investment(deps)?),
        // custom queries for bonding
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
        claims,
    } = CURVE_STATE.load(deps.storage)?;

    // This we can get from the local digits stored in instantiate
    let curve = curve_fn(decimals);
    let spot_price = curve.spot_price(supply);

    Ok(CurveInfoResponse {
        reserve,
        supply,
        spot_price,
        reserve_denom,
        claims,
    })
}

// this is poor mans "skip" flag
#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::StakingParams;
    //use cw20_base::contract::query_token_info;
    use cw_controllers::Claim;
    use std::str::FromStr;

    use crate::msg::CurveType;

    use cosmwasm_std::testing::{
        mock_dependencies, mock_env, mock_info, MockQuerier, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        coin, coins, Addr, BankMsg, Coin, CosmosMsg, Decimal, FullDelegation, OverflowError,
        OverflowOperation, StakingMsg, StdError, SubMsg, Validator,
    };
    use cw0::{Duration, PaymentError, DAY, HOUR};

    const DENOM: &str = "satoshi";
    const CREATOR: &str = "creator";
    const INVESTOR: &str = "investor";
    const BUYER: &str = "buyer";
    const DEFAULT_VALIDATOR: &str = "default-validator";

    fn default_instantiate(
        asset_uri: Option<String>,
        decimals: u8,
        reserve_decimals: u8,
        curve_type: CurveType,
        tax_percent: u64,
        min_withdrawal: u128,
    ) -> InstantiateMsg {
        InstantiateMsg {
            external_permalink_uri:
                "https://squarepusher.bandcamp.com/album/feed-me-weird-things-remastered"
                    .to_string(),
            creator: "Squarepusher".to_string(),
            work: "Feed Me Weird Things (Remaster)".to_string(),
            description: "Feed Me Weird Things (Remaster) - Bandcamp".to_string(),
            asset_uri: asset_uri,
            name: "Windscale2Coin".to_string(),
            symbol: "WIND".to_string(),
            decimals,
            reserve_denom: DENOM.to_string(),
            reserve_decimals,
            curve_type,
            staking_params: StakingParams {
                validator: String::from(DEFAULT_VALIDATOR),
                unbonding_period: DAY * 3,
                exit_tax: Decimal::percent(tax_percent),
                min_withdrawal: Uint128::new(min_withdrawal),
            },
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
        let msg = default_instantiate(asset_uri, decimals, reserve_decimals, curve_type, 2, 50);
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps, mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn proper_instantiation() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        // this matches `linear_curve` test case from curves.rs
        let creator = String::from("creator");
        let curve_type = CurveType::SquareRoot {
            slope: Uint128::new(1),
            scale: 1,
        };
        let msg = default_instantiate(
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
            2,
            50,
        );
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
        assert_eq!(0, res.messages.len());

        // token info is proper
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(&token.external_permalink_uri, &msg.external_permalink_uri);
        assert_eq!(&token.creator, &msg.creator);
        assert_eq!(&token.work, &msg.work);
        assert_eq!(&token.description, &msg.description);
        assert_eq!(&token.asset_uri, &msg.asset_uri);
        assert_eq!(&token.token_info_response.name, &msg.name);
        assert_eq!(&token.token_info_response.symbol, &msg.symbol);
        assert_eq!(token.token_info_response.decimals, 2);
        assert_eq!(token.token_info_response.total_supply, Uint128::new(0));

        // curve state is sensible
        let state = query_curve_info(deps.as_ref(), curve_type.to_curve_fn()).unwrap();
        assert_eq!(state.reserve, Uint128::new(0));
        assert_eq!(state.supply, Uint128::new(0));
        assert_eq!(state.reserve_denom.as_str(), DENOM);
        // spot price 0 as supply is 0
        assert_eq!(state.spot_price, Decimal::zero());

        // curve type is stored properly
        let curve = CURVE_TYPE.load(&deps.storage).unwrap();
        assert_eq!(curve_type, curve);

        // no balance
        assert_eq!(get_balance(deps.as_ref(), &creator), Uint128::new(0));
    }

    #[test]
    fn proper_instantiation_even_with_no_asset_uri() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        // this matches `linear_curve` test case from curves.rs
        let creator = String::from("creator");
        let curve_type = CurveType::SquareRoot {
            slope: Uint128::new(1),
            scale: 1,
        };
        let msg = default_instantiate(None, 2, 8, curve_type.clone(), 2, 50);
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
        assert_eq!(0, res.messages.len());

        // token info is proper
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(&token.external_permalink_uri, &msg.external_permalink_uri);
        assert_eq!(&token.creator, &msg.creator);
        assert_eq!(&token.work, &msg.work);
        assert_eq!(&token.description, &msg.description);
        assert_eq!(&token.asset_uri, &msg.asset_uri);
        assert_eq!(&token.token_info_response.name, &msg.name);
        assert_eq!(&token.token_info_response.symbol, &msg.symbol);
        assert_eq!(token.token_info_response.decimals, 2);
        assert_eq!(token.token_info_response.total_supply, Uint128::new(0));

        // curve state is sensible
        let state = query_curve_info(deps.as_ref(), curve_type.to_curve_fn()).unwrap();
        assert_eq!(state.reserve, Uint128::new(0));
        assert_eq!(state.supply, Uint128::new(0));
        assert_eq!(state.reserve_denom.as_str(), DENOM);
        // spot price 0 as supply is 0
        assert_eq!(state.spot_price, Decimal::zero());

        // curve type is stored properly
        let curve = CURVE_TYPE.load(&deps.storage).unwrap();
        assert_eq!(curve_type, curve);

        // no balance
        assert_eq!(get_balance(deps.as_ref(), &creator), Uint128::new(0));
    }
    #[test]
    fn buy_issues_tokens() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        let curve_type = CurveType::Linear {
            slope: Uint128::new(1),
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
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128::new(1000));
        assert_eq!(get_balance(deps.as_ref(), BUYER), Uint128::new(0));

        // send them all to buyer
        let info = mock_info(INVESTOR, &[]);
        let send = ExecuteMsg::Transfer {
            recipient: BUYER.into(),
            amount: Uint128::new(1000),
        };
        execute(deps.as_mut(), mock_env(), info, send).unwrap();

        // ensure balances updated
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128::new(0));
        assert_eq!(get_balance(deps.as_ref(), BUYER), Uint128::new(1000));

        // second stake needs more to get next 1000 EPOXY
        let info = mock_info(INVESTOR, &coins(1_500_000_000, DENOM));
        execute(deps.as_mut(), mock_env(), info, buy).unwrap();

        // ensure balances updated
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128::new(1000));
        assert_eq!(get_balance(deps.as_ref(), BUYER), Uint128::new(1000));

        // check curve info updated
        let curve = query_curve_info(deps.as_ref(), curve_type.to_curve_fn()).unwrap();
        assert_eq!(curve.reserve, Uint128::new(2_000_000_000));
        assert_eq!(curve.supply, Uint128::new(2000));
        assert_eq!(curve.spot_price, Decimal::percent(200));

        // check token info updated
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(token.token_info_response.decimals, 2);
        assert_eq!(token.token_info_response.total_supply, Uint128::new(2000));
    }

    #[test]
    fn buying_fails_with_wrong_denom() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        let curve_type = CurveType::Linear {
            slope: Uint128::new(1),
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
        set_validator(&mut deps.querier);

        let curve_type = CurveType::Linear {
            slope: Uint128::new(1),
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
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128::new(2000));

        // cannot burn too much
        let info = mock_info(INVESTOR, &[]);
        let burn = ExecuteMsg::Burn {
            amount: Uint128::new(3000),
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
            amount: Uint128::new(1000),
        };
        let res = execute(deps.as_mut(), mock_env(), info, burn).unwrap();

        // balance is lower
        assert_eq!(get_balance(deps.as_ref(), INVESTOR), Uint128::new(1000));

        // ensure we got our money back
        assert_eq!(1, res.messages.len());
        assert_eq!(
            &res.messages[0],
            &SubMsg::new(BankMsg::Send {
                to_address: INVESTOR.into(),
                amount: coins(1_500_000_000, DENOM),
            })
        );

        // check curve info updated
        let curve = query_curve_info(deps.as_ref(), curve_type.to_curve_fn()).unwrap();
        assert_eq!(curve.reserve, Uint128::new(500_000_000));
        assert_eq!(curve.supply, Uint128::new(1000));
        assert_eq!(curve.spot_price, Decimal::percent(100));

        // check token info updated
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(token.token_info_response.decimals, 2);
        assert_eq!(token.token_info_response.total_supply, Uint128::new(1000));
    }

    //
    //  ---- staking starts here ----
    //

    fn sample_validator(addr: &str) -> Validator {
        Validator {
            address: addr.into(),
            commission: Decimal::percent(3),
            max_commission: Decimal::percent(10),
            max_change_rate: Decimal::percent(1),
        }
    }

    fn set_validator(querier: &mut MockQuerier) {
        querier.update_staking("ustake", &[sample_validator(DEFAULT_VALIDATOR)], &[]);
    }

    fn sample_delegation(addr: &str, amount: Coin) -> FullDelegation {
        let can_redelegate = amount.clone();
        let accumulated_rewards = coins(0, &amount.denom);
        FullDelegation {
            validator: addr.into(),
            delegator: Addr::unchecked(MOCK_CONTRACT_ADDR),
            amount,
            can_redelegate,
            accumulated_rewards,
        }
    }

    fn set_delegation(querier: &mut MockQuerier, amount: u128, denom: &str) {
        querier.update_staking(
            "ustake",
            &[sample_validator(DEFAULT_VALIDATOR)],
            &[sample_delegation(DEFAULT_VALIDATOR, coin(amount, denom))],
        );
    }

    // just a test helper, forgive the panic
    fn later(env: &Env, delta: Duration) -> Env {
        let time_delta = match delta {
            Duration::Time(t) => t,
            _ => panic!("Must provide duration in time"),
        };
        let mut res = env.clone();
        res.block.time = res.block.time.plus_seconds(time_delta);
        res
    }

    fn get_claims(deps: Deps, addr: &str) -> Vec<Claim> {
        CLAIMS
            .query_claims(deps, &Addr::unchecked(addr))
            .unwrap()
            .claims
    }

    #[test]
    fn proper_staking_instantiation() {
        let mut deps = mock_dependencies(&[]);
        deps.querier.update_staking(
            "ustake",
            &[
                sample_validator("john"),
                sample_validator("mary"),
                sample_validator("my-validator-addr"),
            ],
            &[],
        );

        let curve_type = CurveType::Linear {
            slope: Uint128::new(1),
            scale: 1,
        };

        let creator = String::from("creator");
        let msg = InstantiateMsg {
            external_permalink_uri:
                "https://squarepusher.bandcamp.com/album/feed-me-weird-things-remastered"
                    .to_string(),
            creator: "Squarepusher".to_string(),
            work: "Feed Me Weird Things (Remaster)".to_string(),
            description: "Feed Me Weird Things (Remaster) - Bandcamp".to_string(),
            name: "Windscale2Coin".to_string(),
            symbol: "WIND".to_string(),
            decimals: 2,
            reserve_denom: DENOM.to_string(),
            reserve_decimals: 8,
            asset_uri: None,
            curve_type: curve_type.clone(),
            staking_params: StakingParams {
                validator: String::from("my-validator-addr"),
                unbonding_period: DAY * 3,
                exit_tax: Decimal::percent(2),
                min_withdrawal: Uint128::new(50),
            },
        };
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
        assert_eq!(0, res.messages.len());

        // token info is proper
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(&token.token_info_response.name, &msg.name);
        assert_eq!(&token.token_info_response.symbol, &msg.symbol);
        assert_eq!(token.token_info_response.decimals, msg.decimals);
        assert_eq!(token.token_info_response.total_supply, Uint128::zero());

        // no balance
        assert_eq!(get_balance(deps.as_ref(), &creator), Uint128::zero());
        // no claims
        assert_eq!(get_claims(deps.as_ref(), &creator), vec![]);

        // investment info correct
        let invest = query_investment(deps.as_ref()).unwrap();
        assert_eq!(&invest.owner, &creator);
        assert_eq!(&invest.validator, &msg.staking_params.validator);
        assert_eq!(invest.exit_tax, msg.staking_params.exit_tax);
        assert_eq!(invest.min_withdrawal, msg.staking_params.min_withdrawal);

        assert_eq!(invest.token_supply, Uint128::zero());
        assert_eq!(invest.staked_tokens, coin(0, "ustake"));
        assert_eq!(invest.nominal_value, Decimal::one());
    }

    #[test]
    fn instantiation_with_missing_validator() {
        let mut deps = mock_dependencies(&[]);
        deps.querier
            .update_staking("ustake", &[sample_validator("john")], &[]);

        let curve_type = CurveType::Linear {
            slope: Uint128::new(1),
            scale: 1,
        };

        let creator = String::from("creator");
        let msg = InstantiateMsg {
            external_permalink_uri:
                "https://squarepusher.bandcamp.com/album/feed-me-weird-things-remastered"
                    .to_string(),
            creator: "Squarepusher".to_string(),
            work: "Feed Me Weird Things (Remaster)".to_string(),
            description: "Feed Me Weird Things (Remaster) - Bandcamp".to_string(),
            name: "Windscale2Coin".to_string(),
            symbol: "WIND".to_string(),
            decimals: 2,
            reserve_denom: DENOM.to_string(),
            reserve_decimals: 8,
            asset_uri: None,
            curve_type: curve_type.clone(),
            staking_params: StakingParams {
                validator: String::from("my-validator-addr"),
                unbonding_period: DAY * 3,
                exit_tax: Decimal::percent(2),
                min_withdrawal: Uint128::new(50),
            },
        };
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let err = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert_eq!(
            err,
            ContractError::NotInValidatorSet {
                validator: "my-validator-addr".into()
            }
        );
    }

    #[test]
    fn bonding_issues_tokens() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        let curve_type = CurveType::SquareRoot {
            slope: Uint128::new(1),
            scale: 1,
        };
        let creator = String::from("creator");
        let instantiate_msg = default_instantiate(
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
            2,
            50,
        );
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, instantiate_msg).unwrap();
        assert_eq!(0, res.messages.len());

        // let's bond some tokens now
        let bob = String::from("bob");
        let bond_msg = ExecuteMsg::Bond {};
        let info = mock_info(&bob, &[coin(10, "random"), coin(1000, "ustake")]);

        // try to bond and make sure we trigger delegation
        let res = execute(deps.as_mut(), mock_env(), info, bond_msg).unwrap();
        assert_eq!(1, res.messages.len());
        let delegate = &res.messages[0];
        match &delegate.msg {
            CosmosMsg::Staking(StakingMsg::Delegate { validator, amount }) => {
                assert_eq!(validator.as_str(), DEFAULT_VALIDATOR);
                assert_eq!(amount, &coin(1000, "ustake"));
            }
            _ => panic!("Unexpected message: {:?}", delegate),
        }

        // bob got 1000 DRV for 1000 stake at a 1.0 ratio
        assert_eq!(get_balance(deps.as_ref(), &bob), Uint128::new(1000));

        // investment info correct (updated supply)
        let invest = query_investment(deps.as_ref()).unwrap();
        assert_eq!(invest.token_supply, Uint128::new(1000));
        assert_eq!(invest.staked_tokens, coin(1000, "ustake"));
        assert_eq!(invest.nominal_value, Decimal::one());

        // token info also properly updated
        let token = query_token_info_with_meta(deps.as_ref()).unwrap();
        assert_eq!(token.token_info_response.total_supply, Uint128::new(1000));
    }

    #[test]
    fn rebonding_changes_pricing() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        let curve_type = CurveType::SquareRoot {
            slope: Uint128::new(1),
            scale: 1,
        };

        let creator = String::from("creator");
        let instantiate_msg = default_instantiate(
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
            2,
            50,
        );
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, instantiate_msg).unwrap();
        assert_eq!(0, res.messages.len());

        // let's bond some tokens now
        let bob = String::from("bob");
        let bond_msg = ExecuteMsg::Bond {};
        let info = mock_info(&bob, &[coin(10, "random"), coin(1000, "ustake")]);
        let res = execute(deps.as_mut(), mock_env(), info, bond_msg).unwrap();
        assert_eq!(1, res.messages.len());

        // update the querier with new bond
        set_delegation(&mut deps.querier, 1000, "ustake");

        // fake a reinvestment (this must be sent by the contract itself)
        let rebond_msg = ExecuteMsg::_BondAllTokens {};
        let info = mock_info(MOCK_CONTRACT_ADDR, &[]);
        deps.querier
            .update_balance(MOCK_CONTRACT_ADDR, coins(500, "ustake"));
        let _ = execute(deps.as_mut(), mock_env(), info, rebond_msg).unwrap();

        // update the querier with new bond
        set_delegation(&mut deps.querier, 1500, "ustake");

        // we should now see 1000 issues and 1500 bonded (and a price of 1.5)
        let invest = query_investment(deps.as_ref()).unwrap();
        assert_eq!(invest.token_supply, Uint128::new(1000));
        assert_eq!(invest.staked_tokens, coin(1500, "ustake"));
        let ratio = Decimal::from_str("1.5").unwrap();
        assert_eq!(invest.nominal_value, ratio);

        // we bond some other tokens and get a different issuance price (maintaining the ratio)
        let alice = String::from("alice");
        let bond_msg = ExecuteMsg::Bond {};
        let info = mock_info(&alice, &[coin(3000, "ustake")]);
        let res = execute(deps.as_mut(), mock_env(), info, bond_msg).unwrap();
        assert_eq!(1, res.messages.len());

        // update the querier with new bond
        set_delegation(&mut deps.querier, 3000, "ustake");

        // alice should have gotten 2000 DRV for the 3000 stake, keeping the ratio at 1.5
        assert_eq!(get_balance(deps.as_ref(), &alice), Uint128::new(2000));

        let invest = query_investment(deps.as_ref()).unwrap();
        assert_eq!(invest.token_supply, Uint128::new(3000));
        assert_eq!(invest.staked_tokens, coin(4500, "ustake"));
        assert_eq!(invest.nominal_value, ratio);
    }

    #[test]
    fn bonding_fails_with_wrong_denom() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        let curve_type = CurveType::SquareRoot {
            slope: Uint128::new(1),
            scale: 1,
        };

        let creator = String::from("creator");
        let instantiate_msg = default_instantiate(
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
            2,
            50,
        );
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, instantiate_msg).unwrap();
        assert_eq!(0, res.messages.len());

        // let's bond some tokens now
        let bob = String::from("bob");
        let bond_msg = ExecuteMsg::Bond {};
        let info = mock_info(&bob, &[coin(500, "photon")]);

        // try to bond and make sure we trigger delegation
        let err = execute(deps.as_mut(), mock_env(), info, bond_msg).unwrap_err();
        assert_eq!(
            err,
            ContractError::EmptyBalance {
                denom: "ustake".to_string()
            }
        );
    }

    #[test]
    fn unbonding_maintains_price_ratio() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        let curve_type = CurveType::SquareRoot {
            slope: Uint128::new(1),
            scale: 1,
        };

        let creator = String::from("creator");
        let instantiate_msg = default_instantiate(
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
            10,
            50,
        );
        let info = mock_info(&creator, &[]);

        // make sure we can instantiate with this
        let res = instantiate(deps.as_mut(), mock_env(), info, instantiate_msg).unwrap();
        assert_eq!(0, res.messages.len());

        // let's bond some tokens now
        let bob = String::from("bob");
        let bond_msg = ExecuteMsg::Bond {};
        let info = mock_info(&bob, &[coin(10, "random"), coin(1000, "ustake")]);
        let res = execute(deps.as_mut(), mock_env(), info, bond_msg).unwrap();
        assert_eq!(1, res.messages.len());

        // update the querier with new bond
        set_delegation(&mut deps.querier, 1000, "ustake");

        // fake a reinvestment (this must be sent by the contract itself)
        // after this, we see 1000 issues and 1500 bonded (and a price of 1.5)
        let rebond_msg = ExecuteMsg::_BondAllTokens {};
        let info = mock_info(MOCK_CONTRACT_ADDR, &[]);
        deps.querier
            .update_balance(MOCK_CONTRACT_ADDR, coins(500, "ustake"));
        let _ = execute(deps.as_mut(), mock_env(), info, rebond_msg).unwrap();

        // update the querier with new bond, lower balance
        set_delegation(&mut deps.querier, 1500, "ustake");
        deps.querier.update_balance(MOCK_CONTRACT_ADDR, vec![]);

        // creator now tries to unbond these tokens - this must fail
        let unbond_msg = ExecuteMsg::Unbond {
            amount: Uint128::new(600),
        };
        let info = mock_info(&creator, &[]);
        let err = execute(deps.as_mut(), mock_env(), info, unbond_msg).unwrap_err();
        assert_eq!(
            err,
            ContractError::Std(StdError::overflow(OverflowError::new(
                OverflowOperation::Sub,
                0,
                600
            )))
        );

        // bob unbonds 600 tokens at 10% tax...
        // 60 are taken and send to the owner
        // 540 are unbonded in exchange for 540 * 1.5 = 810 native tokens
        let unbond_msg = ExecuteMsg::Unbond {
            amount: Uint128::new(600),
        };
        let owner_cut = Uint128::new(60);
        let bobs_claim = Uint128::new(810);
        let bobs_balance = Uint128::new(400);
        let env = mock_env();
        let info = mock_info(&bob, &[]);
        let res = execute(deps.as_mut(), env.clone(), info, unbond_msg).unwrap();
        assert_eq!(1, res.messages.len());
        let delegate = &res.messages[0];
        match &delegate.msg {
            CosmosMsg::Staking(StakingMsg::Undelegate { validator, amount }) => {
                assert_eq!(validator.as_str(), DEFAULT_VALIDATOR);
                assert_eq!(amount, &coin(bobs_claim.u128(), "ustake"));
            }
            _ => panic!("Unexpected message: {:?}", delegate),
        }

        // update the querier with new bond, lower balance
        set_delegation(&mut deps.querier, 690, "ustake");

        // check balances
        assert_eq!(get_balance(deps.as_ref(), &bob), bobs_balance);
        assert_eq!(get_balance(deps.as_ref(), &creator), owner_cut);
        // proper claims
        let expected_claims = vec![Claim {
            amount: bobs_claim,
            release_at: (DAY * 3).after(&env.block),
        }];
        assert_eq!(expected_claims, get_claims(deps.as_ref(), &bob));

        // supplies updated, ratio the same (1.5)
        let ratio = Decimal::from_str("1.5").unwrap();

        let invest = query_investment(deps.as_ref()).unwrap();
        assert_eq!(invest.token_supply, bobs_balance + owner_cut);
        assert_eq!(invest.staked_tokens, coin(690, "ustake")); // 1500 - 810
        assert_eq!(invest.nominal_value, ratio);
    }

    #[test]
    fn claims_paid_out_properly() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        let curve_type = CurveType::SquareRoot {
            slope: Uint128::new(1),
            scale: 1,
        };

        // create contract
        let creator = String::from("creator");
        let instantiate_msg = default_instantiate(
            Some("https://f4.bcbits.com/img/a0113459728_10.jpg".to_string()),
            2,
            8,
            curve_type.clone(),
            10,
            50,
        );
        let info = mock_info(&creator, &[]);
        instantiate(deps.as_mut(), mock_env(), info, instantiate_msg).unwrap();

        // bond some tokens
        let bob = String::from("bob");
        let info = mock_info(&bob, &coins(1000, "ustake"));
        execute(deps.as_mut(), mock_env(), info, ExecuteMsg::Bond {}).unwrap();
        set_delegation(&mut deps.querier, 1000, "ustake");

        // unbond part of them
        let unbond_msg = ExecuteMsg::Unbond {
            amount: Uint128::new(600),
        };
        let env = mock_env();
        let info = mock_info(&bob, &[]);
        execute(deps.as_mut(), env.clone(), info.clone(), unbond_msg).unwrap();
        set_delegation(&mut deps.querier, 460, "ustake");

        // ensure claims are proper
        let bobs_claim = Uint128::new(540);
        let original_claims = vec![Claim {
            amount: bobs_claim,
            release_at: (DAY * 3).after(&env.block),
        }];
        assert_eq!(original_claims, get_claims(deps.as_ref(), &bob));

        // bob cannot exercise claims without enough balance
        let claim_ready = later(&env, (DAY * 3 + HOUR).unwrap());
        let too_soon = later(&env, DAY);
        let fail = execute(
            deps.as_mut(),
            claim_ready.clone(),
            info.clone(),
            ExecuteMsg::Claim {},
        );
        assert!(fail.is_err(), "{:?}", fail);

        // provide the balance, but claim not yet mature - also prohibited
        deps.querier
            .update_balance(MOCK_CONTRACT_ADDR, coins(540, "ustake"));
        let fail = execute(deps.as_mut(), too_soon, info.clone(), ExecuteMsg::Claim {});
        assert!(fail.is_err(), "{:?}", fail);

        // this should work with cash and claims ready
        let res = execute(deps.as_mut(), claim_ready, info, ExecuteMsg::Claim {}).unwrap();
        assert_eq!(1, res.messages.len());
        let payout = &res.messages[0];
        match &payout.msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(amount, &coins(540, "ustake"));
                assert_eq!(to_address, &bob);
            }
            _ => panic!("Unexpected message: {:?}", payout),
        }

        // claims have been removed
        assert_eq!(get_claims(deps.as_ref(), &bob), vec![]);
    }

    //
    //  ---- staking ends here ----
    //

    #[test]
    fn cw20_imports_work() {
        let mut deps = mock_dependencies(&[]);
        set_validator(&mut deps.querier);

        let curve_type = CurveType::Constant {
            value: Uint128::new(15),
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
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128::new(30_000_000));
        assert_eq!(get_balance(deps.as_ref(), carl), Uint128::new(0));

        // send coins to carl
        let bob_info = mock_info(bob, &[]);
        let transfer = ExecuteMsg::Transfer {
            recipient: carl.into(),
            amount: Uint128::new(2_000_000),
        };
        execute(deps.as_mut(), mock_env(), bob_info.clone(), transfer).unwrap();
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128::new(28_000_000));
        assert_eq!(get_balance(deps.as_ref(), carl), Uint128::new(2_000_000));

        // allow alice
        let allow = ExecuteMsg::IncreaseAllowance {
            spender: alice.into(),
            amount: Uint128::new(35_000_000),
            expires: None,
        };
        execute(deps.as_mut(), mock_env(), bob_info, allow).unwrap();
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128::new(28_000_000));
        assert_eq!(get_balance(deps.as_ref(), alice), Uint128::new(0));
        assert_eq!(
            query_allowance(deps.as_ref(), bob.into(), alice.into())
                .unwrap()
                .allowance,
            Uint128::new(35_000_000)
        );

        // alice takes some for herself
        let self_pay = ExecuteMsg::TransferFrom {
            owner: bob.into(),
            recipient: alice.into(),
            amount: Uint128::new(25_000_000),
        };
        let alice_info = mock_info(alice, &[]);
        execute(deps.as_mut(), mock_env(), alice_info, self_pay).unwrap();
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128::new(3_000_000));
        assert_eq!(get_balance(deps.as_ref(), alice), Uint128::new(25_000_000));
        assert_eq!(get_balance(deps.as_ref(), carl), Uint128::new(2_000_000));
        assert_eq!(
            query_allowance(deps.as_ref(), bob.into(), alice.into())
                .unwrap()
                .allowance,
            Uint128::new(10_000_000)
        );

        // test burn from works properly (burn tested in burning_sends_reserve)
        // cannot burn more than they have

        let info = mock_info(alice, &[]);
        let burn_from = ExecuteMsg::BurnFrom {
            owner: bob.into(),
            amount: Uint128::new(3_300_000),
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
            amount: Uint128::new(1_000_000),
        };
        let res = execute(deps.as_mut(), mock_env(), info, burn_from).unwrap();

        // bob balance is lower, not alice
        assert_eq!(get_balance(deps.as_ref(), alice), Uint128::new(25_000_000));
        assert_eq!(get_balance(deps.as_ref(), bob), Uint128::new(2_000_000));

        // ensure alice got our money back
        assert_eq!(1, res.messages.len());
        assert_eq!(
            &res.messages[0],
            &SubMsg::new(BankMsg::Send {
                to_address: alice.into(),
                amount: coins(1_500, DENOM),
            })
        );
    }
}
