# frozen_string_literal: true

require_relative 'lease_machines/version'
# Native is required: no algorithm fallback.
require 'lease_machines_native/lease_machines_native'

# Every value mirrors the core type of the same name, field for field, and
# every method takes the core's arguments in the core's order. Every instant
# and span (`now`, `ttl`, `expires_at`, `observed`, a cadence) is Integer
# nanoseconds. Owners are Integers: the core's `OwnerId` is the caller's number.
#
# Wherever a method takes a FencingToken, a Lease or a Checkpoint it reads the
# value by its readers, not by its class, so a sibling gem's value of the same
# shape (`ElectionMachines::FencingToken`, `HandoffMachines::FencingToken`)
# fits as it is and is validated like this gem's own. Every value a method
# answers is this gem's own class.
module LeaseMachines
  class << self
    private :native_token_scope, :native_token_order, :native_token_newer, :native_cadence, :native_checkpoint
  end

  # A fencing token: the issuing authority's scope in the top 16 bits of `raw`
  # above a sequence number counted from 1. Plain data, so it survives a trip
  # over the wire as `raw` and comes back with `from_raw`. A token built here
  # proves nothing by itself: the authority still answers :unknown to one it
  # never issued, and a fence to one of another scope.
  #
  # Tokens of one scope are ordered; tokens of two scopes are not, so there is
  # no `<` or `<=>`. Compare with `order` and `newer_than?`.
  FencingToken = Data.define(:raw) do
    # The core's `FencingToken::from_raw`. RangeError when the sequence number
    # is zero, which no authority issues.
    def self.from_raw(raw) = new(raw)

    def initialize(raw:)
      LeaseMachines.__send__(:native_token_scope, raw)
      super
    end

    # The scope of the authority that issued this token.
    def scope = LeaseMachines.__send__(:native_token_scope, raw)

    # -1, 0 or 1 against a token of the same scope; nil across scopes.
    def order(other) = LeaseMachines.__send__(:native_token_order, raw, Rows.raw(other))

    # Whether this token is of `other`'s scope and strictly newer. False across
    # scopes, so its negation does not mean "older or equal".
    def newer_than?(other) = LeaseMachines.__send__(:native_token_newer, raw, Rows.raw(other))
  end

  # One grant: who holds the resource, under which token, until when.
  Lease = Data.define(:owner, :token, :expires_at)

  # Committed authority state, the core's `Checkpoint`: persist the four
  # fields before exposing a decision, rebuild with `from_parts`. RangeError
  # when `issued` is beyond the scope's token space or `lease` is not the
  # grant that history ends with (it must carry token number `issued` of
  # `scope`).
  #
  # `lease` is read by its readers and kept as this gem's Lease.
  Checkpoint = Data.define(:scope, :issued, :lease, :observed) do
    # The core's `Checkpoint::from_parts`.
    def self.from_parts(scope, issued, lease, observed) = new(scope, issued, lease, observed)

    def initialize(scope:, issued:, lease:, observed:)
      held = lease && Rows.lease(lease)
      LeaseMachines.__send__(:native_checkpoint, [scope, issued, held, observed])
      super(scope:, issued:, lease: held && Rows.to_lease(held), observed:)
    end
  end

  # Renewal timing for one grant, on the holder's own clock. RangeError unless
  # `ttl` is positive, `renew_after < ttl` and `warn_before <= ttl`.
  Cadence = Data.define(:ttl, :renew_after, :warn_before) do
    def initialize(ttl:, renew_after:, warn_before:)
      LeaseMachines.__send__(:native_cadence, [ttl, renew_after, warn_before])
      super
    end
  end

  # Values to and from the rows the native extension trades in. Tokens, leases
  # and checkpoints are read by their readers, whichever gem defined them; the
  # native side then validates each Integer as it does this gem's own.
  module Rows
    module_function

    def read(value, reader, what)
      raise TypeError, "expected #{what} (no ##{reader})" unless value.respond_to?(reader)

      value.public_send(reader)
    end

    # A bare Integer is not a token: it has no `raw`.
    def raw(token) = read(token, :raw, 'a fencing token')

    def lease(lease)
      what = 'a lease'
      [read(lease, :owner, what), raw(read(lease, :token, what)), read(lease, :expires_at, what)]
    end

    def to_lease(row) = Lease.new(row[0], FencingToken.new(row[1]), row[2])

    def checkpoint(checkpoint)
      what = 'a checkpoint'
      held = read(checkpoint, :lease, what)
      [read(checkpoint, :scope, what), read(checkpoint, :issued, what), held && lease(held),
       read(checkpoint, :observed, what)]
    end

    def cadence(cadence)
      raise TypeError, 'expected a LeaseMachines::Cadence' unless cadence.is_a?(Cadence)

      [cadence.ttl, cadence.renew_after, cadence.warn_before]
    end
  end
  private_constant :Rows

  # The core's `LeaseAuthority`: one resource, one token scope. Every decision
  # answers a Lease when honoured and a refusal Symbol otherwise.
  class LeaseAuthority
    class << self
      # A fresh token namespace in scope zero.
      def new = with_scope(0)

      # A fresh token namespace whose tokens carry `scope`, an Integer in
      # 0..65_535. Give each authority its own.
      def with_scope(scope) = native_with_scope(scope)

      # The newest committed checkpoint of `scope`, with the clock policy
      # stated again. RangeError when the checkpoint is of another scope.
      def restored(scope, checkpoint, clock_policy)
        native_restored(scope, Rows.checkpoint(checkpoint), clock_policy)
      end

      private :native_with_scope, :native_restored
    end

    # :reject or :clamp. The core's builder consumes the authority; here it
    # changes this one and answers it.
    def with_clock_policy(clock_policy)
      native_with_clock_policy(clock_policy)
      self
    end

    def checkpoint
      scope, issued, held, observed = native_checkpoint
      Checkpoint.new(scope, issued, held && Rows.to_lease(held), observed)
    end

    def acquire(owner, now, ttl) = decided(native_acquire(owner, now, ttl))

    def reacquire(owner, now, ttl) = decided(native_reacquire(owner, now, ttl))

    def renew(owner, token, now, ttl) = decided(native_renew(owner, Rows.raw(token), now, ttl))

    def release(owner, token, now) = decided(native_release(owner, Rows.raw(token), now))

    private

    def decided(answer) = answer.is_a?(Symbol) ? answer : Rows.to_lease(answer)
  end

  # The core's `Fence`: the protected resource's side. It is built for a scope
  # and refuses every token of another.
  class Fence
    class << self
      # A resource that has accepted nothing, for scope zero.
      def new = with_scope(0)

      # A resource that has accepted nothing, for the tokens of `scope`.
      def with_scope(scope) = native_with_scope(scope)

      # The newest committed high-water mark of `scope` (nil for none).
      # RangeError when `highest` is a token of another scope.
      def resumed(scope, highest) = native_resumed(scope, highest && Rows.raw(highest))

      private :native_with_scope, :native_resumed
    end

    def highest
      raw = native_highest
      raw && FencingToken.new(raw)
    end

    def accept(token) = native_accept(Rows.raw(token))
  end

  # The core's `Holder`: the renewal tracker on the side that holds the grant.
  class Holder
    class << self
      def new(token, confirmed_at, cadence) = native_new(Rows.raw(token), confirmed_at, Rows.cadence(cadence))

      private :native_new
    end

    def token = FencingToken.new(native_token)

    def cadence = Cadence.new(*native_cadence)

    def confirm(token, at) = native_confirm(Rows.raw(token), at)

    def reacquire(token, at) = native_reacquire(Rows.raw(token), at)
  end
end
