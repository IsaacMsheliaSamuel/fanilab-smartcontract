# Changelog

All notable changes to FaniLab Smart Contracts will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Production-ready CI/CD pipeline with security audits
- CI: coverage upload now fails the build on error (`fail_ci_if_error: true`)
- CI: `cargo machete` step to detect unused dependencies automatically
- CI: `cargo outdated` step is now a hard gate (removed `continue-on-error`)
- `identity_reputation_contract::has_driver_profile` query function for driver existence checks
- `shared_types::ttl` constants (`LEDGER_TTL_THRESHOLD`, `LEDGER_TTL_EXTEND_TO`) now used by `delivery_contract`, `dispute_resolution_contract`, `identity_reputation_contract`, and `fleet_management_contract` instead of hand-typed `518400, 518400` literals at every `extend_ttl` call site
- `fleet_management_contract::confirm_fleet_treasury_update` and `get_pending_treasury_update` to support a timelocked treasury change flow

### Changed
- `escrow_contract::create_escrow` now validates `token` matches the protocol-configured token
- `fleet_management_contract::register_fleet` checks driver profile existence before calling `register_driver`, preventing panic for already-registered drivers
- `fleet_management_contract::update_fleet_treasury` now only *proposes* a treasury change; it takes effect only after a 3-day timelock and an explicit `confirm_fleet_treasury_update` call, giving active drivers advance notice via the new `fleet_treasury_change_proposed` event
- `settlement_contract` source moved from `src/lib.rs` to the flat `lib.rs` layout used by the other five contracts, for structural consistency
- Replaced the crate-level `#![allow(deprecated)]` in all six contracts with `#[allow(deprecated)]` scoped to the individual functions that call the deprecated `events().publish()`, so future unrelated deprecations are no longer silently suppressed
- Enhanced CI pipeline with linting and testing
- Improved error handling across all contracts
- Optimized storage TTL management
- **Upgraded Soroban SDK from 22.0.1 to 27.0.0** with full ecosystem compatibility
- **Updated WASM build target from `wasm32-unknown-unknown` to `wasm32v1-none`** for Soroban SDK 27.0.0 compatibility
- **Pinned Rust toolchain to 1.81.0** in CI workflows for consistent compilation across environments
- Added `#[allow(deprecated)]` annotations for SDK 27.0.0 `env.events().publish()` API deprecation (remains functional)

### Removed
- `escrow_contract::get_status` — dead stub that always returned `DeliveryStatus::Pending`. Use `get_escrow(id).status` instead.
- Comprehensive deployment documentation
- API reference documentation
- Security audit checklist
- Testing guide with coverage requirements
- Governance model documentation
- Issue and PR templates
- Automated dependency updates via Dependabot
- Code formatting standards (rustfmt.toml)
- License compliance checking (cargo-deny)
- Automated release workflow
- Settlement contract integration for currency swaps
- Two-step admin transfer process
- Dispute split resolution mechanism
- Driver reputation tracking system
- Delivery transit status tracking

### Security
- Added balance verification before transfers
- Implemented checks-effects-interactions pattern
- Enhanced access control on admin functions
- Added input validation on all public functions

## [0.2.0] - 2024-12-XX

### Added
- Delivery contract with full lifecycle management
- Escrow contract with dispute resolution
- Shared types library for cross-contract compatibility
- Event system for off-chain indexing
- Basic test coverage

### Fixed
- Storage key collision issues
- State transition validation bugs

## [0.1.0] - 2024-11-XX

### Added
- Initial project structure
- Basic escrow functionality
- Cargo workspace configuration
- README and contributing guidelines

---

## Release Process

1. Update CHANGELOG.md with changes
2. Update version in Cargo.toml files
3. Create git tag: `git tag -a v1.0.0 -m "Release v1.0.0"`
4. Push tag: `git push origin v1.0.0`
5. GitHub Actions will create release automatically

## Version Guidelines

- **Major (X.0.0)**: Breaking changes, major features
- **Minor (0.X.0)**: New features, non-breaking changes
- **Patch (0.0.X)**: Bug fixes, minor improvements
