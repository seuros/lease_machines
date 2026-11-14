//! Symbol names for the core's enums. Every match is exhaustive: a new
//! variant fails to compile here until it has a name.
use lease_machines::{Accepting, ClockPolicy, HolderState, Recording, Refreshing, Refusal};
use magnus::{Ruby, Symbol};
use matryona_machines::ruby::{Symbols, symbol};

matryona_machines::symbols! {
    /// The core's refusals as Symbols: the snake case of each variant.
    pub Refusals for Refusal as "a refusal" {
        Occupied => "occupied",
        Held => "held",
        NoLease => "no_lease",
        WrongOwner => "wrong_owner",
        StaleToken => "stale_token",
        Unknown => "unknown",
        Expired => "expired",
        ClockRegressed => "clock_regressed",
        InvalidTtl => "invalid_ttl",
        Exhausted => "exhausted",
        Fenced => "fenced",
        OutOfPhase => "out_of_phase",
    }
}

matryona_machines::symbols! {
    /// The holder's states as Symbols.
    pub HolderStates for HolderState as "a holder state" {
        Confirmed => "confirmed",
        Unconfirmed => "unconfirmed",
        AtRisk => "at_risk",
        Expired => "expired",
        Fenced => "fenced",
    }
}

matryona_machines::symbols! {
    /// The authority's clock policies as Symbols: `TypeError` for a
    /// non-Symbol, `RangeError` for another Symbol.
    pub ClockPolicies for ClockPolicy as "clock policy" {
        Reject => "reject",
        Clamp => "clamp",
    }
}

const fn accepting(outcome: Accepting) -> &'static str {
    match outcome {
        Accepting::Advanced => "advanced",
        Accepting::Current => "current",
    }
}

const fn recording(outcome: Recording) -> &'static str {
    match outcome {
        Recording::Recorded => "recorded",
        Recording::Duplicate => "duplicate",
    }
}

const fn refreshing(outcome: Refreshing) -> &'static str {
    match outcome {
        Refreshing::Started => "started",
        Refreshing::Outstanding => "outstanding",
    }
}

fn answer<T>(ruby: &Ruby, outcome: Result<T, Refusal>, name: fn(T) -> &'static str) -> Symbol {
    let name = match outcome {
        Ok(honoured) => name(honoured),
        Err(refused) => Refusals::name(refused),
    };
    symbol(ruby, name)
}

pub fn accepted(ruby: &Ruby, outcome: Result<Accepting, Refusal>) -> Symbol {
    answer(ruby, outcome, accepting)
}

pub fn recorded(ruby: &Ruby, outcome: Result<Recording, Refusal>) -> Symbol {
    answer(ruby, outcome, recording)
}

pub fn refreshed(ruby: &Ruby, outcome: Result<Refreshing, Refusal>) -> Symbol {
    answer(ruby, outcome, refreshing)
}
