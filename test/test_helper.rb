# frozen_string_literal: true
$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'lease_machines'
require 'minitest/autorun'

module LeaseHelpers
  L = LeaseMachines
  SECOND = 1_000_000_000
  SCOPE = 7
  OTHER = 8

  # `seconds` as Integer nanoseconds.
  def at(seconds) = seconds * SECOND

  # Token `sequence` of `scope`, as if it had crossed a wire.
  def token(scope, sequence) = L::FencingToken.from_raw((scope << 48) | sequence)

  # Everything observable about an authority, fence or holder.
  def snapshot(subject)
    case subject
    when L::LeaseAuthority then [subject.checkpoint, subject.clock_policy, subject.scope, subject.issued]
    when L::Fence then [subject.scope, subject.highest]
    when L::Holder
      [subject.state, subject.token, subject.cadence, subject.warning_deadline,
       (0..60).map { |s| [subject.renewal_due?(at(s)), subject.at_risk?(at(s))] }]
    end
  end

  # The call answers `refusal` and leaves `subject` exactly as it was.
  def assert_refused(refusal, subject)
    assert_includes L::REFUSALS, refusal
    before = snapshot(subject)
    assert_equal refusal, yield
    assert_equal before, snapshot(subject), "#{refusal} changed something"
  end

  # The call is honoured with `answer` and still changes nothing.
  def assert_repeat(answer, subject)
    refute_includes L::REFUSALS, answer
    before = snapshot(subject)
    assert_equal answer, yield
    assert_equal before, snapshot(subject), "#{answer} changed something"
  end
end

# Runs a hook once in place of the next `new` on a class, before the real
# one: an answer class whose construction raises, re-enters or freezes.
module AnswerHook
  HOOKS = {} # rubocop:disable Style/MutableConstant

  def new(...)
    AnswerHook::HOOKS.delete(self)&.call
    super
  end

  def self.around(klass, hook)
    klass.singleton_class.prepend(self) unless klass.singleton_class.ancestors.include?(self)
    HOOKS[klass] = hook
    yield
  ensure
    HOOKS.delete(klass)
  end
end

module LeaseHelpers
  # Builds a value past `initialize`: Marshal.load skips it. `from` and `to`
  # are the marshalled bytes of one field, before and after.
  def forge(value, from, to)
    bytes = Marshal.dump(value)
    raise ArgumentError, 'field not found' unless bytes.include?(from)

    Marshal.load(bytes.sub(from, to)) # rubocop:disable Security/MarshalLoad
  end
end
