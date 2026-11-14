# frozen_string_literal: true
require 'test_helper'

# Grants, renewals, releases, and every refusal the authority can answer.
class AuthorityTest < Minitest::Test
  include LeaseHelpers

  def setup
    @authority = L::LeaseAuthority.with_scope(SCOPE)
  end

  def test_grant_renew_release_answer_leases
    first = @authority.acquire(1, at(0), at(2))
    assert_equal L::Lease.new(1, token(SCOPE, 1), at(2)), first
    assert_refused(:occupied, @authority) { @authority.acquire(2, at(1), at(2)) }
    renewed = @authority.renew(1, first.token, at(1), at(3))
    assert_equal L::Lease.new(1, first.token, at(4)), renewed
    assert_equal renewed, @authority.release(1, first.token, at(2))
    assert_refused(:no_lease, @authority) { @authority.release(1, first.token, at(2)) }
    second = @authority.acquire(2, at(2), at(2))
    assert second.token.newer_than?(first.token)
    assert_equal 2, @authority.issued
  end

  def test_new_is_scope_zero_and_counts_from_one
    authority = L::LeaseAuthority.new
    assert_equal [0, 0, :reject], [authority.scope, authority.issued, authority.clock_policy]
    assert_equal [1, 2, 3], Array.new(3) { authority.reacquire(1, 0, at(1)).token.raw }
  end

  def test_the_owner_of_a_live_grant_must_renew_or_rotate
    first = @authority.acquire(1, at(0), at(5))
    assert_refused(:held, @authority) { @authority.acquire(1, at(0), at(5)) }
    assert_refused(:occupied, @authority) { @authority.reacquire(2, at(1), at(5)) }
    rotated = @authority.reacquire(1, at(1), at(5))
    assert rotated.token.newer_than?(first.token)
    assert_refused(:stale_token, @authority) { @authority.renew(1, first.token, at(2), at(5)) }
    assert_refused(:wrong_owner, @authority) { @authority.release(2, rotated.token, at(2)) }
  end

  def test_tokens_never_issued_are_unknown
    live = @authority.acquire(1, at(0), at(5)).token
    [token(SCOPE, 2), token(OTHER, 1), token(0, 1)].each do |forged|
      assert_refused(:unknown, @authority) { @authority.renew(1, forged, at(1), at(5)) }
      assert_refused(:unknown, @authority) { @authority.release(1, forged, at(1)) }
    end
    assert_equal live, @authority.renew(1, live, at(1), at(5)).token
  end

  def test_refusals_do_not_move_the_clock
    live = @authority.acquire(1, at(0), at(10)).token
    assert_refused(:invalid_ttl, @authority) { @authority.acquire(2, at(1), 0) }
    assert_refused(:occupied, @authority) { @authority.acquire(2, at(2), at(5)) }
    assert_refused(:wrong_owner, @authority) { @authority.renew(2, live, at(3), at(5)) }
    assert_refused(:expired, @authority) { @authority.renew(1, live, at(20), at(5)) }
    assert_equal 0, @authority.checkpoint.observed
    assert_equal at(13), @authority.renew(1, live, at(3), at(10)).expires_at
    assert_equal at(3), @authority.checkpoint.observed
    assert_refused(:clock_regressed, @authority) { @authority.renew(1, live, at(2), at(10)) }
    assert_refused(:clock_regressed, @authority) { @authority.release(1, live, at(2)) }
    assert_refused(:clock_regressed, @authority) { @authority.acquire(2, at(2), at(1)) }
    assert_refused(:clock_regressed, @authority) { @authority.reacquire(1, at(2), at(1)) }
  end

  def test_clamp_evaluates_at_the_last_honoured_instant
    assert_same @authority, @authority.with_clock_policy(:clamp)
    assert_equal :clamp, @authority.clock_policy
    lease = @authority.acquire(1, at(10), at(5))
    assert_equal at(15), @authority.renew(1, lease.token, at(3), at(5)).expires_at
    assert_equal at(10), @authority.checkpoint.observed
    assert_refused(:occupied, @authority) { @authority.acquire(2, at(1), at(5)) }
    assert_equal lease, @authority.release(1, lease.token, at(2))
    assert_equal at(15), @authority.acquire(2, at(1), at(5)).expires_at
    assert_raises(TypeError) { @authority.with_clock_policy('clamp') }
    assert_raises(RangeError) { @authority.with_clock_policy(:ignore) }
    assert_equal %i[reject clamp], L::CLOCK_POLICIES
  end

  # `:expired` answers for the instant supplied, under either policy, and is
  # not remembered. Exclusion holds anyway: another owner enters only through
  # an honoured `acquire`, which records its instant and takes a newer token.
  def test_expired_is_an_answer_about_the_instant_supplied
    L::CLOCK_POLICIES.each do |policy|
      authority = L::LeaseAuthority.with_scope(SCOPE).with_clock_policy(policy)
      first = authority.acquire(1, at(0), at(10)).token
      assert_equal token(SCOPE, 1), first
      assert_refused(:expired, authority) { authority.renew(1, first, at(15), at(10)) }
      assert_equal L::Lease.new(1, first, at(15)), authority.renew(1, first, at(5), at(10)), policy
      assert_equal at(5), authority.checkpoint.observed
      assert_refused(:occupied, authority) { authority.acquire(2, at(12), at(10)) }
      second = authority.acquire(2, at(15), at(10))
      assert_equal token(SCOPE, 2), second.token
      assert_refused(:wrong_owner, authority) { authority.renew(1, first, at(16), at(10)) }
      assert_equal second, authority.checkpoint.lease
    end
  end

  def test_a_holder_that_recorded_expiry_refuses_the_late_confirmation
    first = @authority.acquire(1, at(0), at(10)).token
    holder = L::Holder.new(first, at(0), L::Cadence.new(at(10), at(3), at(2)))
    assert_equal :started, holder.refresh(at(3))
    assert_refused(:expired, @authority) { @authority.renew(1, first, at(15), at(10)) }
    assert_equal :recorded, holder.expire
    renewed = @authority.renew(1, first, at(5), at(10))
    assert_refused(:expired, holder) { holder.confirm(renewed.token, at(6)) }
    assert_refused(:stale_token, holder) { holder.reacquire(renewed.token, at(6)) }
  end

  def test_an_exhausted_scope_fails_closed
    spent = L::Checkpoint.from_parts(SCOPE, (2**48) - 2, nil, 0)
    authority = L::LeaseAuthority.restored(SCOPE, spent, :reject)
    last = authority.acquire(1, 0, at(1))
    assert_equal token(SCOPE, (2**48) - 1), last.token
    assert_refused(:exhausted, authority) { authority.acquire(2, at(1), at(1)) }
    assert_refused(:exhausted, authority) { authority.reacquire(1, 0, at(1)) }
  end

  def test_time_is_integer_nanoseconds_in_and_out
    lease = @authority.acquire(1, 1, 2)
    assert_equal 3, lease.expires_at
    assert_equal 1, @authority.checkpoint.observed
    # Past 2**64 nanoseconds, exactly.
    far = @authority.renew(1, lease.token, 2, 2**70)
    assert_equal (2**70) + 2, far.expires_at
    assert_equal far, @authority.checkpoint.lease
    [1.0, '1', nil, 1r].each do |bad|
      assert_raises(TypeError) { @authority.acquire(1, bad, at(1)) }
      assert_raises(TypeError) { @authority.acquire(1, 0, bad) }
      assert_raises(TypeError) { @authority.renew(1, lease.token, bad, at(1)) }
      assert_raises(TypeError) { @authority.release(1, lease.token, bad) }
    end
    [-1, (2**64) * SECOND].each do |bad|
      assert_raises(RangeError) { @authority.acquire(1, bad, at(1)) }
      assert_raises(RangeError) { @authority.reacquire(1, 0, bad) }
    end
    assert_refused(:invalid_ttl, @authority) { @authority.renew(1, lease.token, 3, ((2**64) * SECOND) - 1) }
  end

  def test_malformed_input_raises_and_frozen_means_frozen
    assert_raises(TypeError) { @authority.acquire(1.0, 0, at(1)) }
    assert_raises(RangeError) { @authority.acquire(-1, 0, at(1)) }
    assert_raises(RangeError) { @authority.acquire(2**64, 0, at(1)) }
    assert_raises(TypeError) { @authority.renew(1, 1, 0, at(1)) }
    assert_raises(TypeError) { @authority.release(1, nil, 0) }
    assert_raises(ArgumentError) { @authority.acquire(1, 0) }
    assert_raises(ArgumentError) { @authority.acquire(owner: 1, now: 0, ttl: 1) }
    assert_raises(TypeError) { @authority.dup }
    assert_raises(NoMethodError) { @authority.native_acquire(1, 0, 1) }
    lease = @authority.acquire((2**64) - 1, 0, at(1))
    GC.compact
    assert_equal (2**64) - 1, lease.owner
    @authority.freeze
    assert_raises(FrozenError) { @authority.acquire(1, at(5), at(1)) }
    assert_raises(FrozenError) { @authority.reacquire(1, at(5), at(1)) }
    assert_raises(FrozenError) { @authority.renew(1, lease.token, 0, at(1)) }
    assert_raises(FrozenError) { @authority.release(1, lease.token, 0) }
    assert_raises(FrozenError) { @authority.with_clock_policy(:clamp) }
    assert_equal 1, @authority.checkpoint.issued
  end

  # The classes of an honoured answer are resolved once at load: removing a
  # constant afterwards changes nothing the binding builds.
  def test_answer_classes_are_resolved_at_load
    %i[Lease FencingToken].each do |name|
      saved = L.send(:remove_const, name)
      begin
        lease = L::LeaseAuthority.with_scope(SCOPE).acquire(1, 0, at(1))
        assert_instance_of saved, name == :Lease ? lease : lease.token
      ensure
        L.const_set(name, saved)
      end
    end
  end

  def test_refusals_are_listed_once_each
    expected = %i[occupied held no_lease wrong_owner stale_token unknown expired clock_regressed
                  invalid_ttl exhausted fenced out_of_phase]
    assert_equal expected, L::REFUSALS
    assert_predicate L::REFUSALS, :frozen?
  end
end
