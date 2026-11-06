# frozen_string_literal: true
require_relative 'lease_machines/version'
require 'lease_machines_native/lease_machines_native'

module LeaseMachines
  Decision = Data.define(:status, :lease, :reason)

  class Authority
    def acquire(owner:, now:, ttl:)
      Decision.new(*_acquire(owner, now, ttl))
    end

    def renew(owner:, token:, now:, ttl:)
      Decision.new(*_renew(owner, token, now, ttl))
    end

    def release(owner:, token:, now:)
      Decision.new(*_release(owner, token, now))
    end

    private :_acquire, :_renew, :_release
  end

  class Fence
    def self.new(highest: nil) = _new(highest)
    private_class_method :_new
  end
end
