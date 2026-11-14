# frozen_string_literal: true

require_relative 'lease_machines/version'

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
#
# The native side is the authority on every value it reads: one of this gem's
# classes must be exactly that class (a subclass is a TypeError, whatever it
# holds), and every field is validated again through the core constructor, so
# a value forged past `initialize` (`Marshal.load`, a bound
# `Data#initialize`) raises TypeError or RangeError and never reaches a core.
module LeaseMachines
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
      LeaseMachines.__send__(:native_raw, raw)
      super
    end

    # The scope of the authority that issued this token.
    def scope = LeaseMachines.__send__(:native_token_scope, self)

    # -1, 0 or 1 against a token of the same scope; nil across scopes.
    def order(other) = LeaseMachines.__send__(:native_token_order, self, other)

    # Whether this token is of `other`'s scope and strictly newer. False across
    # scopes, so its negation does not mean "older or equal".
    def newer_than?(other) = LeaseMachines.__send__(:native_token_newer, self, other)
  end

  # One grant: who holds the resource, under which token, until when. Built by
  # hand it is held to the rule a granted one is: an Integer owner in
  # 0..2**64-1, a token read by its `raw` reader and kept as this gem's
  # FencingToken, Integer nanoseconds. TypeError for the wrong type,
  # RangeError for a value outside that.
  Lease = Data.define(:owner, :token, :expires_at) do
    def initialize(owner:, token:, expires_at:)
      super(owner:, token: LeaseMachines.__send__(:native_lease, owner, token, expires_at), expires_at:)
    end
  end

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
      held = LeaseMachines.__send__(:native_checkpoint, scope, issued, lease, observed)
      super(scope:, issued:, lease: held, observed:)
    end
  end

  # Renewal timing for one grant, on the holder's own clock. RangeError unless
  # `ttl` is positive, `renew_after < ttl` and `warn_before <= ttl`.
  Cadence = Data.define(:ttl, :renew_after, :warn_before) do
    def initialize(ttl:, renew_after:, warn_before:)
      LeaseMachines.__send__(:native_cadence, ttl, renew_after, warn_before)
      super
    end
  end

  # Sealed: no subclass can skip the checks above by overriding `initialize`,
  # and no bare `allocate` builds a value that never ran them. Ruby still
  # binds a subclass constant before `inherited` raises; the native side
  # refuses its instances.
  [FencingToken, Lease, Checkpoint, Cadence].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end
end

# Native is required: no algorithm fallback. It resolves the classes above
# once, here.
require 'lease_machines_native/lease_machines_native'

module LeaseMachines
  class << self
    private :native_raw, :native_token_scope, :native_token_order, :native_token_newer, :native_lease,
            :native_cadence, :native_checkpoint
  end

  # A lease as the native side answers it, `[owner, raw, expires_at]`.
  def self.lease_of(row) = Lease.new(row[0], FencingToken.new(row[1]), row[2])
  private_class_method :lease_of

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
        native_restored(scope, checkpoint, clock_policy)
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
      Checkpoint.new(scope, issued, held && LeaseMachines.__send__(:lease_of, held), observed)
    end

    # Every decision decides on a copy of the authority, builds the Lease
    # (its `new` runs), then keeps the copy only if this authority is still
    # unfrozen (FrozenError) and unchanged (RuntimeError: a re-entrant call
    # changed it). Either way, a raise means nothing was applied.
    def acquire(owner, now, ttl) = native_acquire(owner, now, ttl)

    def reacquire(owner, now, ttl) = native_reacquire(owner, now, ttl)

    def renew(owner, token, now, ttl) = native_renew(owner, token, now, ttl)

    def release(owner, token, now) = native_release(owner, token, now)
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
      def resumed(scope, highest) = native_resumed(scope, highest)

      private :native_with_scope, :native_resumed
    end

    def highest
      raw = native_highest
      raw && FencingToken.new(raw)
    end

    def accept(token) = native_accept(token)
  end

  # The core's `Holder`: the renewal tracker on the side that holds the grant.
  class Holder
    class << self
      def new(token, confirmed_at, cadence) = native_new(token, confirmed_at, cadence)

      private :native_new
    end

    def token = FencingToken.new(native_token)

    def cadence = Cadence.new(*native_cadence)

    def confirm(token, at) = native_confirm(token, at)

    def reacquire(token, at) = native_reacquire(token, at)
  end
end
