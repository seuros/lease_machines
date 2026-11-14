# frozen_string_literal: true
require 'test_helper'
require 'json'

# Persisting an authority and a fence across a process restart.
class CheckpointTest < Minitest::Test
  include LeaseHelpers

  # What a store would hold: plain Integers and nil.
  def dump(checkpoint)
    held = checkpoint.lease
    JSON.generate(scope: checkpoint.scope, issued: checkpoint.issued, observed: checkpoint.observed,
                  lease: held && [held.owner, held.token.raw, held.expires_at])
  end

  def load(json)
    row = JSON.parse(json)
    held = row['lease'] && L::Lease.new(row['lease'][0], L::FencingToken.from_raw(row['lease'][1]), row['lease'][2])
    L::Checkpoint.from_parts(row['scope'], row['issued'], held, row['observed'])
  end

  def test_a_restart_resumes_the_tokens_instead_of_reissuing_them
    authority = L::LeaseAuthority.with_scope(SCOPE)
    first = authority.acquire(1, at(10), at(5))
    saved = dump(authority.checkpoint)
    fence = L::Fence.with_scope(SCOPE)
    assert_equal :advanced, fence.accept(first.token)
    highest = fence.highest.raw

    # The process dies. Everything below is rebuilt from `saved` and `highest`.
    restored = L::LeaseAuthority.restored(SCOPE, load(saved), :reject)
    assert_equal authority.checkpoint, restored.checkpoint
    assert_equal [SCOPE, 1], [restored.scope, restored.issued]
    assert_refused(:occupied, restored) { restored.acquire(2, at(11), at(5)) }
    assert_refused(:clock_regressed, restored) { restored.acquire(2, at(9), at(5)) }
    second = restored.acquire(2, at(15), at(5))
    assert_equal token(SCOPE, 2), second.token

    resumed = L::Fence.resumed(SCOPE, L::FencingToken.from_raw(highest))
    assert_equal :advanced, resumed.accept(second.token)
    assert_refused(:stale_token, resumed) { resumed.accept(first.token) }
  end

  def test_a_fresh_authority_would_have_reissued_the_token
    authority = L::LeaseAuthority.with_scope(SCOPE)
    first = authority.acquire(1, 0, at(5))
    reborn = L::LeaseAuthority.with_scope(SCOPE)
    assert_equal first.token, reborn.acquire(2, 0, at(5)).token
  end

  def test_restoring_takes_the_scope_and_the_policy_explicitly
    authority = L::LeaseAuthority.with_scope(SCOPE).with_clock_policy(:clamp)
    authority.acquire(1, at(1), at(5))
    saved = authority.checkpoint
    [0, OTHER, 65_535].each do |wrong|
      assert_raises(RangeError) { L::LeaseAuthority.restored(wrong, saved, :clamp) }
    end
    assert_raises(RangeError) { L::LeaseAuthority.restored(0, L::LeaseAuthority.with_scope(SCOPE).checkpoint, :reject) }
    assert_equal :clamp, L::LeaseAuthority.restored(SCOPE, saved, :clamp).clock_policy
    assert_equal :reject, L::LeaseAuthority.restored(SCOPE, saved, :reject).clock_policy
    assert_raises(ArgumentError) { L::LeaseAuthority.restored(SCOPE, saved) }
    assert_raises(RangeError) { L::LeaseAuthority.restored(SCOPE, saved, :later) }
    assert_raises(TypeError) { L::LeaseAuthority.restored(SCOPE, saved.to_h, :reject) }
    assert_raises(TypeError) { L::LeaseAuthority.restored('7', saved, :reject) }
  end

  def test_checkpoints_mirror_the_core_fields
    authority = L::LeaseAuthority.with_scope(SCOPE)
    assert_equal L::Checkpoint.new(SCOPE, 0, nil, 0), authority.checkpoint
    lease = authority.acquire(1, at(1), at(5))
    saved = authority.checkpoint
    assert_equal %i[scope issued lease observed], L::Checkpoint.members
    assert_equal [SCOPE, 1, lease, at(1)], saved.to_h.values
    assert_equal %i[owner token expires_at], L::Lease.members
    assert_equal saved, L::Checkpoint.from_parts(*saved.to_h.values)
    authority.release(1, lease.token, at(2))
    assert_equal L::Checkpoint.new(SCOPE, 1, nil, at(2)), authority.checkpoint
  end

  def test_rows_that_disagree_are_refused
    lease = L::Lease.new(1, token(SCOPE, 3), at(9))
    assert_equal lease, L::Checkpoint.from_parts(SCOPE, 3, lease, 0).lease
    [0, 2, 4].each { |issued| assert_raises(RangeError) { L::Checkpoint.from_parts(SCOPE, issued, lease, 0) } }
    assert_raises(RangeError) { L::Checkpoint.from_parts(OTHER, 3, lease, 0) }
    assert_raises(RangeError) { L::Checkpoint.from_parts(SCOPE, 2**48, nil, 0) }
    assert_raises(RangeError) { L::Checkpoint.from_parts(65_536, 0, nil, 0) }
    assert_raises(RangeError) { L::Checkpoint.from_parts(SCOPE, 0, nil, -1) }
    assert_raises(TypeError) { L::Checkpoint.from_parts(SCOPE, 3, [1, 3, 9], 0) }
    assert_raises(TypeError) { L::Checkpoint.from_parts(SCOPE, 3, L::Lease.new(1, 3, at(9)), 0) }
    assert_raises(TypeError) { L::Checkpoint.from_parts(SCOPE, 0, nil, 0.0) }
    assert_raises(TypeError) { L::Checkpoint.new(scope: nil, issued: 0, lease: nil, observed: 0) }
  end
end
