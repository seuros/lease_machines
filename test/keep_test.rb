# frozen_string_literal: true
require 'test_helper'

# Building an answer runs Ruby (`new`), so a decision is held on a copy and
# kept only once its answer exists: whatever happens in between, a raise
# means nothing was applied.
class KeepTest < Minitest::Test
  include LeaseHelpers

  def setup
    @authority = L::LeaseAuthority.with_scope(SCOPE)
  end

  def test_an_answer_whose_new_raises_applies_nothing
    [L::Lease, L::FencingToken].each do |answer|
      before = snapshot(@authority)
      AnswerHook.around(answer, -> { raise IOError, 'interrupted' }) do
        assert_raises(IOError) { @authority.acquire(1, 0, at(5)) }
      end
      assert_equal before, snapshot(@authority)
    end
    assert_equal L::Lease.new(1, token(SCOPE, 1), at(5)), @authority.acquire(1, 0, at(5))
  end

  def test_an_answer_whose_new_re_enters_applies_only_the_inner_call
    inner = nil
    AnswerHook.around(L::Lease, -> { inner = @authority.acquire(2, 0, at(5)) }) do
      assert_raises(RuntimeError) { @authority.acquire(1, 0, at(9)) }
    end
    assert_equal L::Lease.new(2, token(SCOPE, 1), at(5)), inner
    assert_equal inner, @authority.checkpoint.lease
    assert_equal 1, @authority.issued
  end

  def test_an_answer_whose_new_freezes_the_receiver_applies_nothing
    before = snapshot(@authority)
    AnswerHook.around(L::Lease, -> { @authority.freeze }) do
      assert_raises(FrozenError) { @authority.acquire(1, 0, at(5)) }
    end
    assert_equal before, snapshot(@authority)
  end

  def test_every_decision_keeps_its_copy
    lease = @authority.acquire(1, 0, at(5))
    %i[renew release].each do |call|
      before = snapshot(@authority)
      args = call == :renew ? [1, lease.token, at(1), at(5)] : [1, lease.token, at(1)]
      AnswerHook.around(L::Lease, -> { raise IOError }) do
        assert_raises(IOError) { @authority.public_send(call, *args) }
      end
      assert_equal before, snapshot(@authority)
    end
    @authority.release(1, lease.token, at(1))
    before = snapshot(@authority)
    AnswerHook.around(L::Lease, -> { raise IOError }) do
      assert_raises(IOError) { @authority.reacquire(1, at(2), at(5)) }
    end
    assert_equal before, snapshot(@authority)
  end

  def test_symbol_answers_check_frozen_up_front
    fence = L::Fence.with_scope(SCOPE).freeze
    assert_raises(FrozenError) { fence.accept(token(SCOPE, 1)) }
    holder = L::Holder.new(token(SCOPE, 1), 0, L::Cadence.new(at(30), at(10), at(5))).freeze
    assert_raises(FrozenError) { holder.refresh(at(10)) }
    assert_raises(FrozenError) { holder.confirm(token(SCOPE, 1), at(10)) }
    assert_raises(FrozenError) { holder.expire }
    assert_raises(FrozenError) { holder.reacquire(token(SCOPE, 2), at(10)) }
    assert_raises(FrozenError) { holder.fence }
  end

  # Frozen is checked before any argument is read: a frozen receiver handed a
  # malformed argument raises FrozenError, as Ruby's own core methods do.
  def test_frozen_is_checked_before_the_arguments
    @authority.freeze
    assert_raises(FrozenError) { @authority.acquire(1.5, 0, at(5)) }
    assert_raises(FrozenError) { @authority.reacquire(1, nil, at(5)) }
    assert_raises(FrozenError) { @authority.renew(1, :token, 0, at(1)) }
    assert_raises(FrozenError) { @authority.release(-1, nil, 0) }
    assert_raises(FrozenError) { @authority.with_clock_policy(:sometimes) }
    assert_raises(FrozenError) { L::Fence.with_scope(SCOPE).freeze.accept(nil) }
    holder = L::Holder.new(token(SCOPE, 1), 0, L::Cadence.new(at(30), at(10), at(5))).freeze
    assert_raises(FrozenError) { holder.refresh('10') }
    assert_raises(FrozenError) { holder.confirm(1, at(10)) }
    assert_raises(FrozenError) { holder.reacquire(token(SCOPE, 2), -1) }
  end

  # The binding holds its classes by address: they are pinned, so compaction
  # moves none of them and every path that checks or builds one still works.
  def test_classes_survive_compaction
    GC.compact if GC.respond_to?(:compact)
    lease = @authority.acquire(1, 0, at(5))
    assert_instance_of L::Lease, lease
    assert_instance_of L::FencingToken, lease.token
    assert_equal L::Lease.new(1, lease.token, at(6)), @authority.renew(1, lease.token, at(1), at(5))
    assert_instance_of L::Lease, @authority.release(1, lease.token, at(2))
    assert_instance_of L::Lease, @authority.reacquire(1, at(3), at(5))
    assert_instance_of L::Checkpoint, @authority.checkpoint
    assert_instance_of L::Lease, L::Checkpoint.new(SCOPE, 2, @authority.checkpoint.lease, 0).lease
    assert_equal :advanced, L::Fence.with_scope(SCOPE).accept(lease.token)
    holder = L::Holder.new(lease.token, 0, L::Cadence.new(at(30), at(10), at(5)))
    assert_equal lease.token, holder.token
  end
end
