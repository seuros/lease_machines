# frozen_string_literal: true
require 'test_helper'

# The holder's renewal tracker: every report in every state.
class HolderTest < Minitest::Test
  include LeaseHelpers

  STATES = %i[confirmed unconfirmed at_risk expired fenced].freeze

  def cadence = L::Cadence.new(at(30), at(10), at(5))

  # A holder of token 5 driven into `state`.
  def holder_in(state)
    holder = L::Holder.new(token(SCOPE, 5), 0, cadence)
    case state
    when :unconfirmed then holder.refresh(at(10))
    when :at_risk then holder.refresh(at(25))
    when :expired then holder.expire
    when :fenced then holder.fence
    end
    assert_equal state, holder.state
    holder
  end

  def check(state, expected)
    holder = holder_in(state)
    before = snapshot(holder)
    assert_equal expected, yield(holder), "in #{state}"
    if expected == :recorded
      refute_equal before, snapshot(holder)
    else
      assert_equal before, snapshot(holder), "#{expected} in #{state} changed something"
    end
  end

  def test_renewal_is_due_at_the_fraction_and_risk_at_the_warning_deadline
    holder = L::Holder.new(token(SCOPE, 1), at(100), cadence)
    assert_equal [:confirmed, token(SCOPE, 1), cadence], [holder.state, holder.token, holder.cadence]
    assert_equal [false, true], [holder.renewal_due?(at(109)), holder.renewal_due?(at(110))]
    assert_equal at(125), holder.warning_deadline
    assert_equal [false, true], [holder.at_risk?(at(124)), holder.at_risk?(at(125))]
    assert_equal STATES, L::HOLDER_STATES
  end

  def test_refresh_starts_one_attempt
    holder = holder_in(:confirmed)
    assert_equal :started, holder.refresh(at(10))
    assert_repeat(:outstanding, holder) { holder.refresh(at(11)) }
    assert_equal :outstanding, holder.refresh(at(25))
    assert_equal :at_risk, holder.state
    expired = holder_in(:expired)
    assert_repeat(:outstanding, expired) { expired.refresh(at(1)) }
    fenced = holder_in(:fenced)
    assert_refused(:fenced, fenced) { fenced.refresh(at(1)) }
  end

  def test_confirm_in_every_state_for_every_kind_of_token
    older, same, newer, foreign = token(SCOPE, 4), token(SCOPE, 5), token(SCOPE, 6), token(OTHER, 6)
    %i[unconfirmed at_risk].each do |state|
      check(state, :recorded) { |h| h.confirm(same, at(27)) }
      check(state, :recorded) { |h| h.confirm(newer, at(27)) }
    end
    check(:confirmed, :duplicate) { |h| h.confirm(same, at(27)) }
    check(:confirmed, :out_of_phase) { |h| h.confirm(newer, at(27)) }
    check(:expired, :expired) { |h| h.confirm(same, at(27)) }
    (STATES - [:fenced]).each do |state|
      check(state, :stale_token) { |h| h.confirm(older, at(27)) }
      check(state, :unknown) { |h| h.confirm(foreign, at(27)) }
    end
    [older, same, newer, foreign].each { |t| check(:fenced, :fenced) { |h| h.confirm(t, at(27)) } }
  end

  def test_reacquire_in_every_state_for_every_kind_of_token
    older, same, newer, foreign = token(SCOPE, 4), token(SCOPE, 5), token(SCOPE, 6), token(OTHER, 6)
    check(:expired, :recorded) { |h| h.reacquire(newer, at(40)) }
    check(:expired, :stale_token) { |h| h.reacquire(same, at(40)) }
    check(:confirmed, :duplicate) { |h| h.reacquire(same, at(40)) }
    check(:confirmed, :out_of_phase) { |h| h.reacquire(newer, at(40)) }
    %i[unconfirmed at_risk].each do |state|
      check(state, :out_of_phase) { |h| h.reacquire(same, at(40)) }
      check(state, :out_of_phase) { |h| h.reacquire(newer, at(40)) }
    end
    (STATES - [:fenced]).each do |state|
      check(state, :stale_token) { |h| h.reacquire(older, at(40)) }
      check(state, :unknown) { |h| h.reacquire(foreign, at(40)) }
    end
    [older, same, newer, foreign].each { |t| check(:fenced, :fenced) { |h| h.reacquire(t, at(40)) } }
  end

  def test_expire_and_fence_in_every_state
    %i[confirmed unconfirmed at_risk].each { |state| check(state, :recorded, &:expire) }
    check(:expired, :duplicate, &:expire)
    check(:fenced, :fenced, &:expire)
    STATES.each do |state|
      holder = holder_in(state)
      assert_nil holder.fence
      assert_equal [:fenced, nil, false, true],
                   [holder.state, holder.warning_deadline, holder.renewal_due?(at(3)), holder.at_risk?(at(3))]
      assert_nil holder.fence
    end
  end

  def test_a_holder_follows_an_authority
    authority = L::LeaseAuthority.with_scope(SCOPE)
    lease = authority.acquire(1, 0, cadence.ttl)
    holder = L::Holder.new(lease.token, 0, cadence)
    assert_equal :started, holder.refresh(at(10))
    renewed = authority.renew(1, holder.token, at(10), cadence.ttl)
    assert_equal :recorded, holder.confirm(renewed.token, at(10))
    assert_equal at(35), holder.warning_deadline
    assert_equal :expired, authority.renew(1, holder.token, at(40), cadence.ttl)
    assert_equal :recorded, holder.expire
    fresh = authority.acquire(1, at(40), cadence.ttl)
    assert_equal :recorded, holder.reacquire(fresh.token, at(40))
    assert_equal fresh.token, holder.token
  end

  def test_cadence_is_validated_data
    assert_equal %i[ttl renew_after warn_before], L::Cadence.members
    assert_equal [at(30), at(10), at(5)], cadence.to_h.values
    assert_raises(RangeError) { L::Cadence.new(0, 0, 0) }
    assert_raises(RangeError) { L::Cadence.new(at(30), at(30), at(5)) }
    assert_raises(RangeError) { L::Cadence.new(at(30), at(10), at(31)) }
    assert_raises(RangeError) { L::Cadence.new(at(30), -1, at(5)) }
    assert_raises(TypeError) { L::Cadence.new(30.0, 10.0, 5.0) }
    assert_raises(ArgumentError) { L::Cadence.new(at(30), at(10)) }
  end

  def test_malformed_input_raises_and_frozen_means_frozen
    assert_raises(TypeError) { L::Holder.new(1, 0, cadence) }
    assert_raises(TypeError) { L::Holder.new(token(SCOPE, 1), 0.0, cadence) }
    assert_raises(TypeError) { L::Holder.new(token(SCOPE, 1), 0, [at(30), at(10), at(5)]) }
    assert_raises(RangeError) { L::Holder.new(token(SCOPE, 1), -1, cadence) }
    holder = holder_in(:unconfirmed)
    assert_raises(TypeError) { holder.confirm(5, at(1)) }
    assert_raises(TypeError) { holder.confirm(token(SCOPE, 5), 1.5) }
    assert_raises(TypeError) { holder.renewal_due?(nil) }
    assert_raises(TypeError) { holder.dup }
    holder.freeze
    assert_raises(FrozenError) { holder.refresh(at(1)) }
    assert_raises(FrozenError) { holder.confirm(token(SCOPE, 5), at(1)) }
    assert_raises(FrozenError) { holder.expire }
    assert_raises(FrozenError) { holder.reacquire(token(SCOPE, 6), at(1)) }
    assert_raises(FrozenError) { holder.fence }
    assert_equal :unconfirmed, holder.state
  end
end
