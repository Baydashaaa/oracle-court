use cosmwasm_schema::{cw_serde, QueryResponses};

use crate::state::{Case, Choice, Config};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: Option<String>,
    pub prophecy: String,
    pub council: Vec<String>,
    pub quorum: u32,
    pub voting_secs: u64,
    pub vp_source: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Голос по спору. Первый голос сам открывает дело: отдельного шага
    /// "открыть" нет, чтобы спор нельзя было похоронить, просто не открыв.
    Vote { market_id: u64, choice: Choice },

    /// Закрыть дело после окончания голосования и отправить решение рынку.
    /// Может кто угодно. Нет кворума или ничья наверху - VOID, всем возврат.
    Close { market_id: u64 },

    /// Только админ. Меняет правила для БУДУЩИХ дел: открытые дела живут
    /// по правилам, скопированным в момент открытия.
    /// `vp_source: Some("")` снимает источник права голоса.
    UpdateConfig {
        admin: Option<String>,
        council: Option<Vec<String>>,
        quorum: Option<u32>,
        voting_secs: Option<u64>,
        vp_source: Option<String>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},

    #[returns(Option<Case>)]
    Case { market_id: u64 },

    #[returns(VoteResponse)]
    Vote { market_id: u64, address: String },

    /// Может ли адрес голосовать по этому спору прямо сейчас, и если нет -
    /// почему. Та же проверка, что при голосовании: интерфейс показывает
    /// причину до того, как человек потратит газ.
    #[returns(CanVoteResponse)]
    CanVote { market_id: u64, address: String },
}

#[cw_serde]
pub struct VoteResponse {
    pub choice: Option<Choice>,
}

#[cw_serde]
pub struct CanVoteResponse {
    pub can: bool,
    pub reason: Option<String>,
}

#[cw_serde]
pub struct MigrateMsg {}

// ── интерфейс источника права голоса ───────────────────────────────────────
//
// Его обязан реализовать будущий Oracle Vault. `at` - момент начала спора:
// засчитывается только позиция, внесённая раньше, поэтому купить право
// голоса после того, как спор открылся, нельзя.

#[cw_serde]
pub enum VpQueryMsg {
    Eligible { address: String, at: u64 },
}

#[cw_serde]
pub struct EligibleResponse {
    pub eligible: bool,
}
