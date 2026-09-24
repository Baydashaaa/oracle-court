// Суд против настоящего контракта рынков.
//
// Суд читает ответы oracle-prophecy своими копиями типов, поэтому тесты
// гоняются не на макете, а на настоящем контракте рынков: расхождение в
// формате должно ловиться здесь, а не на mainnet.
//
// Пример везде один: Alice 400 на YES, Bob 600 на NO, резолвер объявляет
// исход, Carol оспаривает, совет из трёх решает.

use cosmwasm_std::{
    coins, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
    Uint128,
};
use cw_multi_test::{App, AppBuilder, ContractWrapper, Executor};

use oracle_court::msg::{
    CanVoteResponse, EligibleResponse, ExecuteMsg as CourtExec, InstantiateMsg as CourtInit,
    QueryMsg as CourtQuery, VpQueryMsg,
};
use oracle_court::state::{Case, Choice};
use oracle_prophecy::msg::{
    ExecuteMsg as PExec, InstantiateMsg as PInit, QueryMsg as PQuery,
};
use oracle_prophecy::state::{Market, Spec, Status};

const DENOM: &str = "uluna";
const ADMIN: &str = "admin";
const RESOLVER: &str = "resolver";
const CREATOR: &str = "creator";
const ALICE: &str = "alice";
const BOB: &str = "bob";
const CAROL: &str = "carol";
const C1: &str = "council1";
const C2: &str = "council2";
const C3: &str = "council3";
const STRANGER: &str = "stranger";
const VAULT_USER: &str = "vaultuser";

const START: u128 = 1_000_000;
const BOND: u128 = 50;
const CH_BOND: u128 = 50;
const CUTOFF: u64 = 86_400;
const CHALLENGE: u64 = 3_600;
const ARBITER_SECS: u64 = 7_200;
const VOTING: u64 = 3_600;

// ── макет источника права голоса ────────────────────────────────────────────
//
// Будущий Oracle Vault. Здесь он признаёт только VAULT_USER и только если
// спросили про момент, не раньше отметки 1 - этого хватает, чтобы проверить,
// что суд передаёт время начала спора.

fn vp_instantiate(_: DepsMut, _: Env, _: MessageInfo, _: Binary) -> StdResult<Response> {
    Ok(Response::new())
}
fn vp_execute(_: DepsMut, _: Env, _: MessageInfo, _: Binary) -> StdResult<Response> {
    Ok(Response::new())
}
fn vp_query(_: Deps, _: Env, msg: VpQueryMsg) -> StdResult<Binary> {
    match msg {
        VpQueryMsg::Eligible { address, at } => to_json_binary(&EligibleResponse {
            eligible: address == VAULT_USER && at > 1,
        }),
    }
}

// ── обвязка ─────────────────────────────────────────────────────────────────

struct World {
    app: App,
    market: Addr,
    court: Addr,
}

fn world(council: &[&str], quorum: u32, voting: u64, with_vp: bool) -> World {
    let mut app = AppBuilder::new().build(|router, _, storage| {
        for who in [ADMIN, CREATOR, ALICE, BOB, CAROL, RESOLVER, C1, C2, C3, STRANGER, VAULT_USER] {
            router
                .bank
                .init_balance(storage, &Addr::unchecked(who), coins(START, DENOM))
                .unwrap();
        }
    });

    let pcode = app.store_code(Box::new(ContractWrapper::new(
        oracle_prophecy::contract::execute,
        oracle_prophecy::contract::instantiate,
        oracle_prophecy::contract::query,
    )));
    let ccode = app.store_code(Box::new(ContractWrapper::new(
        oracle_court::contract::execute,
        oracle_court::contract::instantiate,
        oracle_court::contract::query,
    )));

    // Рынку нужен адрес арбитра при создании, а суду - адрес рынка. Поэтому
    // рынок стартует с временным арбитром, и суд подставляется после.
    // Так же будет и на цепочке.
    let market = app
        .instantiate_contract(
            pcode,
            Addr::unchecked(ADMIN),
            &PInit {
                admin: Some(ADMIN.into()),
                resolver: RESOLVER.into(),
                draw_pool: "draw_pool".into(),
                treasury: "treasury".into(),
                denom: DENOM.into(),
                protocol_bps: 500,
                creator_bps: 300,
                boost_bps: 200,
                creation_bond: Uint128::new(BOND),
                promo_fee: Uint128::new(200),
                min_bet: Uint128::new(10),
                max_bet: Uint128::new(1_000),
                boost_amount: Uint128::new(100),
                boost_per_week: 2,
                challenge_secs: CHALLENGE,
                bet_cutoff_secs: CUTOFF,
                arbiter: Some(ADMIN.into()),
                challenge_bond: Uint128::new(CH_BOND),
                arbiter_secs: ARBITER_SECS,
                resolve_grace_secs: 604_800,
            },
            &[],
            "prophecy",
            None,
        )
        .unwrap();

    let vp = if with_vp {
        let vcode = app.store_code(Box::new(ContractWrapper::new(vp_execute, vp_instantiate, vp_query)));
        Some(
            app.instantiate_contract(vcode, Addr::unchecked(ADMIN), &Binary::default(), &[], "vault", None)
                .unwrap(),
        )
    } else {
        None
    };

    let court = app
        .instantiate_contract(
            ccode,
            Addr::unchecked(ADMIN),
            &CourtInit {
                admin: Some(ADMIN.into()),
                prophecy: market.to_string(),
                council: council.iter().map(|s| s.to_string()).collect(),
                quorum,
                voting_secs: voting,
                vp_source: vp.map(|a| a.to_string()),
            },
            &[],
            "court",
            None,
        )
        .unwrap();

    app.execute_contract(
        Addr::unchecked(ADMIN),
        market.clone(),
        &set_arbiter(court.as_str()),
        &[],
    )
    .unwrap();

    World { app, market, court }
}

fn set_arbiter(a: &str) -> PExec {
    PExec::UpdateConfig {
        admin: None,
        resolver: None,
        draw_pool: None,
        treasury: None,
        protocol_bps: None,
        creator_bps: None,
        boost_bps: None,
        creation_bond: None,
        promo_fee: None,
        min_bet: None,
        max_bet: None,
        boost_amount: None,
        boost_per_week: None,
        challenge_secs: None,
        bet_cutoff_secs: None,
        paused: None,
        arbiter: Some(a.into()),
        challenge_bond: None,
        arbiter_secs: None,
        resolve_grace_secs: None,
    }
}

fn default_world() -> World {
    world(&[C1, C2, C3], 2, VOTING, false)
}

fn advance(app: &mut App, secs: u64) {
    app.update_block(|b| {
        b.time = b.time.plus_seconds(secs);
        b.height += secs / 6;
    });
}

fn balance(app: &App, who: &str) -> u128 {
    app.wrap().query_balance(who, DENOM).unwrap().amount.u128()
}

fn run(app: &mut App, who: &str, to: &Addr, msg: &(impl serde::Serialize + std::fmt::Debug), funds: u128) -> anyhow::Result<()> {
    let f = if funds == 0 { vec![] } else { coins(funds, DENOM) };
    app.execute_contract(Addr::unchecked(who), to.clone(), msg, &f)
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!(e.root_cause().to_string()))
}

fn market(w: &World) -> Market {
    w.app
        .wrap()
        .query_wasm_smart(w.market.clone(), &PQuery::Market { market_id: 1 })
        .unwrap()
}

fn case(w: &World) -> Option<Case> {
    w.app
        .wrap()
        .query_wasm_smart(w.court.clone(), &CourtQuery::Case { market_id: 1 })
        .unwrap()
}

/// Рынок, две ставки, объявление, оспаривание: спор открыт.
fn disputed(w: &mut World, proposed: bool) {
    let now = w.app.block_info().time.seconds();
    let close = now + 1_000;
    let m = w.market.clone();
    run(
        &mut w.app,
        CREATOR,
        &m,
        &PExec::Create {
            question: "Will LUNC supply be below 6T?".into(),
            category: "economy".into(),
            spec: Spec {
                metric: Some("total_supply".into()),
                param: None,
                comparator: Some("lt".into()),
                threshold: Some("6000000000000000000".into()),
                height: Some(30_312_400),
                criterion: "bank supply of uluna at the given height".into(),
                unit: Some("uluna".into()),
            },
            bets_close_at: close,
            resolve_after: close + CUTOFF + 1,
            promoted: false,
        },
        BOND,
    )
    .unwrap();
    run(&mut w.app, ALICE, &m, &PExec::Predict { market_id: 1, side: true }, 400).unwrap();
    run(&mut w.app, BOB, &m, &PExec::Predict { market_id: 1, side: false }, 600).unwrap();
    advance(&mut w.app, 1_000 + CUTOFF + 2);
    run(
        &mut w.app,
        RESOLVER,
        &m,
        &PExec::Propose {
            market_id: 1,
            outcome: proposed,
            reading: "supply 6.45T LUNC at height 30312400".into(),
        },
        0,
    )
    .unwrap();
    run(
        &mut w.app,
        CAROL,
        &m,
        &PExec::Challenge {
            market_id: 1,
            reading: "wrong height".into(),
        },
        CH_BOND,
    )
    .unwrap();
}

fn vote(w: &mut World, who: &str, choice: Choice) -> anyhow::Result<()> {
    let c = w.court.clone();
    run(&mut w.app, who, &c, &CourtExec::Vote { market_id: 1, choice }, 0)
}

fn close(w: &mut World, who: &str) -> anyhow::Result<()> {
    let c = w.court.clone();
    run(&mut w.app, who, &c, &CourtExec::Close { market_id: 1 }, 0)
}

fn claim(w: &mut World, who: &str) -> anyhow::Result<()> {
    let m = w.market.clone();
    run(&mut w.app, who, &m, &PExec::Claim { market_id: 1 }, 0)
}

// ── решения ─────────────────────────────────────────────────────────────────

/// Резолвер объявил YES, совет двумя голосами из трёх решил NO. Исход
/// меняется, Carol получает залог и протокольную долю 20 из проигравших 400.
#[test]
fn the_council_majority_overturns_a_wrong_outcome() {
    let mut w = default_world();
    disputed(&mut w, true);

    vote(&mut w, C1, Choice::No).unwrap();
    vote(&mut w, C2, Choice::No).unwrap();
    vote(&mut w, C3, Choice::Yes).unwrap();
    advance(&mut w.app, VOTING);
    close(&mut w, STRANGER).unwrap();

    let m = market(&w);
    assert_eq!(m.status, Status::Settled);
    assert_eq!(m.outcome, Some(false));
    assert!(m.ruling.unwrap().contains("2 no"));
    assert_eq!(balance(&w.app, CAROL), START + 20);

    claim(&mut w, BOB).unwrap();
    assert_eq!(balance(&w.app, BOB), START + 360);
    assert_eq!(case(&w).unwrap().result, Some(Choice::No));
}

/// Совет подтвердил объявление: Carol теряет залог, исход прежний.
#[test]
fn the_council_confirms_a_correct_outcome() {
    let mut w = default_world();
    disputed(&mut w, false);

    vote(&mut w, C1, Choice::No).unwrap();
    vote(&mut w, C2, Choice::No).unwrap();
    advance(&mut w.app, VOTING);
    close(&mut w, STRANGER).unwrap();

    let m = market(&w);
    assert_eq!(m.status, Status::Settled);
    assert_eq!(m.outcome, Some(false));
    assert_eq!(balance(&w.app, CAROL), START - CH_BOND);
}

/// Меньше кворума - VOID, все получают ставки назад, Carol тоже.
#[test]
fn no_quorum_voids_and_refunds_everyone() {
    let mut w = default_world();
    disputed(&mut w, true);

    vote(&mut w, C1, Choice::No).unwrap();
    advance(&mut w.app, VOTING);
    close(&mut w, STRANGER).unwrap();

    let m = market(&w);
    assert_eq!(m.status, Status::Void);
    assert!(m.ruling.unwrap().contains("not reached"));
    assert_eq!(balance(&w.app, CAROL), START);
    claim(&mut w, ALICE).unwrap();
    claim(&mut w, BOB).unwrap();
    assert_eq!(balance(&w.app, ALICE), START);
    assert_eq!(balance(&w.app, BOB), START);
}

/// Ничья наверху - VOID. Суд не выбирает жребием, чьи деньги уйдут кому.
#[test]
fn a_tie_voids() {
    let mut w = default_world();
    disputed(&mut w, true);

    vote(&mut w, C1, Choice::Yes).unwrap();
    vote(&mut w, C2, Choice::No).unwrap();
    advance(&mut w.app, VOTING);
    close(&mut w, STRANGER).unwrap();

    assert_eq!(market(&w).status, Status::Void);
    assert_eq!(case(&w).unwrap().result, Some(Choice::Void));
}

// ── кто не голосует ─────────────────────────────────────────────────────────

#[test]
fn a_stranger_cannot_vote_without_a_vp_source() {
    let mut w = default_world();
    disputed(&mut w, true);
    let e = vote(&mut w, STRANGER, Choice::No).unwrap_err().to_string();
    assert!(e.contains("Not eligible"), "{e}");
}

/// Член совета со ставкой в рынке не голосует. В мультисиге это было бы
/// договорённостью, здесь это проверяет код.
#[test]
fn a_council_member_with_a_stake_cannot_vote() {
    let mut w = default_world();
    let now = w.app.block_info().time.seconds();
    let close_at = now + 1_000;
    let m = w.market.clone();
    run(
        &mut w.app,
        CREATOR,
        &m,
        &PExec::Create {
            question: "q".into(),
            category: "economy".into(),
            spec: Spec {
                metric: Some("total_supply".into()),
                param: None,
                comparator: Some("lt".into()),
                threshold: Some("1".into()),
                height: Some(1),
                criterion: "c".into(),
                unit: None,
            },
            bets_close_at: close_at,
            resolve_after: close_at + CUTOFF + 1,
            promoted: false,
        },
        BOND,
    )
    .unwrap();
    run(&mut w.app, C1, &m, &PExec::Predict { market_id: 1, side: true }, 400).unwrap();
    run(&mut w.app, BOB, &m, &PExec::Predict { market_id: 1, side: false }, 600).unwrap();
    advance(&mut w.app, 1_000 + CUTOFF + 2);
    run(&mut w.app, RESOLVER, &m, &PExec::Propose { market_id: 1, outcome: true, reading: "r".into() }, 0).unwrap();
    run(&mut w.app, CAROL, &m, &PExec::Challenge { market_id: 1, reading: "x".into() }, CH_BOND).unwrap();

    let e = vote(&mut w, C1, Choice::Yes).unwrap_err().to_string();
    assert!(e.contains("stake in this market"), "{e}");
}

#[test]
fn the_resolver_and_the_challenger_cannot_vote_even_on_the_council() {
    let mut w = world(&[RESOLVER, CAROL, C1], 1, VOTING, false);
    disputed(&mut w, true);

    let e = vote(&mut w, RESOLVER, Choice::Yes).unwrap_err().to_string();
    assert!(e.contains("resolver cannot judge"), "{e}");
    let e = vote(&mut w, CAROL, Choice::No).unwrap_err().to_string();
    assert!(e.contains("challenger cannot judge"), "{e}");
}

#[test]
fn nobody_votes_twice() {
    let mut w = default_world();
    disputed(&mut w, true);
    vote(&mut w, C1, Choice::No).unwrap();
    let e = vote(&mut w, C1, Choice::Yes).unwrap_err().to_string();
    assert!(e.contains("Already voted"), "{e}");
}

// ── время ───────────────────────────────────────────────────────────────────

#[test]
fn votes_close_on_time_and_closing_waits_for_the_end() {
    let mut w = default_world();
    disputed(&mut w, true);
    vote(&mut w, C1, Choice::No).unwrap();

    let e = close(&mut w, STRANGER).unwrap_err().to_string();
    assert!(e.contains("still open"), "{e}");

    advance(&mut w.app, VOTING);
    let e = vote(&mut w, C2, Choice::No).unwrap_err().to_string();
    assert!(e.contains("Voting has ended"), "{e}");
}

/// Голосование, которое длится дольше окна арбитра, никогда бы не
/// закрылось - рынок аннулировал бы спор раньше. Суд отказывается его
/// открывать.
#[test]
fn voting_longer_than_the_arbiter_window_is_refused() {
    let mut w = world(&[C1, C2, C3], 2, ARBITER_SECS, false);
    disputed(&mut w, true);
    let e = vote(&mut w, C1, Choice::No).unwrap_err().to_string();
    assert!(e.contains("before the market's arbiter window"), "{e}");
}

#[test]
fn there_is_nothing_to_vote_on_without_a_dispute() {
    let mut w = default_world();
    let e = vote(&mut w, C1, Choice::No).unwrap_err().to_string();
    // Рынка №1 ещё нет - запрос к рынку падает; спора нет в любом случае.
    assert!(!e.is_empty());
    assert!(case(&w).is_none());
}

// ── заморозка правил дела ───────────────────────────────────────────────────

/// Смена совета посреди спора не действует на уже открытое дело. Иначе
/// админ мог бы добавить нужных голосующих и повлиять на исход.
#[test]
fn a_council_change_does_not_reach_an_open_case() {
    let mut w = default_world();
    disputed(&mut w, true);
    vote(&mut w, C1, Choice::No).unwrap();

    let c = w.court.clone();
    run(
        &mut w.app,
        ADMIN,
        &c,
        &CourtExec::UpdateConfig {
            admin: None,
            council: Some(vec![STRANGER.into(), C3.into()]),
            quorum: None,
            voting_secs: None,
            vp_source: None,
        },
        0,
    )
    .unwrap();

    // Новый член не голосует по старому делу, старый - голосует.
    let e = vote(&mut w, STRANGER, Choice::Yes).unwrap_err().to_string();
    assert!(e.contains("Not eligible"), "{e}");
    vote(&mut w, C2, Choice::No).unwrap();
}

// ── источник права голоса ───────────────────────────────────────────────────

/// Пользователь вне совета голосует, если источник права голоса его
/// признаёт. Так подключится Oracle Vault.
#[test]
fn a_vp_source_lets_users_vote() {
    let mut w = world(&[C1, C2, C3], 2, VOTING, true);
    disputed(&mut w, true);

    let r: CanVoteResponse = w
        .app
        .wrap()
        .query_wasm_smart(
            w.court.clone(),
            &CourtQuery::CanVote {
                market_id: 1,
                address: VAULT_USER.into(),
            },
        )
        .unwrap();
    assert!(r.can, "{:?}", r.reason);

    vote(&mut w, VAULT_USER, Choice::No).unwrap();
    let e = vote(&mut w, STRANGER, Choice::No).unwrap_err().to_string();
    assert!(e.contains("Not eligible"), "{e}");
}

#[test]
fn can_vote_explains_the_refusal() {
    let mut w = default_world();
    disputed(&mut w, true);
    let r: CanVoteResponse = w
        .app
        .wrap()
        .query_wasm_smart(
            w.court.clone(),
            &CourtQuery::CanVote {
                market_id: 1,
                address: CAROL.into(),
            },
        )
        .unwrap();
    assert!(!r.can);
    assert!(r.reason.unwrap().contains("challenger"));
}

// ── конфиг ──────────────────────────────────────────────────────────────────

#[test]
fn the_config_cannot_make_every_dispute_void() {
    let mut w = default_world();
    let c = w.court.clone();
    for (council, quorum) in [
        (Some(vec![C1.to_string(), C1.to_string()]), None),
        (None, Some(4u32)),
        (None, Some(0u32)),
    ] {
        let e = run(
            &mut w.app,
            ADMIN,
            &c,
            &CourtExec::UpdateConfig {
                admin: None,
                council,
                quorum,
                voting_secs: None,
                vp_source: None,
            },
            0,
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("Bad config"), "{e}");
    }
    let e = run(
        &mut w.app,
        STRANGER,
        &c,
        &CourtExec::UpdateConfig {
            admin: None,
            council: None,
            quorum: Some(1),
            voting_secs: None,
            vp_source: None,
        },
        0,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("Unauthorized"), "{e}");
}
