# Changelog

## [0.4.1](https://github.com/seuros/lease_machines/compare/lease_machines/v0.4.0...lease_machines/v0.4.1) (2026-11-14)


### Bug Fixes

* **deps:** require matryona-machines &gt;= 0.3 and build exact-field messages lazily ([4b65322](https://github.com/seuros/lease_machines/commit/4b6532212f3356dbd70eb710724bac699da5c299))

## [0.4.0](https://github.com/seuros/lease_machines/compare/lease_machines/v0.3.0...lease_machines/v0.4.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* adopt matryona-machines 0.2: shared instant, exact field reads, compiler-checked symbol tables

### Features

* adopt matryona-machines 0.2: shared instant, exact field reads, compiler-checked symbol tables ([24fc8e1](https://github.com/seuros/lease_machines/commit/24fc8e10dad7079c101fe5388ec58a147d87b78b))

## [0.3.0](https://github.com/seuros/lease_machines/compare/lease_machines/v0.2.0...lease_machines/v0.3.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* validate every value through the core, seal values, exact-class reads, clone-then-keep decisions, restore the token niche

### Features

* validate every value through the core, seal values, exact-class reads, clone-then-keep decisions, restore the token niche ([690d5af](https://github.com/seuros/lease_machines/commit/690d5afaea91d7ce0cf3fdcf3a3720c8663d00ca))

## [0.2.0](https://github.com/seuros/lease_machines/compare/lease_machines/v0.1.6...lease_machines/v0.2.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* converted to the fleet conventions. One exhaustive Refusal per crate, Result from every operation that can refuse, a refusal changes nothing, provable idempotent repeats are honoured, Ruby mirrors the core. No compatibility aliases. Decision is gone; FencingToken has no Ord (use order / is_newer_than); authorities and fences are built for a scope; restored takes scope and clock policy; Ruby times are Integer nanoseconds and sibling values are read by their readers. Expired is an answer about the instant supplied and is not remembered.

### Features

* scope fencing tokens through periscope, answer Result&lt;_, Refusal&gt;, stop refusals moving the clock ([8d47108](https://github.com/seuros/lease_machines/commit/8d47108cec8fb514bb1b1d204f7590d2e066d1c3))

## [0.1.6](https://github.com/seuros/lease_machines/compare/lease_machines/v0.1.5...lease_machines/v0.1.6) (2026-11-06)


### Bug Fixes

* share lease renew/release checks in authority ([3c44626](https://github.com/seuros/lease_machines/commit/3c446267e004228b03df335e577f35b19b0c645f))

## [0.1.5](https://github.com/seuros/lease_machines/compare/lease_machines/v0.1.4...lease_machines/v0.1.5) (2026-11-06)


### Features

* rebuild authorities from storage and model the holder side ([5fe2199](https://github.com/seuros/lease_machines/commit/5fe2199af3efca8666f5015511649c802d95b77f))

## [0.1.4](https://github.com/seuros/lease_machines/compare/lease_machines/v0.1.3...lease_machines/v0.1.4) (2026-11-06)


### Bug Fixes

* crates.io keywords and categories for the core ([c4f147f](https://github.com/seuros/lease_machines/commit/c4f147f9cb2d69bac209d2d1c50e851f5137ff80))

## [0.1.3](https://github.com/seuros/lease_machines/compare/lease_machines/v0.1.2...lease_machines/v0.1.3) (2026-11-06)


### Bug Fixes

* keep test sources out of the packaged gem and crate ([6f0b979](https://github.com/seuros/lease_machines/commit/6f0b979351f1fe456d7ae922203f1ec69274d70d))

## [0.1.2](https://github.com/seuros/lease_machines/compare/lease_machines/v0.1.1...lease_machines/v0.1.2) (2026-11-06)


### Bug Fixes

* cross builds no longer compile for the host ([e7571a9](https://github.com/seuros/lease_machines/commit/e7571a988886534052055c05b31d97a686aaeca7))

## [0.1.1](https://github.com/seuros/lease_machines/compare/lease_machines-v0.1.0...lease_machines/v0.1.1) (2026-11-06)


### Features

* initial extraction ([243e8f6](https://github.com/seuros/lease_machines/commit/243e8f642f63cc2dd61a61524de0f45cd858ab96))
