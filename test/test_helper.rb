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
