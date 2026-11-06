# frozen_string_literal: true
require 'bundler/gem_tasks'
require 'rb_sys/extensiontask'
spec = Gem::Specification.load('lease_machines.gemspec')
SUPPORTED_NATIVE_PLATFORMS = %w[
  arm64-darwin
  x86_64-darwin
  aarch64-linux
  x86_64-linux
  x86_64-linux-musl
].freeze

native_ext = RbSys::ExtensionTask.new('lease_machines_native', spec) do |extension|
  extension.lib_dir = 'lib/lease_machines_native'
  extension.tmp_dir = 'tmp/rb_sys'
  extension.cross_platform = SUPPORTED_NATIVE_PLATFORMS if ENV.key?('RUBY_TARGET')
end

# A single-version cross build stages its binary at the host binary's path,
# which the gem package depends on, so packaging would also compile for the
# host and fail where the host linker can't target the platform.
if ENV.key?('RUBY_TARGET')
  host_binary = "#{native_ext.lib_dir}/#{native_ext.name}.#{RbConfig::CONFIG['DLEXT']}"
  Rake::Task[host_binary].clear_prerequisites if Rake::Task.task_defined?(host_binary)
end
desc 'Compile and run native Ruby boundary tests'
task test: :compile do
  sh 'padded_cell'
end
desc 'Test the standalone Rust core'
task :cargo do
  sh 'cargo', 'test', '-p', 'lease-machines'
end
desc 'Validate Ruby signatures'
task :rbs do
  sh 'rbs', '-I', 'sig', 'validate'
end
task default: %i[test cargo rbs]

Dir[File.join(__dir__, 'tasks/*.rake')].sort.each { |f| load f }
