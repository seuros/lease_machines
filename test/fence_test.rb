# frozen_string_literal: true
require 'test_helper'

# Tokens, and the fence at the protected resource.
class FenceTest < Minitest::Test
  include LeaseHelpers

  def test_accept_says_advanced_or_current
    fence = L::Fence.with_scope(SCOPE)
    assert_equal [SCOPE, nil], [fence.scope, fence.highest]
    assert_equal :advanced, fence.accept(token(SCOPE, 3))
    assert_repeat(:current, fence) { fence.accept(token(SCOPE, 3)) }
    assert_equal :advanced, fence.accept(token(SCOPE, 5))
    assert_refused(:stale_token, fence) { fence.accept(token(SCOPE, 4)) }
    assert_equal token(SCOPE, 5), fence.highest
  end

  def test_a_fence_refuses_every_other_scope
    fence = L::Fence.with_scope(SCOPE)
    # The first token does not pick the scope.
    assert_refused(:unknown, fence) { fence.accept(token(OTHER, 1)) }
    assert_equal :advanced, fence.accept(token(SCOPE, 2))
    [token(OTHER, 1), token(OTHER, 9), token(0, 9), token(65_535, (2**48) - 1)].each do |foreign|
      assert_refused(:unknown, fence) { fence.accept(foreign) }
    end
    zero = L::Fence.new
    assert_equal 0, zero.scope
    assert_refused(:unknown, zero) { zero.accept(token(SCOPE, 1)) }
    assert_equal :advanced, zero.accept(L::FencingToken.from_raw(1))
  end

  def test_two_authorities_cannot_be_confused
    here = L::LeaseAuthority.with_scope(SCOPE)
    there = L::LeaseAuthority.with_scope(OTHER)
    mine = here.acquire(1, 0, at(5)).token
    theirs = there.acquire(1, 0, at(5)).token
    refute_equal mine, theirs
    assert_equal [SCOPE, OTHER], [mine.scope, theirs.scope]
    assert_refused(:unknown, here) { here.renew(1, theirs, at(1), at(5)) }
    assert_refused(:unknown, there) { there.release(1, mine, at(1)) }
    fence = L::Fence.with_scope(SCOPE)
    assert_equal :advanced, fence.accept(mine)
    assert_refused(:unknown, fence) { fence.accept(theirs) }
  end

  def test_resumed_takes_the_scope_explicitly
    fence = L::Fence.resumed(SCOPE, token(SCOPE, 9))
    assert_refused(:stale_token, fence) { fence.accept(token(SCOPE, 8)) }
    assert_repeat(:current, fence) { fence.accept(token(SCOPE, 9)) }
    assert_nil L::Fence.resumed(SCOPE, nil).highest
    assert_raises(RangeError) { L::Fence.resumed(OTHER, token(SCOPE, 9)) }
    assert_raises(RangeError) { L::Fence.resumed(0, token(SCOPE, 9)) }
    assert_raises(TypeError) { L::Fence.resumed(SCOPE, 9) }
    assert_raises(ArgumentError) { L::Fence.new(highest: nil) }
    assert_raises(RangeError) { L::Fence.with_scope(65_536) }
    assert_raises(TypeError) { L::Fence.with_scope(1.0) }
  end

  def test_tokens_are_data_ordered_only_within_a_scope
    old = token(SCOPE, 3)
    new = token(SCOPE, 4)
    foreign = token(OTHER, 1)
    assert_equal [-1, 1, 0], [old.order(new), new.order(old), new.order(new)]
    assert new.newer_than?(old)
    refute old.newer_than?(new)
    refute new.newer_than?(new)
    assert_operator foreign.raw, :>, new.raw
    assert_nil foreign.order(new)
    refute foreign.newer_than?(new)
    refute new.newer_than?(foreign)
    %i[< > <= >= between? clamp].each { |ranking| refute_respond_to new, ranking }
    assert_equal new, L::FencingToken.new(raw: new.raw)
    assert_equal [SCOPE, (SCOPE << 48) | 4], [new.scope, new.raw]
    assert_raises(TypeError) { new.order(new.raw) }
    assert_raises(TypeError) { new.newer_than?(nil) }
  end

  def test_a_token_needs_a_positive_sequence
    [0, SCOPE << 48, -1, 2**64].each { |raw| assert_raises(RangeError) { L::FencingToken.from_raw(raw) } }
    [1.0, '1', nil].each { |raw| assert_raises(TypeError) { L::FencingToken.from_raw(raw) } }
    assert_equal 65_535, L::FencingToken.from_raw((2**64) - 1).scope
  end

  def test_malformed_input_raises_and_frozen_means_frozen
    fence = L::Fence.with_scope(SCOPE)
    assert_raises(TypeError) { fence.accept(1) }
    assert_raises(TypeError) { fence.accept(nil) }
    assert_raises(TypeError) { fence.dup }
    fence.accept(token(SCOPE, 1))
    fence.freeze
    assert_raises(FrozenError) { fence.accept(token(SCOPE, 2)) }
    assert_equal token(SCOPE, 1), fence.highest
  end
end
