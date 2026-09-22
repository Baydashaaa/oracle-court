use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Market is not under dispute")]
    NotDisputed {},

    #[error("No case for this market")]
    NoCase {},

    #[error("Case is already closed")]
    CaseClosed {},

    #[error("Voting has ended")]
    VotingClosed {},

    #[error("Voting is still open")]
    VotingOpen {},

    #[error("Already voted")]
    AlreadyVoted {},

    /// Резолвер не судит собственное показание, оспоривший - собственный
    /// спор, а тот, у кого ставка в рынке, - исход, от которого зависят его
    /// деньги. В мультисиге это было устной договорённостью, здесь это код.
    #[error("Cannot vote: {why}")]
    Conflict { why: String },

    #[error("Not eligible to vote")]
    NotEligible {},

    /// Голосование, которое длится дольше окна арбитра, никогда бы не
    /// закрылось: рынок аннулировал бы спор раньше.
    #[error("Voting must end before the market's arbiter window closes")]
    VotingTooLong {},

    #[error("Bad config: {what}")]
    BadConfig { what: String },
}
