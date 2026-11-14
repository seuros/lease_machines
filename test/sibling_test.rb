# frozen_string_literal: true
require 'test_helper'

# A sibling gem's token, lease or checkpoint is read by its readers.
class SiblingTest < Minitest::Test
  include LeaseHelpers

  # What `ElectionMachines` and `HandoffMachines` values look like to this
  # gem: other classes with the same readers.
  SiblingToken = Data.define(:raw)
  SiblingLease = Data.define(:owner, :token, :expires_at)
  SiblingCheckpoint = Data.define(:scope, :issued, :lease, :observed)

  def sibling(scope, sequence) = SiblingToken.new((scope << 48) | sequence)

  def cadence = L::Cadence.new(at(30), at(10), at(5))

  def test_a_fence_reads_a_siblings_token
    fence = L::Fence.with_scope(SCOPE)
    assert_equal :advanced, fence.accept(sibling(SCOPE, 3))
    assert_repeat(:current, fence) { fence.accept(sibling(SCOPE, 3)) }
    assert_refused(:stale_token, fence) { fence.accept(sibling(SCOPE, 2)) }
    assert_refused(:unknown, fence) { fence.accept(sibling(OTHER, 9)) }
    assert_equal token(SCOPE, 3), fence.highest
    assert_instance_of L::FencingToken, fence.highest
    resumed = L::Fence.resumed(SCOPE, sibling(SCOPE, 9))
    assert_equal token(SCOPE, 9), resumed.highest
    assert_raises(RangeError) { L::Fence.resumed(OTHER, sibling(SCOPE, 9)) }
  end

  def test_an_authority_reads_a_siblings_token
    authority = L::LeaseAuthority.with_scope(SCOPE)
    lease = authority.acquire(1, 0, at(5))
    held = SiblingToken.new(lease.token.raw)
    assert_refused(:wrong_owner, authority) { authority.renew(2, held, at(1), at(5)) }
    assert_refused(:unknown, authority) { authority.release(1, sibling(OTHER, 1), at(1)) }
    renewed = authority.renew(1, held, at(1), at(5))
    assert_equal L::Lease.new(1, lease.token, at(6)), renewed
    assert_instance_of L::FencingToken, renewed.token
    assert_equal renewed, authority.release(1, held, at(2))
  end

  def test_a_holder_reads_a_siblings_token
    holder = L::Holder.new(sibling(SCOPE, 1), 0, cadence)
    assert_equal token(SCOPE, 1), holder.token
    assert_instance_of L::FencingToken, holder.token
    holder.refresh(at(10))
    assert_equal :recorded, holder.confirm(sibling(SCOPE, 1), at(11))
    assert_repeat(:duplicate, holder) { holder.confirm(sibling(SCOPE, 1), at(11)) }
    assert_refused(:unknown, holder) { holder.confirm(sibling(OTHER, 1), at(12)) }
    holder.expire
    assert_refused(:stale_token, holder) { holder.reacquire(sibling(SCOPE, 1), at(50)) }
    assert_equal :recorded, holder.reacquire(sibling(SCOPE, 2), at(50))
    assert_equal token(SCOPE, 2), holder.token
  end

  def test_tokens_compare_with_a_siblings
    mine = token(SCOPE, 4)
    assert_equal [1, 0, -1], [3, 4, 5].map { |sequence| mine.order(sibling(SCOPE, sequence)) }
    assert mine.newer_than?(sibling(SCOPE, 3))
    assert_nil mine.order(sibling(OTHER, 1))
  end

  def test_a_siblings_raw_is_validated_like_from_raw
    fence = L::Fence.with_scope(SCOPE)
    authority = L::LeaseAuthority.with_scope(SCOPE)
    [0, SCOPE << 48, -1, 2**64].each do |raw|
      assert_raises(RangeError) { L::FencingToken.from_raw(raw) }
      assert_raises(RangeError) { fence.accept(SiblingToken.new(raw)) }
      assert_raises(RangeError) { authority.renew(1, SiblingToken.new(raw), 0, at(1)) }
      assert_raises(RangeError) { L::Holder.new(SiblingToken.new(raw), 0, cadence) }
    end
    [1.0, '1', nil].each do |raw|
      assert_raises(TypeError) { L::FencingToken.from_raw(raw) }
      assert_raises(TypeError) { fence.accept(SiblingToken.new(raw)) }
      assert_raises(TypeError) { authority.release(1, SiblingToken.new(raw), 0) }
    end
    # No `raw` reader, no token: a bare Integer included.
    [1, (SCOPE << 48) | 1, nil, '1', [1], { raw: 1 }].each do |bare|
      assert_raises(TypeError) { fence.accept(bare) }
      assert_raises(TypeError) { authority.renew(1, bare, 0, at(1)) }
      assert_raises(TypeError) { L::Holder.new(bare, 0, cadence) }
      assert_raises(TypeError) { token(SCOPE, 1).order(bare) }
    end
    assert_equal [nil, L::Checkpoint.new(SCOPE, 0, nil, 0)], [fence.highest, authority.checkpoint]
  end

  def test_a_value_without_the_readers_is_refused_naming_them
    fence = L::Fence.with_scope(SCOPE)
    error = assert_raises(TypeError) { fence.accept(1) }
    assert_equal 'expected a LeaseMachines::FencingToken or a value with #raw', error.message
    error = assert_raises(TypeError) { L::Checkpoint.from_parts(SCOPE, 1, [1, 1, 5], 0) }
    assert_equal 'expected a LeaseMachines::Lease or a value with #owner, #token, #expires_at', error.message
    error = assert_raises(TypeError) { L::LeaseAuthority.restored(SCOPE, [SCOPE, 0, nil, 0], :reject) }
    assert_equal 'expected a LeaseMachines::Checkpoint or a value with #scope, #issued, #lease, #observed',
                 error.message
  end

  # Every reader is asked for before any runs: a value missing one runs none.
  # A private reader is no reader.
  def test_a_value_missing_one_reader_runs_none
    calls = []
    partial = Object.new
    partial.define_singleton_method(:owner) { calls << :owner && 1 }
    partial.define_singleton_method(:token) { calls << :token && sibling(SCOPE, 1) }
    assert_raises(TypeError) { L::Checkpoint.from_parts(SCOPE, 1, partial, 0) }
    partial.define_singleton_method(:expires_at) { calls << :expires_at && 0 }
    partial.singleton_class.send(:private, :expires_at)
    assert_raises(TypeError) { L::Checkpoint.from_parts(SCOPE, 1, partial, 0) }
    assert_empty calls
  end

  def test_a_siblings_lease_and_checkpoint_are_read_and_answered_as_this_gems
    held = SiblingLease.new(1, sibling(SCOPE, 1), at(15))
    own = L::Lease.new(1, token(SCOPE, 1), at(15))
    saved = L::Checkpoint.from_parts(SCOPE, 1, held, at(10))
    assert_equal own, saved.lease
    assert_instance_of L::Lease, saved.lease
    assert_instance_of L::FencingToken, saved.lease.token
    assert_raises(RangeError) { L::Checkpoint.from_parts(SCOPE, 2, held, 0) }
    assert_raises(TypeError) { L::Checkpoint.from_parts(SCOPE, 1, SiblingLease.new(1, 1, at(15)), 0) }

    authority = L::LeaseAuthority.restored(SCOPE, SiblingCheckpoint.new(SCOPE, 1, held, at(10)), :reject)
    assert_equal saved, authority.checkpoint
    assert_instance_of L::Checkpoint, authority.checkpoint
    assert_equal L::Lease.new(1, token(SCOPE, 1), at(16)), authority.renew(1, held.token, at(11), at(5))
    # A sibling's checkpoint is validated like `from_parts`.
    assert_raises(RangeError) { L::LeaseAuthority.restored(SCOPE, SiblingCheckpoint.new(SCOPE, 2, held, 0), :reject) }
    assert_raises(RangeError) { L::LeaseAuthority.restored(OTHER, SiblingCheckpoint.new(SCOPE, 1, held, 0), :reject) }
    assert_raises(TypeError) { L::LeaseAuthority.restored(SCOPE, [SCOPE, 0, nil, 0], :reject) }
    rows = SiblingCheckpoint.new(SCOPE, 1, [1, 1, 5], 0)
    assert_raises(TypeError) { L::LeaseAuthority.restored(SCOPE, rows, :reject) }
  end
end
