use cosmwasm_std::{
    coin, to_binary, Addr, BankMsg, Decimal, Deps, DepsMut, DistributionMsg, Env, MessageInfo,
    QuerierWrapper, Response, StakingMsg, StdError, StdResult, Uint128, WasmMsg,
};
use cw20_bonding::msg::CurveFn;

use crate::bonding::{execute_burn, execute_mint};
use crate::error::ContractError;
use crate::msg::ExecuteMsg;
use crate::query::InvestmentResponse;
use crate::state::{CurveState, CLAIMS, CURVE_STATE, INVESTMENT};

const FALLBACK_RATIO: Decimal = Decimal::one();

// get_bonded returns the total amount of delegations from contract
// it ensures they are all the same denom
fn get_bonded(querier: &QuerierWrapper, contract: &Addr) -> Result<Uint128, ContractError> {
    let bonds = querier.query_all_delegations(contract)?;
    if bonds.is_empty() {
        return Ok(Uint128::zero());
    }
    let denom = bonds[0].amount.denom.as_str();
    bonds.iter().fold(Ok(Uint128::zero()), |racc, d| {
        let acc = racc?;
        if d.amount.denom.as_str() != denom {
            Err(ContractError::DifferentBondDenom {
                denom1: denom.into(),
                denom2: d.amount.denom.to_string(),
            })
        } else {
            Ok(acc + d.amount.amount)
        }
    })
}

fn assert_bonds(curve_state: &CurveState, bonded: Uint128) -> Result<(), ContractError> {
    if curve_state.reserve != bonded {
        Err(ContractError::BondedMismatch {
            stored: curve_state.reserve,
            queried: bonded,
        })
    } else {
        Ok(())
    }
}

pub fn bond(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    curve_fn: CurveFn,
) -> Result<Response, ContractError> {
    // ensure we have the proper denom
    let invest = INVESTMENT.load(deps.storage)?;
    // payment finds the proper coin (or throws an error)
    let payment = info
        .funds
        .iter()
        .find(|x| x.denom == invest.bond_denom)
        .ok_or_else(|| ContractError::EmptyBalance {
            denom: invest.bond_denom.clone(),
        })?;

    // bonded is the total number of tokens we have delegated from this address
    let bonded = get_bonded(&deps.querier, &env.contract.address)?;

    // calculate to_mint and update total supply
    let mut curve_state = CURVE_STATE.load(deps.storage)?;

    // TODO: this is just a safety assertion - do we keep it, or remove caching?
    // in the end supply is just there to cache the (expected) results of get_bonded() so we don't
    // have expensive queries everywhere
    assert_bonds(&curve_state, bonded)?;

    // this logic should be the same as execute buy
    // let to_mint = if curve_state.supply.is_zero() || bonded.is_zero() {
    //     FALLBACK_RATIO * payment.amount
    // } else {
    //     payment.amount.multiply_ratio(curve_state.supply, bonded)
    // };
    // curve_state.reserve = bonded + payment.amount;
    // curve_state.supply += to_mint;
    // CURVE_STATE.save(deps.storage, &curve_state)?;
    // end of the bit that needs changing

    // let payment: Uint128 = must_pay(&info, &state.reserve_denom)?;

    let curve = curve_fn(curve_state.decimals);
    curve_state.reserve += payment.amount;

    // curve.supply() calculates native -> CW20
    let new_supply = curve.supply(curve_state.reserve);
    let minted = new_supply
        .checked_sub(curve_state.supply)
        .map_err(StdError::overflow)?;
    curve_state.supply = new_supply;
    CURVE_STATE.save(deps.storage, &curve_state)?;

    // call into cw20-base to mint the token, call as self as no one else is allowed
    let sub_info = MessageInfo {
        sender: env.contract.address.clone(),
        funds: vec![],
    };
    execute_mint(deps, env, sub_info, info.sender.to_string(), minted)?;

    // bond them to the validator
    let res = Response::new()
        .add_message(StakingMsg::Delegate {
            validator: invest.validator,
            amount: payment.clone(),
        })
        .add_attribute("action", "bond")
        .add_attribute("from", info.sender)
        .add_attribute("bonded", payment.amount)
        .add_attribute("minted", minted);
    Ok(res)
}

pub fn unbond(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    curve_fn: CurveFn,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let invest = INVESTMENT.load(deps.storage)?;
    // ensure it is big enough to care
    if amount < invest.min_withdrawal {
        return Err(ContractError::UnbondTooSmall {
            min_bonded: invest.min_withdrawal,
            denom: invest.bond_denom,
        });
    }
    // calculate tax and remainer to unbond
    let tax = amount * invest.exit_tax;

    // burn from the original caller
    execute_burn(deps.branch(), env.clone(), info.clone(), amount)?;
    if tax > Uint128::zero() {
        let sub_info = MessageInfo {
            sender: env.contract.address.clone(),
            funds: vec![],
        };
        // call into cw20-base to mint tokens to owner, call as self as no one else is allowed
        execute_mint(
            deps.branch(),
            env.clone(),
            sub_info,
            invest.owner.to_string(),
            tax,
        )?;
    }

    // re-calculate bonded to ensure we have real values
    // bonded is the total number of tokens we have delegated from this address
    let bonded = get_bonded(&deps.querier, &env.contract.address)?;

    // calculate how many native tokens this is worth from curve
    // to do this, first we load curve state
    let mut curve_state = CURVE_STATE.load(deps.storage)?;

    // TODO: this is just a safety assertion - do we keep it, or remove caching?
    // in the end supply is just there to cache the (expected) results of get_bonded() so we don't
    // have expensive queries everywhere
    assert_bonds(&curve_state, bonded)?;

    // unbond the amount minus tax
    let amount_minus_tax = amount.checked_sub(tax).map_err(StdError::overflow)?;
    let curve = curve_fn(curve_state.decimals);
    curve_state.supply = curve_state
        .supply
        .checked_sub(amount_minus_tax)
        .map_err(StdError::overflow)?;

    // curve.reserve() calculates CW20 -> native
    // we've just updated total supply of CW20
    // so we use that to calc the total reserve
    // unbond is old reserve minus new reserve
    // giving the amount of native tokens being unbonded
    let new_reserve = curve.reserve(curve_state.supply);
    let unbond = curve_state
        .reserve
        .checked_sub(new_reserve)
        .map_err(StdError::overflow)?;
    curve_state.reserve = new_reserve;
    curve_state.claims += unbond;
    CURVE_STATE.save(deps.storage, &curve_state)?;

    CLAIMS.create_claim(
        deps.storage,
        &info.sender,
        unbond,
        invest.unbonding_period.after(&env.block),
    )?;

    // unbond them
    let res = Response::new()
        .add_message(StakingMsg::Undelegate {
            validator: invest.validator,
            amount: coin(unbond.u128(), &invest.bond_denom),
        })
        .add_attribute("action", "unbond")
        .add_attribute("to", info.sender)
        .add_attribute("unbonded", unbond)
        .add_attribute("burnt", amount);
    Ok(res)
}

pub fn claim(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    // find how many tokens the contract has
    let invest = INVESTMENT.load(deps.storage)?;
    let mut balance = deps
        .querier
        .query_balance(&env.contract.address, &invest.bond_denom)?;
    if balance.amount < invest.min_withdrawal {
        return Err(ContractError::BalanceTooSmall {});
    }

    // check how much to send - min(balance, claims[sender]), and reduce the claim
    // Ensure we have enough balance to cover this and only send some claims if that is all we can cover
    let to_send =
        CLAIMS.claim_tokens(deps.storage, &info.sender, &env.block, Some(balance.amount))?;
    if to_send == Uint128::zero() {
        return Err(ContractError::NothingToClaim {});
    }

    // update total supply (lower claim)
    CURVE_STATE.update(deps.storage, |mut curve_state| -> StdResult<_> {
        curve_state.claims = curve_state.claims.checked_sub(to_send)?;
        Ok(curve_state)
    })?;

    // transfer tokens to the sender
    balance.amount = to_send;
    let res = Response::new()
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![balance],
        })
        .add_attribute("action", "claim")
        .add_attribute("from", info.sender)
        .add_attribute("amount", to_send);
    Ok(res)
}

/// reinvest will withdraw all pending rewards,
/// then issue a callback to itself via _bond_all_tokens
/// to reinvest the new earnings (and anything else that accumulated)
pub fn reinvest(deps: DepsMut, env: Env, _info: MessageInfo) -> Result<Response, ContractError> {
    let contract_addr = env.contract.address;
    let invest = INVESTMENT.load(deps.storage)?;
    let msg = to_binary(&ExecuteMsg::_BondAllTokens {})?;

    // and bond them to the validator
    let res = Response::new()
        .add_message(DistributionMsg::WithdrawDelegatorReward {
            validator: invest.validator,
        })
        .add_message(WasmMsg::Execute {
            contract_addr: contract_addr.to_string(),
            msg,
            funds: vec![],
        });
    Ok(res)
}

pub fn _bond_all_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // this is just meant as a call-back to ourself
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // find how many tokens we have to bond
    let invest = INVESTMENT.load(deps.storage)?;
    let mut balance = deps
        .querier
        .query_balance(&env.contract.address, &invest.bond_denom)?;

    // we deduct pending claims from our account balance before reinvesting.
    // if there is not enough funds, we just return a no-op
    match CURVE_STATE.update(deps.storage, |mut curve_state| -> StdResult<_> {
        balance.amount = balance.amount.checked_sub(curve_state.claims)?;
        // this just triggers the "no op" case if we don't have min_withdrawal left to reinvest
        balance.amount.checked_sub(invest.min_withdrawal)?;

        // TODO: think about this some more.
        // need coffee and a full night of sleep cos moderately certain
        // that this ain't right like
        curve_state.reserve += balance.amount;
        Ok(curve_state)
    }) {
        Ok(_) => {}
        // if it is below the minimum, we do a no-op (do not revert other state from withdrawal)
        Err(StdError::Overflow { .. }) => return Ok(Response::default()),
        Err(e) => return Err(ContractError::Std(e)),
    }

    // and bond them to the validator
    let res = Response::new()
        .add_message(StakingMsg::Delegate {
            validator: invest.validator,
            amount: balance.clone(),
        })
        .add_attribute("action", "reinvest")
        .add_attribute("bonded", balance.amount);
    Ok(res)
}

pub fn query_investment(deps: Deps) -> StdResult<InvestmentResponse> {
    let invest = INVESTMENT.load(deps.storage)?;
    let curve_state = CURVE_STATE.load(deps.storage)?;

    let res = InvestmentResponse {
        owner: invest.owner.to_string(),
        exit_tax: invest.exit_tax,
        validator: invest.validator,
        min_withdrawal: invest.min_withdrawal,
        token_supply: curve_state.supply,
        staked_tokens: coin(curve_state.reserve.u128(), &invest.bond_denom),
        nominal_value: if curve_state.supply.is_zero() {
            FALLBACK_RATIO
        } else {
            Decimal::from_ratio(curve_state.reserve, curve_state.supply)
        },
    };
    Ok(res)
}
