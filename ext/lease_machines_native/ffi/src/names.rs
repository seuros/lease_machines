//! Symbol names for the core's enums. Every match is exhaustive: a new
//! variant fails to compile here until it has a name.
use lease_machines::{Accepting, ClockPolicy, HolderState, Recording, Refreshing, Refusal};
use magnus::{Error, Ruby, Symbol, TryConvert, Value};

pub const REFUSALS: [&str; 12] = [
    "occupied",
    "held",
    "no_lease",
    "wrong_owner",
    "stale_token",
    "unknown",
    "expired",
    "clock_regressed",
    "invalid_ttl",
    "exhausted",
    "fenced",
    "out_of_phase",
];

pub const HOLDER_STATES: [&str; 5] = ["confirmed", "unconfirmed", "at_risk", "expired", "fenced"];

pub const CLOCK_POLICIES: [&str; 2] = ["reject", "clamp"];

pub const fn refusal(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Occupied => "occupied",
        Refusal::Held => "held",
        Refusal::NoLease => "no_lease",
        Refusal::WrongOwner => "wrong_owner",
        Refusal::StaleToken => "stale_token",
        Refusal::Unknown => "unknown",
        Refusal::Expired => "expired",
        Refusal::ClockRegressed => "clock_regressed",
        Refusal::InvalidTtl => "invalid_ttl",
        Refusal::Exhausted => "exhausted",
        Refusal::Fenced => "fenced",
        Refusal::OutOfPhase => "out_of_phase",
    }
}

pub const fn holder_state(state: HolderState) -> &'static str {
    match state {
        HolderState::Confirmed => "confirmed",
        HolderState::Unconfirmed => "unconfirmed",
        HolderState::AtRisk => "at_risk",
        HolderState::Expired => "expired",
        HolderState::Fenced => "fenced",
    }
}

pub const fn clock_policy(policy: ClockPolicy) -> &'static str {
    match policy {
        ClockPolicy::Reject => "reject",
        ClockPolicy::Clamp => "clamp",
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
    ruby.to_symbol(match outcome {
        Ok(honoured) => name(honoured),
        Err(refused) => refusal(refused),
    })
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

/// A clock policy named by its symbol.
#[derive(Clone, Copy)]
pub struct Policy(pub ClockPolicy);

impl TryConvert for Policy {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let ruby = Ruby::get_with(value);
        let symbol = Symbol::from_value(value).ok_or_else(|| {
            Error::new(ruby.exception_type_error(), "clock policy must be a Symbol")
        })?;
        match symbol.name()?.as_ref() {
            "reject" => Ok(Self(ClockPolicy::Reject)),
            "clamp" => Ok(Self(ClockPolicy::Clamp)),
            _ => Err(Error::new(
                ruby.exception_arg_error(),
                "clock policy must be :reject or :clamp",
            )),
        }
    }
}
