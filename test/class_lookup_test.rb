# frozen_string_literal: true

require_relative 'test_helper'
require 'open3'
require 'rbconfig'

# The classes of the values the extension wraps (LeaseAuthority, Fence,
# Holder) are fixed when it loads. Nothing a caller does later, redefining a
# constant or `Object.const_get`, reaches a value being built. Each case runs
# in a fresh process: the class is fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'lease_machines'
    L = LeaseMachines
    second = 1_000_000_000
    token = L::FencingToken.from_raw((7 << 48) | 5)
    cadence = L::Cadence.new(30 * second, 10 * second, 5 * second)
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: a LeaseAuthority constant redefined before the first authority made
  # it an instance of the new class, without the gem's methods.
  def test_a_redefined_authority_constant_does_not_change_the_authority_class
    assert_equal 'true 7 1', isolated(<<~RUBY)
      original = L::LeaseAuthority
      L.send(:remove_const, :LeaseAuthority)
      L.const_set(:LeaseAuthority, Class.new)
      authority = original.with_scope(7)
      lease = authority.acquire(1, 0, second)
      puts [authority.instance_of?(original), lease.token.scope, authority.issued].join(' ')
    RUBY
  end

  # Bug: the same for a Fence.
  def test_a_redefined_fence_constant_does_not_change_the_fence_class
    assert_equal 'true advanced', isolated(<<~RUBY)
      original = L::Fence
      L.send(:remove_const, :Fence)
      L.const_set(:Fence, Class.new)
      fence = original.with_scope(7)
      puts [fence.instance_of?(original), fence.accept(token)].join(' ')
    RUBY
  end

  # Bug: the same for a Holder.
  def test_a_redefined_holder_constant_does_not_change_the_holder_class
    assert_equal 'true confirmed started', isolated(<<~RUBY)
      original = L::Holder
      L.send(:remove_const, :Holder)
      L.const_set(:Holder, Class.new)
      holder = original.new(token, 0, cadence)
      puts [holder.instance_of?(original), holder.state, holder.refresh(10 * second)].join(' ')
    RUBY
  end

  # Bug: building the first value ran `Object.const_get`, a caller's code in
  # the middle of a native call. No Ruby runs while one is built now.
  def test_building_a_value_runs_no_const_get
    assert_equal '0 false LeaseAuthority Fence Holder', isolated(<<~RUBY)
      hits = 0
      log = []
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        log.freeze
        super(*args)
      end
      values = [L::LeaseAuthority.with_scope(7), L::Fence.with_scope(7), L::Holder.new(token, 0, cadence)]
      puts [hits, log.frozen?, *values.map { |v| v.class.name.split('::').last }].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal '1 advanced started', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran mid-call' }
      authority = L::LeaseAuthority.with_scope(7)
      authority.acquire(1, 0, second)
      fence = L::Fence.with_scope(7)
      holder = L::Holder.new(token, 0, cadence)
      puts [authority.issued, fence.accept(token), holder.refresh(10 * second)].join(' ')
    RUBY
  end
end
