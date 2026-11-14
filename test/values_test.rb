# frozen_string_literal: true
require 'test_helper'

# A value is valid or does not exist, whoever built it; what a value holds is
# this gem's own class.
class ValuesTest < Minitest::Test
  include LeaseHelpers

  SiblingToken = Data.define(:raw)

  def test_a_lease_built_by_hand_is_validated_like_a_granted_one
    assert_raises(TypeError) { L::Lease.new('x', 5, -1) }
    assert_raises(TypeError) { L::Lease.new('x', token(SCOPE, 1), 0) }
    assert_raises(TypeError) { L::Lease.new(1, 5, 0) }
    assert_raises(TypeError) { L::Lease.new(1, token(SCOPE, 1), 1.5) }
    assert_raises(TypeError) { L::Lease.new(1, SiblingToken.new('1'), 0) }
    assert_raises(RangeError) { L::Lease.new(-1, token(SCOPE, 1), 0) }
    assert_raises(RangeError) { L::Lease.new(2**64, token(SCOPE, 1), 0) }
    assert_raises(RangeError) { L::Lease.new(1, token(SCOPE, 1), -1) }
    assert_raises(RangeError) { L::Lease.new(1, token(SCOPE, 1), (2**64) * SECOND) }
    assert_raises(RangeError) { L::Lease.new(1, SiblingToken.new(SCOPE << 48), 0) }
    assert_raises(RangeError) { L::Lease.new(owner: 1, token: token(SCOPE, 1), expires_at: -1) }
    assert_raises(ArgumentError) { L::Lease.new(1, token(SCOPE, 1)) }
  end

  def test_a_lease_holds_this_gems_token
    foreign = SiblingToken.new(token(SCOPE, 3).raw)
    lease = L::Lease.new(1, foreign, at(5))
    assert_instance_of L::FencingToken, lease.token
    assert_equal token(SCOPE, 3), lease.token
    assert_equal L::Lease.new(1, token(SCOPE, 3), at(5)), lease
    authority = L::LeaseAuthority.with_scope(SCOPE)
    granted = authority.acquire(1, 0, at(5))
    assert_equal granted, L::Lease.new(1, SiblingToken.new(granted.token.raw), at(5))
  end

  def test_a_value_out_of_the_cores_domain_is_a_range_error_never_an_argument_error
    authority = L::LeaseAuthority.with_scope(SCOPE)
    assert_raises(RangeError) { authority.with_clock_policy(:ignore) }
    assert_raises(RangeError) { L::LeaseAuthority.restored(SCOPE, authority.checkpoint, :later) }
    assert_raises(TypeError) { authority.with_clock_policy('clamp') }
    assert_raises(RangeError) { L::Cadence.new(at(30), at(30), at(5)) }
    assert_raises(TypeError) { L::Cadence.new(at(30), nil, at(5)) }
    assert_raises(RangeError) { L::FencingToken.new(0) }
    assert_raises(TypeError) { L::FencingToken.new('1') }
    assert_raises(RangeError) { L::Checkpoint.new(SCOPE, 2**48, nil, 0) }
    assert_raises(TypeError) { L::Checkpoint.new(SCOPE, 0, :lease, 0) }
  end
end
