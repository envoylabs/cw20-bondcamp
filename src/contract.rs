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
use crate::query::TokenInfoResponseWithMeta;
use crate::state::{
    CurveState, InvestmentInfo, Supply, TokenInfoWithMeta, CURVE_STATE, CURVE_TYPE, INVESTMENT,
    TOKEN_INFO_WITH_META, TOTAL_SUPPLY,
};
use cw0::nonpayable;
use cw20::TokenInfoResponse;
use cw20_bonding::contract::query_curve_info;
use cw20_bonding::curves::DecimalPlaces;
use cw20_bonding::msg::CurveFn;

use crate::bonding::{execute_buy, execute_sell, execute_sell_from};

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
    let supply = Supply::default();
    TOTAL_SUPPLY.save(deps.storage, &supply)?;

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
        // ExecuteMsg::Bond {} => bond(deps, env, info),
        // ExecuteMsg::Unbond { amount } => unbond(deps, env, info, amount),
        // ExecuteMsg::Claim {} => claim(deps, env, info),
        // ExecuteMsg::Reinvest {} => reinvest(deps, env, info),
        // ExecuteMsg::_BondAllTokens {} => _bond_all_tokens(deps, env, info),

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
        // QueryMsg::Claims { address } => {
        //     to_binary(&CLAIMS.query_claims(deps, &deps.api.addr_validate(&address)?)?)
        // }
        // QueryMsg::Investment {} => to_binary(&query_investment(deps)?),
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

// this is poor mans "skip" flag
#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::StakingParams;

    use cw20_bonding::msg::CurveType;

    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockQuerier};
    use cosmwasm_std::{
        coin, coins, BankMsg, Decimal, OverflowError, OverflowOperation, StdError, SubMsg,
        Validator,
    };
    use cw0::{PaymentError, DAY};

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
    fn bonding_fails_with_wrong_denom() {
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
