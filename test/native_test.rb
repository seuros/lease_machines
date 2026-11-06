# frozen_string_literal: true
require 'test_helper'

class LeaseNativeTest < Minitest::Test
  def test_grant_renew_release_and_new_owner_fencing
    authority = LeaseMachines::Authority.new
    first = authority.acquire(owner: 1, now: 0.0, ttl: 2.0)
    assert_equal :granted, first.status
    assert_equal [1, 1, 2.0], [first.lease.owner, first.lease.token, first.lease.expires_at]
    assert_equal :occupied, authority.acquire(owner: 2, now: 1.0, ttl: 2.0).reason
    renewed = authority.renew(owner: 1, token: 1, now: 1.0, ttl: 3.0)
    assert_equal :renewed, renewed.status
    assert_equal 4.0, renewed.lease.expires_at
    assert_equal :released, authority.release(owner: 1, token: 1, now: 2.0).status
    second = authority.acquire(owner: 2, now: 2.0, ttl: 2.0)
    fence = LeaseMachines::Fence.new
    assert fence.accept(second.lease.token)
    refute fence.accept(first.lease.token)
    assert_equal 2, fence.highest
  end

  def test_expiry_stale_tokens_and_clock_regression_remain_core_decisions
    authority = LeaseMachines::Authority.new
    authority.acquire(owner: 1, now: 2.0, ttl: 1.0)
    assert_equal :clock_regressed, authority.acquire(owner: 2, now: 1.0, ttl: 1.0).reason
    assert_equal :stale_token, authority.renew(owner: 1, token: 2, now: 2.0, ttl: 1.0).reason
    assert_equal :expired, authority.release(owner: 1, token: 1, now: 3.0).reason
    assert_equal :invalid_ttl, authority.acquire(owner: 2, now: 3.0, ttl: 0.0).reason
  end

  def test_checkpoint_retains_token_high_water
    authority = LeaseMachines::Authority.new
    authority.acquire(owner: 1, now: 0.0, ttl: 1.0)
    checkpoint = authority.checkpoint
    assert_equal 1, checkpoint.last_issued
    assert_equal 0.0, checkpoint.observed
    restored = LeaseMachines::Authority.restore(checkpoint)
    assert_equal 2, restored.acquire(owner: 2, now: 1.0, ttl: 1.0).lease.token
    assert_raises(TypeError) { LeaseMachines::Authority.restore(Object.new) }
    assert_raises(TypeError) { checkpoint.dup }
  end

  def test_boundary_validation_and_frozen_state
    authority = LeaseMachines::Authority.new
    assert_raises(TypeError) { authority.acquire(owner: 1.0, now: 0.0, ttl: 1.0) }
    assert_raises(RangeError) { authority.acquire(owner: -1, now: 0.0, ttl: 1.0) }
    assert_raises(RangeError) { authority.acquire(owner: 2**64, now: 0.0, ttl: 1.0) }
    [-1.0, Float::INFINITY, Float::NAN].each do |now|
      assert_raises(ArgumentError) { authority.acquire(owner: 1, now: now, ttl: 1.0) }
    end
    assert_raises(ArgumentError) { LeaseMachines::Fence.new(highest: 0) }
    authority.freeze
    assert_raises(FrozenError) { authority.acquire(owner: 1, now: 0.0, ttl: 1.0) }
    assert_raises(FrozenError) { LeaseMachines::Fence.new.freeze.accept(1) }
  end

  def test_fence_equal_tokens_and_native_state_survive_gc
    authority = LeaseMachines::Authority.new
    first = authority.acquire(owner: (2**64) - 1, now: 0.0, ttl: 1.0)
    checkpoint = authority.checkpoint
    GC.compact
    assert_equal((2**64) - 1, first.lease.owner)
    assert_equal 1, checkpoint.last_issued
    fence = LeaseMachines::Fence.new(highest: 2)
    assert fence.accept(2)
    refute fence.accept(1)
    assert_equal 2, fence.highest
    assert_raises(TypeError) { fence.accept(2.0) }
    assert_raises(ArgumentError) { fence.accept(0) }
    assert_raises(TypeError) { authority.dup }
    assert_raises(TypeError) { LeaseMachines::Lease.new }
    assert_raises(TypeError) { LeaseMachines::Checkpoint.new }
  end
end
