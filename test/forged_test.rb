# frozen_string_literal: true
require 'test_helper'

# Sealing stops accidents; the native side is the authority. A value forged
# past `initialize` raises TypeError or RangeError wherever it is read, and
# never reaches a core.
class ForgedTest < Minitest::Test
  include LeaseHelpers

  VALUES = [L::FencingToken, L::Lease, L::Checkpoint, L::Cadence].freeze

  def test_every_value_class_is_sealed
    VALUES.each do |value|
      assert_raises(NoMethodError) { value.allocate }
      assert_raises(TypeError) { Class.new(value) }
    end
  end

  # A named subclass still raises, but Ruby has bound the constant first: it
  # leaks. That is why sealing is not the authority.
  def test_a_named_subclass_raises_but_its_constant_leaks
    leaked_token
    assert ForgedTest.const_defined?(:LeakedToken, false)
    assert_operator ForgedTest::LeakedToken, :<, L::FencingToken
  end

  # A named subclass of FencingToken: `class` raises TypeError, yet the
  # constant is bound. Given a `Data#initialize` bound past the check, it
  # builds tokens holding anything.
  def leaked_token
    unless ForgedTest.const_defined?(:LeakedToken, false)
      source = 'class ForgedTest::LeakedToken < L::FencingToken; end'
      assert_raises(TypeError) { eval(source) } # rubocop:disable Security/Eval
    end
    klass = ForgedTest::LeakedToken
    klass.class_eval do
      define_method(:initialize) { |**fields| Data.instance_method(:initialize).bind_call(self, **fields) }
    end
    klass
  end

  def test_a_subclass_instance_is_refused_whatever_it_holds
    authority = L::LeaseAuthority.with_scope(SCOPE)
    lease = authority.acquire(1, 0, at(5))
    fence = L::Fence.with_scope(SCOPE)
    [0, lease.token.raw].each do |raw|
      forged = leaked_token.new(raw:)
      before = [snapshot(authority), snapshot(fence)]
      assert_raises(TypeError) { authority.renew(1, forged, at(1), at(5)) }
      assert_raises(TypeError) { authority.release(1, forged, at(1)) }
      assert_raises(TypeError) { fence.accept(forged) }
      assert_raises(TypeError) { L::Lease.new(1, forged, 0) }
      assert_raises(TypeError) { L::Holder.new(forged, 0, L::Cadence.new(at(30), at(10), at(5))) }
      assert_raises(TypeError) { lease.token.order(forged) }
      assert_raises(TypeError) { forged.scope }
      assert_equal before, [snapshot(authority), snapshot(fence)]
    end
  end

  def test_a_marshalled_token_is_validated_again
    forged = forge(L::FencingToken.new(1), "i\x06".b, "i\x00".b)
    assert_equal 0, forged.raw
    fence = L::Fence.new
    assert_raises(RangeError) { fence.accept(forged) }
    assert_nil fence.highest
    authority = L::LeaseAuthority.new
    authority.acquire(1, 0, at(5))
    assert_refused(:wrong_owner, authority) { authority.renew(2, L::FencingToken.new(1), 0, at(5)) }
    before = snapshot(authority)
    assert_raises(RangeError) { authority.renew(1, forged, 0, at(5)) }
    assert_raises(RangeError) { authority.release(1, forged, 0) }
    assert_equal before, snapshot(authority)
    assert_raises(RangeError) { L::Lease.new(1, forged, 0) }
  end

  def test_a_marshalled_lease_checkpoint_or_cadence_is_validated_again
    lease = forge(L::Lease.new(1, L::FencingToken.new(1), 77), "i\x52".b, "i\xFA".b)
    assert_equal(-1, lease.expires_at)
    assert_raises(RangeError) { L::Checkpoint.new(0, 1, lease, 0) }
    saved = forge(L::Checkpoint.new(0, 0, nil, 77), "i\x52".b, "i\xFA".b)
    assert_raises(RangeError) { L::LeaseAuthority.restored(0, saved, :reject) }
    cadence = forge(L::Cadence.new(30, 10, 5), "i\x0F".b, "i\x28".b)
    assert_equal 35, cadence.renew_after
    assert_raises(RangeError) { L::Holder.new(L::FencingToken.new(1), 0, cadence) }
  end

  # A singleton class on the instance itself does not change what it is.
  def test_a_value_with_its_own_singleton_class_is_still_this_gems
    held = token(SCOPE, 1).tap(&:singleton_class)
    assert_equal :advanced, L::Fence.with_scope(SCOPE).accept(held)
    assert_equal L::Lease.new(1, token(SCOPE, 1), 0), L::Lease.new(1, held, 0)
  end
end
