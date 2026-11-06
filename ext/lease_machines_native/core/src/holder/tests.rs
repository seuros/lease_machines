#![allow(clippy::expect_used)] // Failed test assumptions should panic.

use super::*;

fn at(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn token(value: u64) -> FencingToken {
    FencingToken::new(value).expect("test token is positive")
}

fn cadence() -> Cadence {
    Cadence::new(at(30), at(10), at(5)).expect("renewal fits inside the ttl")
}

#[test]
fn cadence_rejects_renewals_outside_the_ttl() {
    assert_eq!(
        Cadence::new(Duration::ZERO, Duration::ZERO, Duration::ZERO),
        None
    );
    assert_eq!(Cadence::new(at(30), at(30), at(5)), None);
    assert_eq!(Cadence::new(at(30), at(10), at(31)), None);
    assert_eq!(cadence().ttl(), at(30));
}

#[test]
fn renewal_is_due_at_the_fraction_and_risk_at_the_warning_deadline() {
    let holder = Holder::new(token(1), at(100), cadence());
    assert_eq!(holder.state(), HolderState::Confirmed);
    assert!(!holder.renewal_due(at(109)));
    assert!(holder.renewal_due(at(110)));
    assert_eq!(holder.warning_deadline(), Some(at(125)));
    assert!(!holder.at_risk(at(124)));
    assert!(holder.at_risk(at(125)));
}

#[test]
fn confirmation_reschedules_and_stale_responses_are_ignored() {
    let mut holder = Holder::new(token(2), at(0), cadence());
    assert!(holder.refresh(at(10)));
    assert_eq!(holder.state(), HolderState::Unconfirmed);
    assert!(holder.renewal_due(at(10)));
    assert!(holder.at_risk(at(10)));
    assert!(!holder.confirm(token(1), at(11)));
    assert_eq!(holder.state(), HolderState::Unconfirmed);
    assert!(holder.confirm(token(2), at(11)));
    assert_eq!(holder.state(), HolderState::Confirmed);
    assert_eq!(holder.warning_deadline(), Some(at(36)));
    assert!(!holder.confirm(token(2), at(12)));
    assert!(holder.refresh(at(21)));
    assert!(holder.confirm(token(3), at(22)));
    assert_eq!(holder.token(), token(3));
}

#[test]
fn outstanding_renewal_past_the_warning_deadline_is_at_risk() {
    let mut holder = Holder::new(token(1), at(0), cadence());
    assert!(holder.refresh(at(10)));
    assert!(holder.refresh(at(25)));
    assert_eq!(holder.state(), HolderState::AtRisk);
    let mut late = Holder::new(token(1), at(0), cadence());
    assert!(late.refresh(at(26)));
    assert_eq!(late.state(), HolderState::AtRisk);
    assert!(late.confirm(token(1), at(27)));
    assert_eq!(late.state(), HolderState::Confirmed);
}

#[test]
fn expiry_requires_a_newer_grant_and_fencing_is_terminal() {
    let mut holder = Holder::new(token(1), at(0), cadence());
    assert!(holder.expire());
    assert_eq!(holder.state(), HolderState::Expired);
    assert!(holder.refresh(at(1)));
    assert!(holder.renewal_due(at(1)));
    assert!(!holder.confirm(token(1), at(1)));
    assert!(!holder.reacquire(token(1), at(1)));
    assert!(holder.reacquire(token(2), at(2)));
    assert_eq!(holder.state(), HolderState::Confirmed);
    assert_eq!(holder.token(), token(2));
    assert_eq!(holder.warning_deadline(), Some(at(27)));
    holder.fence();
    holder.fence();
    assert_eq!(holder.state(), HolderState::Fenced);
    assert!(!holder.refresh(at(3)));
    assert!(!holder.expire());
    assert!(!holder.confirm(token(3), at(3)));
    assert!(!holder.reacquire(token(3), at(3)));
    assert!(!holder.renewal_due(at(3)));
    assert_eq!(holder.warning_deadline(), None);
    assert!(holder.at_risk(at(3)));
}

#[test]
fn deadlines_saturate_instead_of_overflowing() {
    let holder = Holder::new(token(1), Duration::MAX, cadence());
    assert_eq!(holder.warning_deadline(), Some(Duration::MAX));
    assert!(holder.renewal_due(Duration::MAX));
}
