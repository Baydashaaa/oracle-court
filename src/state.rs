use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    pub admin: Addr,
    /// Контракт рынков. Неизменяем: суд судит только его споры, и подмена
    /// адреса ничего бы не дала - рынок принимает решение лишь от того
    /// арбитра, что записан в его собственном конфиге.
    pub prophecy: Addr,
    /// Совет голосует всегда и без условий. На старте, пока пользователей
    /// мало, он и есть кворум.
    pub council: Vec<Addr>,
    /// Сколько голосов нужно, чтобы решение состоялось. Меньше - VOID.
    pub quorum: u32,
    /// Длительность голосования. Обязана быть меньше `arbiter_secs` рынка,
    /// иначе рынок аннулирует спор раньше, чем суд успеет его закрыть.
    pub voting_secs: u64,
    /// Источник права голоса для пользователей, будущий Oracle Vault.
    /// Пока не задан, голосует только совет.
    pub vp_source: Option<Addr>,
}

#[cw_serde]
pub enum Choice {
    Yes,
    No,
    Void,
}

/// Дело по одному спору. Правила - совет, кворум, источник права голоса -
/// копируются в дело в момент открытия и дальше не меняются. Иначе сменой
/// конфига посреди спора можно было бы добавить нужных голосующих или
/// поднять кворум и тем повлиять на исход.
#[cw_serde]
pub struct Case {
    pub market_id: u64,
    pub disputed_at: u64,
    pub ends_at: u64,
    pub council: Vec<Addr>,
    pub quorum: u32,
    pub vp_source: Option<Addr>,
    pub yes: u32,
    pub no: u32,
    pub void: u32,
    pub closed: bool,
    pub result: Option<Choice>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const CASES: Map<u64, Case> = Map::new("cases");
pub const VOTES: Map<(u64, &Addr), Choice> = Map::new("votes");
