#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
    Uint128, WasmMsg,
};
use cosmwasm_schema::cw_serde;
use serde::Deserialize;

use crate::error::ContractError;
use crate::msg::{
    CanVoteResponse, EligibleResponse, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg,
    VoteResponse, VpQueryMsg,
};
use crate::state::{Case, Choice, Config, CASES, CONFIG, VOTES};

const CONTRACT_NAME: &str = "crates.io:oracle-court";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ── что суд читает у рынка ──────────────────────────────────────────────────
//
// Свои копии типов, а не зависимость от крейта рынка: суду нужны три поля
// из десятков, и обычный Deserialize молча пропускает остальные. У типов с
// cw_serde стоит deny_unknown_fields, и любое новое поле в рынке ломало бы
// суд. Совпадение формата проверяют интеграционные тесты против настоящего
// контракта рынков.

#[derive(Deserialize)]
struct ProphecyMarket {
    status: String,
    disputed_at: Option<u64>,
    challenger: Option<String>,
}

#[derive(Deserialize)]
struct ProphecyConfig {
    resolver: String,
    arbiter_secs: u64,
}

#[derive(Deserialize)]
struct ProphecyPosition {
    yes: Uint128,
    no: Uint128,
}

#[cw_serde]
enum ProphecyQuery {
    Market { market_id: u64 },
    Config {},
    Position { market_id: u64, address: String },
}

#[cw_serde]
enum ProphecyExec {
    Rule {
        market_id: u64,
        outcome: Option<bool>,
        bad_spec: bool,
        ruling: String,
    },
}

// ── конфиг ──────────────────────────────────────────────────────────────────

fn check_config(cfg: &Config) -> Result<(), ContractError> {
    let bad = |what: &str| {
        Err(ContractError::BadConfig {
            what: what.to_string(),
        })
    };
    if cfg.council.is_empty() {
        return bad("council is empty");
    }
    let mut seen: Vec<&Addr> = vec![];
    for a in &cfg.council {
        if seen.contains(&a) {
            return bad("council has a duplicate member");
        }
        seen.push(a);
    }
    if cfg.quorum == 0 {
        return bad("quorum must be at least 1");
    }
    // Без внешнего источника голосовать может только совет. Кворум больше
    // совета означал бы, что каждый спор уходит в VOID.
    if cfg.vp_source.is_none() && cfg.quorum as usize > cfg.council.len() {
        return bad("quorum is larger than the council and there is no vp_source");
    }
    if cfg.voting_secs == 0 {
        return bad("voting_secs must be positive");
    }
    Ok(())
}

fn validate_all(deps: Deps, list: &[String]) -> StdResult<Vec<Addr>> {
    list.iter().map(|a| deps.api.addr_validate(a)).collect()
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let cfg = Config {
        admin: match msg.admin {
            Some(a) => deps.api.addr_validate(&a)?,
            None => info.sender,
        },
        prophecy: deps.api.addr_validate(&msg.prophecy)?,
        council: validate_all(deps.as_ref(), &msg.council)?,
        quorum: msg.quorum,
        voting_secs: msg.voting_secs,
        vp_source: msg
            .vp_source
            .map(|a| deps.api.addr_validate(&a))
            .transpose()?,
    };
    check_config(&cfg)?;
    CONFIG.save(deps.storage, &cfg)?;
    Ok(Response::new().add_attribute("action", "instantiate"))
}

// ── дело и право голоса ─────────────────────────────────────────────────────

/// Существующее дело или новое, собранное из текущего состояния рынка.
/// Новое сохраняется только вместе с первым голосом.
fn load_or_open(deps: Deps, cfg: &Config, market_id: u64) -> Result<Case, ContractError> {
    if let Some(c) = CASES.may_load(deps.storage, market_id)? {
        return Ok(c);
    }
    let m: ProphecyMarket = deps
        .querier
        .query_wasm_smart(&cfg.prophecy, &ProphecyQuery::Market { market_id })?;
    if m.status != "disputed" {
        return Err(ContractError::NotDisputed {});
    }
    let disputed_at = m.disputed_at.ok_or(ContractError::NotDisputed {})?;
    let pcfg: ProphecyConfig = deps
        .querier
        .query_wasm_smart(&cfg.prophecy, &ProphecyQuery::Config {})?;
    if cfg.voting_secs >= pcfg.arbiter_secs {
        return Err(ContractError::VotingTooLong {});
    }
    Ok(Case {
        market_id,
        disputed_at,
        ends_at: disputed_at + cfg.voting_secs,
        council: cfg.council.clone(),
        quorum: cfg.quorum,
        vp_source: cfg.vp_source.clone(),
        yes: 0,
        no: 0,
        void: 0,
        closed: false,
        result: None,
    })
}

/// Все проверки голосующего в одном месте: голосование и запрос CanVote
/// обязаны отвечать одинаково.
fn check_voter(deps: Deps, env: &Env, cfg: &Config, case: &Case, who: &Addr) -> Result<(), ContractError> {
    if case.closed {
        return Err(ContractError::CaseClosed {});
    }
    if env.block.time.seconds() >= case.ends_at {
        return Err(ContractError::VotingClosed {});
    }
    if VOTES.has(deps.storage, (case.market_id, who)) {
        return Err(ContractError::AlreadyVoted {});
    }

    // Рынок мог уже уйти из спора, например аннулироваться админом.
    let m: ProphecyMarket = deps.querier.query_wasm_smart(
        &cfg.prophecy,
        &ProphecyQuery::Market {
            market_id: case.market_id,
        },
    )?;
    if m.status != "disputed" {
        return Err(ContractError::NotDisputed {});
    }

    let conflict = |why: &str| {
        Err(ContractError::Conflict {
            why: why.to_string(),
        })
    };
    let pcfg: ProphecyConfig = deps
        .querier
        .query_wasm_smart(&cfg.prophecy, &ProphecyQuery::Config {})?;
    if who.as_str() == pcfg.resolver {
        return conflict("the resolver cannot judge its own reading");
    }
    if m.challenger.as_deref() == Some(who.as_str()) {
        return conflict("the challenger cannot judge its own challenge");
    }
    let pos: ProphecyPosition = deps.querier.query_wasm_smart(
        &cfg.prophecy,
        &ProphecyQuery::Position {
            market_id: case.market_id,
            address: who.to_string(),
        },
    )?;
    if !pos.yes.is_zero() || !pos.no.is_zero() {
        return conflict("the voter has a stake in this market");
    }

    // Совет - по составу, скопированному в дело, а не по текущему конфигу.
    if case.council.contains(who) {
        return Ok(());
    }
    if let Some(src) = &case.vp_source {
        let r: EligibleResponse = deps.querier.query_wasm_smart(
            src,
            &VpQueryMsg::Eligible {
                address: who.to_string(),
                at: case.disputed_at,
            },
        )?;
        if r.eligible {
            return Ok(());
        }
    }
    Err(ContractError::NotEligible {})
}

// ── исполнение ──────────────────────────────────────────────────────────────

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Vote { market_id, choice } => exec_vote(deps, env, info, market_id, choice),
        ExecuteMsg::Close { market_id } => exec_close(deps, env, market_id),
        ExecuteMsg::UpdateConfig {
            admin,
            council,
            quorum,
            voting_secs,
            vp_source,
        } => exec_update_config(deps, info, admin, council, quorum, voting_secs, vp_source),
    }
}

fn exec_vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    market_id: u64,
    choice: Choice,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut case = load_or_open(deps.as_ref(), &cfg, market_id)?;
    check_voter(deps.as_ref(), &env, &cfg, &case, &info.sender)?;

    match choice {
        Choice::Yes => case.yes += 1,
        Choice::No => case.no += 1,
        Choice::Void => case.void += 1,
    }
    VOTES.save(deps.storage, (market_id, &info.sender), &choice)?;
    CASES.save(deps.storage, market_id, &case)?;

    Ok(Response::new()
        .add_attribute("action", "court_vote")
        .add_attribute("market_id", market_id.to_string())
        .add_attribute("voter", info.sender)
        .add_attribute(
            "choice",
            match choice {
                Choice::Yes => "yes",
                Choice::No => "no",
                Choice::Void => "void",
            },
        ))
}

/// Итог голосования. Ничья наверху - VOID: суд не выбирает жребием, чьи
/// деньги уйдут кому.
fn tally(case: &Case) -> Choice {
    if case.yes + case.no + case.void < case.quorum {
        return Choice::Void;
    }
    let top = case.yes.max(case.no).max(case.void);
    let leaders = [case.yes, case.no, case.void]
        .iter()
        .filter(|&&n| n == top)
        .count();
    if leaders > 1 {
        return Choice::Void;
    }
    if case.yes == top {
        Choice::Yes
    } else if case.no == top {
        Choice::No
    } else {
        Choice::Void
    }
}

fn exec_close(deps: DepsMut, env: Env, market_id: u64) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut case = CASES
        .may_load(deps.storage, market_id)?
        .ok_or(ContractError::NoCase {})?;
    if case.closed {
        return Err(ContractError::CaseClosed {});
    }
    if env.block.time.seconds() < case.ends_at {
        return Err(ContractError::VotingOpen {});
    }

    let result = tally(&case);
    let outcome = match result {
        Choice::Yes => Some(true),
        Choice::No => Some(false),
        Choice::Void => None,
    };
    let quorum_met = case.yes + case.no + case.void >= case.quorum;
    let ruling = format!(
        "court: {} yes, {} no, {} void, quorum {}{}",
        case.yes,
        case.no,
        case.void,
        case.quorum,
        if quorum_met { "" } else { " not reached" }
    );

    case.closed = true;
    case.result = Some(result);
    CASES.save(deps.storage, market_id, &case)?;

    // Если рынок уже ушёл из спора, например аннулирован по таймауту,
    // сообщение откатится вместе со всей транзакцией, и дело останется
    // открытым - вреда в этом нет, решать там уже нечего.
    let msg = WasmMsg::Execute {
        contract_addr: cfg.prophecy.to_string(),
        msg: to_json_binary(&ProphecyExec::Rule {
            market_id,
            outcome,
            bad_spec: false,
            ruling: ruling.clone(),
        })?,
        funds: vec![],
    };

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "court_close")
        .add_attribute("market_id", market_id.to_string())
        .add_attribute("ruling", ruling))
}

fn exec_update_config(
    deps: DepsMut,
    info: MessageInfo,
    admin: Option<String>,
    council: Option<Vec<String>>,
    quorum: Option<u32>,
    voting_secs: Option<u64>,
    vp_source: Option<String>,
) -> Result<Response, ContractError> {
    let mut cfg = CONFIG.load(deps.storage)?;
    if info.sender != cfg.admin {
        return Err(ContractError::Unauthorized {});
    }
    if let Some(a) = admin {
        cfg.admin = deps.api.addr_validate(&a)?;
    }
    if let Some(c) = council {
        cfg.council = validate_all(deps.as_ref(), &c)?;
    }
    if let Some(q) = quorum {
        cfg.quorum = q;
    }
    if let Some(v) = voting_secs {
        cfg.voting_secs = v;
    }
    if let Some(v) = vp_source {
        cfg.vp_source = if v.is_empty() {
            None
        } else {
            Some(deps.api.addr_validate(&v)?)
        };
    }
    check_config(&cfg)?;
    CONFIG.save(deps.storage, &cfg)?;
    Ok(Response::new().add_attribute("action", "update_config"))
}

// ── запросы ─────────────────────────────────────────────────────────────────

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Case { market_id } => to_json_binary(&CASES.may_load(deps.storage, market_id)?),
        QueryMsg::Vote { market_id, address } => {
            let who = deps.api.addr_validate(&address)?;
            to_json_binary(&VoteResponse {
                choice: VOTES.may_load(deps.storage, (market_id, &who))?,
            })
        }
        QueryMsg::CanVote { market_id, address } => {
            let who = deps.api.addr_validate(&address)?;
            let cfg = CONFIG.load(deps.storage)?;
            let r = load_or_open(deps, &cfg, market_id)
                .and_then(|case| check_voter(deps, &env, &cfg, &case, &who));
            to_json_binary(&match r {
                Ok(()) => CanVoteResponse {
                    can: true,
                    reason: None,
                },
                Err(e) => CanVoteResponse {
                    can: false,
                    reason: Some(e.to_string()),
                },
            })
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("action", "migrate"))
}
