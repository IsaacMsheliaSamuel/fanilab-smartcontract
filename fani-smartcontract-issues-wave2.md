# FaniLab Smart Contracts — Wave 2 Backlog (Issues #188–)

Authored for the Drips Stellar Wave. Every issue below was derived from a direct
read of the repository at commit `510af08` (`main`, immediately after PR #187
merged the Holdback refund authorization fix), cross-checked against the already
published backlog in `fani-smartcontract-issues.md` (GitHub issues #7–#144) and
against the live issue tracker at `github.com/fanilabs/fanilab-smartcontract`.

Numbering starts at **#188** because GitHub's shared issue/PR sequence is already
at #187; starting here keeps local numbers aligned with the numbers these issues
will receive when filed, matching the convention of the existing backlog document
(where each row links to `/issues/N`).

**Complexity is strictly Medium (150 points) or High (200 points). There are zero
Trivial issues in this document.**

Scope note: the HIGH-severity `refund_escrow` / `Holdback` authorization
vulnerability fixed in PR #187 is deliberately **not** reproduced here. Issues
that touch `Holdback` below concern *different* defects in the surrounding state
machine that the fix did not address.

---

# Issue #238 — Reentrancy is tested only for the settlement-swap path, leaving three other cross-contract call sites unverified

## Problem Statement

`escrow_contract/test.rs` contains one reentrancy test,
`test_release_escrow_rejects_reentrant_call_during_settlement_swap`, built on a
`MaliciousSettlementContract` whose `execute_settlement_swap` re-enters
`release_escrow`. It was added for issue #87's checks-effects-interactions work.

The escrow contract makes cross-contract calls at three other points, none of
which has an equivalent test:

- `payout_driver` → `fleet_management_contract::get_payout_address`
- `payout_driver` → `settlement_contract::get_driver_preference`
- `settle_escrow_funds` → the token contract's `transfer`, for both the driver payout and the platform fee

Each is a call to an address that is admin-configured or protocol-configured, and
each occurs after state has been committed.

## Why It Matters

The single existing test proves the pattern holds for one call site. It does not
prove it holds for the others, and the others are reachable under configurations
the protocol explicitly supports — a fleet contract is configured whenever fleet
routing is used, and `get_driver_preference` is called on every payout when a
settlement contract is set.

A malicious or compromised fleet-management contract returning a treasury address
whose token transfer re-enters the escrow is the concrete scenario. Because
`get_payout_address` is called *before* the transfer, a reentrant call at that
point sees committed state — which the checks-effects-interactions ordering is
designed to make safe, but nothing verifies it.

## Proposed Solution

Extend the existing malicious-mock approach to the other call sites: a malicious
fleet contract whose `get_payout_address` re-enters, and a malicious token whose
`transfer` re-enters, each attempting to double-release or refund the same
escrow.

Assert that the second entry is rejected by the state machine (the escrow is
already `Released`) and that no double payout occurs.

## Acceptance Criteria

- [ ] A reentrant call from `get_payout_address` cannot cause a double release or double refund
- [ ] A reentrant call from a token `transfer` cannot cause a double payout
- [ ] A reentrant call from `get_driver_preference` is covered
- [ ] Each test asserts final balances, not just that the call reverted
- [ ] `release_holdback_escrow` and `resolve_dispute` are covered as well as `release_escrow`
- [ ] The existing settlement-swap reentrancy test continues to pass

## Technical Notes

- `MaliciousSettlementContract` in `escrow_contract/test.rs` is the template.
- A malicious token requires a custom contract implementing the token interface rather than `register_stellar_asset_contract_v2`; scope this carefully, as a full token mock is non-trivial.
- The state guard doing the real work is the status check at the top of each fund-moving function — the tests should demonstrate that guard is what rejects reentry, so a future refactor that weakens it fails loudly.
- Fleet routing is reached only when the escrow has a `fleet_id` and a fleet contract is configured; see issue #227 for the configuration coverage this builds on.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `payout_driver`, `settle_escrow_funds`, `release_escrow`, `release_holdback_escrow`, `resolve_dispute`
- `contracts/escrow_contract/test.rs` — `MaliciousSettlementContract`, existing reentrancy test

## Testing Requirements

- Reentrancy test via a malicious fleet contract on `release_escrow`
- Reentrancy test via a malicious fleet contract on `release_holdback_escrow`
- Reentrancy test via a malicious token transfer, if a token mock is feasible
- Balance assertions proving no double payout in each case
- Regression test: the existing settlement-swap reentrancy test unchanged
- Edge case: reentry attempting `refund_escrow` rather than `release_escrow`

## Definition of Done

- [ ] Reentrancy coverage extended to the remaining cross-contract call sites
- [ ] Balance-level assertions in every case
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

1–2 days

## Dependencies

Builds naturally on #227's fleet configuration coverage, but can be written independently with its own mock setup.

## Labels

`test`


---

# Issue #239 — `escrow_contract` has no way to unset a configured fleet-management contract

## Problem Statement

`escrow_contract` provides `set_settlement_contract` (timelocked),
`confirm_settlement_contract`, and `clear_settlement_contract` — a complete
lifecycle for the settlement integration, with `clear_settlement_contract` added
for issue #90 specifically so a configured address could be unset.

The fleet-management integration has only `set_fleet_management_contract` and
`get_fleet_management_contract`. There is no clear or unset counterpart. The same
is true of `set_dispute_resolution_contract`.

Once a fleet-management contract is configured, it can be repointed but never
removed, so the payout path permanently retains the cross-contract call in
`payout_driver` for any escrow carrying a `fleet_id`.

## Why It Matters

The reason `clear_settlement_contract` was added applies identically here: if the
configured fleet contract becomes unavailable, buggy, or compromised, every
payout for a fleet-linked escrow routes through a cross-contract call to it.
`payout_driver` invokes `get_payout_address` and uses the returned address as the
transfer destination, so a misbehaving fleet contract can redirect driver
earnings, and the escrow admin's only remedy is to point at a different contract
rather than to disable the integration.

The asymmetry between the two integrations is itself a maintenance hazard — an
operator who knows `clear_settlement_contract` exists will reasonably assume the
fleet equivalent does too.

## Proposed Solution

Add `clear_fleet_management_contract(admin)` mirroring
`clear_settlement_contract`: admin-gated, removing the instance key so
`get_fleet_management_contract` returns `None` and `payout_driver` falls back to
paying the driver directly.

Consider the same for `set_dispute_resolution_contract`, weighing that clearing
it disables `freeze_funds` entirely — which may be a deliberate reason *not* to
offer it. Decide explicitly and document the reasoning either way.

## Acceptance Criteria

- [ ] `clear_fleet_management_contract` removes the configured address
- [ ] After clearing, `get_fleet_management_contract` returns `None`
- [ ] After clearing, payouts for escrows with a `fleet_id` go directly to the driver
- [ ] The function is admin-gated and rejects non-admin callers
- [ ] Clearing when nothing is configured succeeds without error, matching `clear_settlement_contract`'s behavior
- [ ] A decision on the dispute-contract equivalent is documented

## Technical Notes

- `clear_settlement_contract` is the direct template, including its behavior of also removing any pending change.
- `payout_driver` guards on `if let (Some(fleet_addr), Some(fid))`, so a cleared contract naturally falls through to the direct transfer with no further change needed.
- Existing test `test_clear_nonexistent_settlement_contract_succeeds` establishes the no-op-when-absent expectation to mirror.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `set_fleet_management_contract`, `get_fleet_management_contract`, `clear_settlement_contract`, `payout_driver`
- `contracts/escrow_contract/test.rs` — `test_clear_settlement_contract_*` as templates
- `docs/API.md`

## Testing Requirements

- Unit test: set then clear → getter returns `None`
- Unit test: clear when nothing configured → succeeds
- Authorization test: non-admin cannot clear
- Behavioral test: after clearing, a fleet-linked escrow pays the driver directly
- Regression test: settlement-contract clearing behavior unchanged

## Definition of Done

- [ ] Clear function added and documented in `docs/API.md`
- [ ] Dispute-contract decision documented
- [ ] Tests above added and passing
- [ ] Formatting and clippy clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Related to #227's fleet-routing coverage; either can land first.

## Labels

`enhancement`


---

# Issue #240 — `get_driver_tier`'s Silver threshold is a bare literal while Gold uses a named constant

## Problem Statement

`identity_reputation_contract` names one tier threshold and inlines the other:

```rust
const GOLD_TIER_THRESHOLD: u32 = 75;
const ENTERPRISE_THRESHOLD: u32 = GOLD_TIER_THRESHOLD;

pub fn get_driver_tier(env: Env, driver: Address) -> DriverTier {
    let score = profile.reputation_score;
    if score >= GOLD_TIER_THRESHOLD {
        DriverTier::Gold
    } else if score >= 50 {          // <-- bare literal
        DriverTier::Silver
    } else {
        DriverTier::Bronze
    }
}
```

The literal `50` is also the value `register_driver` assigns as a new driver's
starting `reputation_score` — so every newly registered driver begins exactly at
the Silver boundary. That coupling is invisible: the two `50`s are unrelated
literals that happen to match.

Issue #106 addressed the Gold/Enterprise duplication by introducing the named
constant and deriving `ENTERPRISE_THRESHOLD` from it. The Silver threshold and
the starting score were not included.

## Why It Matters

Changing the starting reputation score without changing the Silver threshold — or
vice versa — silently reclassifies every driver in the protocol. A new driver
currently starts as Silver, which is a meaningful policy decision that is nowhere
stated; it emerges from two independent literals coinciding.

Tier affects `is_eligible_for_enterprise` and is intended to affect driver
assignment, so a silent reclassification has real consequences. The risk is
latent rather than active — nothing is wrong today — which is why this is Medium.

## Proposed Solution

Introduce `SILVER_TIER_THRESHOLD` and `INITIAL_REPUTATION_SCORE` as named
constants, use them in `get_driver_tier` and `register_driver`, and make the
relationship between them explicit — either by deriving one from the other or by
documenting in a comment that a new driver is intended to start at Silver.

If the tier thresholds should be admin-configurable alongside the reputation
point values that issue #105 made configurable, note that as a follow-up rather
than expanding this issue.

## Acceptance Criteria

- [ ] The Silver threshold is a named constant used by `get_driver_tier`
- [ ] The initial reputation score is a named constant used by `register_driver`
- [ ] The intended relationship between the two is explicit in code or comment
- [ ] Tier boundaries are unchanged in behavior
- [ ] Regression test asserts tier classification at every boundary value

## Technical Notes

- `MAX_REPUTATION` is 100 and `GOLD_TIER_THRESHOLD` is 75; the constants should be internally consistent with both.
- `get_driver_tier` panics via `get_driver_profile` for an unregistered driver — that behavior is out of scope here but worth an assertion in the tests.
- Existing tier tests, if any, should be extended to cover exact boundary scores (49/50/74/75) rather than only mid-range values.

## Relevant Files

- `contracts/identity_reputation_contract/lib.rs` — `get_driver_tier`, `register_driver`, tier constants
- `contracts/identity_reputation_contract/test.rs`

## Testing Requirements

- Unit test: score 49 → Bronze, 50 → Silver, 74 → Silver, 75 → Gold
- Unit test: a newly registered driver's tier matches the documented intent
- Unit test: `is_eligible_for_enterprise` agrees with `get_driver_tier` at the Gold boundary
- Regression test: existing tier behavior unchanged
- Edge case: score 0 and score `MAX_REPUTATION`

## Definition of Done

- [ ] Named constants introduced and used
- [ ] Relationship documented
- [ ] Boundary tests added and passing
- [ ] Formatting and clippy clean

## Complexity

**Medium**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`refactor`


---

# Issue #241 — The testnet deploy workflow can never authenticate: the `deployer` identity is never created

## Problem Statement

Every deployment and initialization script signs with a named Stellar CLI
identity:

```bash
--source deployer
```

This appears in `scripts/deploy-all-contracts.sh` (line 70),
`scripts/deploy-contract.sh` (line 91), `scripts/initialize-all-contracts.sh`
(lines 44, 58), and `scripts/initialize-contract.sh` (lines 64, 95).

Nothing ever creates that identity. Grepping the entire `scripts/` directory and
all four workflows for `stellar keys add`, `stellar keys generate`, or any other
identity provisioning returns no matches.

`deploy-testnet.yml` supplies the key material under a third name entirely:

```yaml
env:
  CONTRACT_DEPLOYER_SECRET: ${{ secrets.TESTNET_DEPLOYER_SECRET }}
```

No script reads `CONTRACT_DEPLOYER_SECRET`. Separately, `.env.example` documents
the variable as `CONTRACT_DEPLOYER_KEY`. Three different names refer to the same
credential, and the one the scripts actually consume — a CLI identity called
`deployer` — is never provisioned anywhere.

## Why It Matters

The "Deploy to Testnet" workflow cannot succeed. The Stellar CLI will fail to
resolve `--source deployer` because no such identity exists in the runner's
freshly created config directory, so every deployment attempt aborts at the first
`stellar contract deploy` call.

Because the workflow is `workflow_dispatch`-only it is not exercised on every
push, which is why the break has gone unnoticed. Any contributor or maintainer
attempting a testnet deployment hits it immediately, and the failure message
points at the CLI rather than at the missing provisioning step.

## Proposed Solution

Add an identity-provisioning step to the workflow before the deploy step, using
the secret it already passes:

```yaml
- name: Configure deployer identity
  run: stellar keys add deployer --secret-key "$CONTRACT_DEPLOYER_SECRET"
```

Then settle on one name for the credential across the workflow, the scripts, and
`.env.example` so the three no longer disagree. Have the scripts fail early with
a clear message if the identity is absent, rather than surfacing a raw CLI error.

Note this is distinct from the already-closed issue #60, which concerned
`.env.example` documenting variables the scripts did not use; this issue is the
missing provisioning step that makes the workflow non-functional.

## Acceptance Criteria

- [ ] The deploy workflow provisions the `deployer` identity before invoking any script
- [ ] The credential uses a single consistent name across the workflow, scripts, and `.env.example`
- [ ] Scripts fail with a clear, actionable message when the identity is missing
- [ ] The secret is never echoed into logs
- [ ] A dry-run or documented manual verification confirms the workflow reaches the deploy step
- [ ] Local (non-CI) usage of the scripts continues to work with a manually configured identity

## Technical Notes

- `stellar keys add <name> --secret-key <S...>` is the provisioning command for the CLI version installed by the workflow (`cargo install --locked stellar-cli --features opt`).
- The workflow already declares `environment: testnet`, so the secret is scoped to that environment.
- Secrets must be passed via `env:` rather than interpolated into `run:` strings to avoid log exposure.
- `scripts/initialize-*.sh` share the same `--source deployer` assumption, so the fix must cover initialization as well as deployment.

## Relevant Files

- `.github/workflows/deploy-testnet.yml` — deploy step and secret wiring
- `scripts/deploy-all-contracts.sh`, `scripts/deploy-contract.sh`
- `scripts/initialize-all-contracts.sh`, `scripts/initialize-contract.sh`
- `.env.example`
- `docs/DEPLOYMENT.md`

## Testing Requirements

- Verification: the workflow reaches and passes the identity-configuration step
- Verification: a deploy attempt with a missing/blank secret fails with the scripts' clear message, not a raw CLI error
- Verification: no secret material appears in workflow logs
- Regression: local script usage with a pre-configured identity is unaffected
- Documentation check: `.env.example` and `docs/DEPLOYMENT.md` name the same variable

## Definition of Done

- [ ] Identity provisioning added to the workflow
- [ ] Credential naming unified across workflow, scripts, and `.env.example`
- [ ] Early failure with a clear message implemented
- [ ] `docs/DEPLOYMENT.md` updated to match

## Complexity

**High**

## Estimated Effort

4–8 hours

## Dependencies

**None**

## Labels

`bug`


---

# Issue #242 — Release builds omit `--locked`, so published artifacts may not match the tested dependency set

## Problem Statement

`ci.yml` uses `--locked` on every Cargo invocation:

```yaml
run: cargo clippy --locked --all-targets --all-features --target wasm32v1-none -- -D warnings
run: cargo build --locked --target wasm32v1-none --release
run: cargo test --locked --verbose
```

`release.yml` does not:

```yaml
- name: Build Release Contracts
  run: cargo build --target wasm32v1-none --release
```

`deploy-testnet.yml` has the same omission in its `Build Contracts` step.

Without `--locked`, Cargo is free to update `Cargo.lock` during the build and
resolve dependencies that differ from those CI validated.

## Why It Matters

The artifacts published by `release.yml` are the WASM binaries users deploy on
chain, and their SHA256 checksums are recorded in the release notes as the
authoritative reference. If the release build resolves a different dependency set
than CI tested, the published binaries were never exercised by the test suite.

The project has already had `Cargo.lock` pinning fire-drills — closed issue #63
introduced `--locked` in CI for exactly this reason. The release path, which is
the one that actually ships bytes to users, was left out.

This also undermines build reproducibility: rebuilding the same tag later can
produce different checksums.

## Proposed Solution

Add `--locked` to the Cargo invocations in `release.yml` and
`deploy-testnet.yml` so every build path resolves the committed lockfile
exactly, matching CI.

## Acceptance Criteria

- [ ] `release.yml`'s build step uses `--locked`
- [ ] `deploy-testnet.yml`'s build step uses `--locked`
- [ ] A release build fails if `Cargo.lock` is out of date rather than silently updating it
- [ ] Existing release artifacts build successfully with the flag applied
- [ ] All Cargo invocations across all four workflows are consistent

## Technical Notes

- `cargo install --locked stellar-cli` in `deploy-testnet.yml` already uses the flag, so the inconsistency is confined to the project's own builds.
- `--locked` fails the build when the lockfile would need updating, which is the desired signal.
- Verify `Cargo.lock` is committed and current before enabling, otherwise the first release after this change will fail.

## Relevant Files

- `.github/workflows/release.yml` — `Build Release Contracts`
- `.github/workflows/deploy-testnet.yml` — `Build Contracts`
- `.github/workflows/ci.yml` — reference for the established pattern
- `Cargo.lock`

## Testing Requirements

- Verification: release workflow succeeds with `--locked` on the current lockfile
- Verification: the build fails when `Cargo.lock` is deliberately made stale
- Regression: produced artifacts are byte-identical to a `--locked` local build
- Consistency check: every Cargo invocation across all workflows uses the flag

## Definition of Done

- [ ] `--locked` applied to both workflows
- [ ] Verified failure behavior on a stale lockfile
- [ ] All workflows consistent

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`bug`, `security`


---

# Issue #243 — `release.yml` pins a third-party action by mutable tag while the toolchain is SHA-pinned

## Problem Statement

`release.yml` pins the Rust toolchain to an immutable commit SHA:

```yaml
uses: dtolnay/rust-toolchain@4cda84d5c5c54efe2404f9d843567869ab1699d4 # stable
```

but pins the action that publishes the release to a mutable major-version tag:

```yaml
uses: softprops/action-gh-release@v1
```

The `v1` tag can be repointed by its maintainer at any commit. This is the action
that receives `GITHUB_TOKEN` and uploads the WASM binaries and their checksums.

Closed issue #138 addressed deprecated third-party action versions, and CI was
updated accordingly (it now uses `codecov-action@v5` and `upload-artifact@v4`),
but `release.yml`'s release action was not included.

## Why It Matters

This is the highest-privilege third-party action in the repository: it runs with
a token that can write to repository contents and it publishes the exact binaries
users are told to trust by checksum. A compromised or repointed `v1` tag could
alter published artifacts or exfiltrate the token, and nothing in the workflow
would detect it.

Pinning the toolchain by SHA while leaving the publishing action floating is an
inconsistency that undercuts the supply-chain posture the SHA pin was adopted to
establish. `softprops/action-gh-release` is also several major versions past
`v1`, so the pin is stale as well as mutable.

## Proposed Solution

Pin `softprops/action-gh-release` to a specific commit SHA with a trailing
version comment, matching the `dtolnay/rust-toolchain` style already used in the
same file. Move to a current major version at the same time.

Audit the remaining `uses:` entries across all four workflows
(`actions/checkout`, `Swatinem/rust-cache`, `actions/upload-artifact`,
`codecov/codecov-action`) and apply a consistent pinning policy, documenting that
policy so future additions follow it.

## Acceptance Criteria

- [ ] `softprops/action-gh-release` is pinned to a commit SHA with a version comment
- [ ] The pinned version is a currently supported major release
- [ ] All third-party actions across the four workflows follow one documented pinning policy
- [ ] The release workflow still publishes artifacts and checksums correctly
- [ ] The pinning policy is written down for future contributors

## Technical Notes

- `dtolnay/rust-toolchain@4cda84d… # stable` is the in-repo pattern to follow.
- Dependabot can keep SHA pins updated; check `.github/dependabot.yml` covers the `github-actions` ecosystem so pins do not go stale silently.
- First-party `actions/*` entries are lower risk but should still follow whatever policy is documented, for consistency.

## Relevant Files

- `.github/workflows/release.yml`
- `.github/workflows/ci.yml`, `deploy-testnet.yml`, `security-audit.yml`
- `.github/dependabot.yml`
- `CONTRIBUTING.md` or `SECURITY.md` — wherever the policy is recorded

## Testing Requirements

- Verification: a tagged release still builds, uploads artifacts, and attaches checksums
- Verification: every `uses:` entry conforms to the documented policy
- Verification: Dependabot is configured to update the pinned actions
- Regression: no change to release artifact contents

## Definition of Done

- [ ] Release action SHA-pinned and version-current
- [ ] Pinning policy applied consistently and documented
- [ ] A test release verified end to end

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`security`


---

# Issue #244 — `release.yml` declares no `permissions` block and relies on default token scope

## Problem Statement

The `create_release` job in `release.yml` has no `permissions:` key at either the
workflow or job level. It passes the default token to the publishing step:

```yaml
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

With no explicit declaration, the job inherits whatever the repository or
organization default is. That default is either read-only — in which case
creating a release fails — or read-write across every scope, in which case the
job holds far more authority than publishing a release requires.

None of the other three workflows declare `permissions` either.

## Why It Matters

The failure mode is not hypothetical in either direction. If the org default is
read-only (increasingly the recommended setting), releases break at the publish
step with a permissions error that is unobvious to diagnose. If the default is
permissive, a workflow that only needs to write releases can also write code,
issues, packages, and workflow files — so any compromise of a third-party action
in that job (see issue #243) inherits the full set.

Declaring least-privilege permissions explicitly makes the workflow correct under
either default and bounds the blast radius.

## Proposed Solution

Add an explicit minimal `permissions` block to the release job:

```yaml
permissions:
  contents: write
```

Audit the other three workflows and declare their minimal needs too —
`deploy-testnet.yml` needs `contents: read`, `ci.yml` needs `contents: read`
(plus whatever Codecov upload requires), and `security-audit.yml` needs
`contents: read`.

## Acceptance Criteria

- [ ] `release.yml` declares an explicit minimal `permissions` block
- [ ] The release job succeeds regardless of the repository default token setting
- [ ] The other three workflows declare their minimal permissions
- [ ] No workflow requests a scope it does not use
- [ ] A tagged release is verified end to end after the change

## Technical Notes

- `contents: write` is the minimum for `softprops/action-gh-release` to create a release and upload assets.
- Declaring `permissions` at the job level overrides the workflow default and is the more precise placement.
- Codecov upload in `ci.yml` may need additional scope depending on how the token is supplied — verify rather than assuming.

## Relevant Files

- `.github/workflows/release.yml`
- `.github/workflows/ci.yml`, `deploy-testnet.yml`, `security-audit.yml`

## Testing Requirements

- Verification: release succeeds with the explicit block in place
- Verification: each workflow still completes with its reduced scope
- Verification: no workflow declares a scope it does not exercise
- Regression: artifact upload and checksum attachment unchanged

## Definition of Done

- [ ] Minimal `permissions` declared in all four workflows
- [ ] Release verified end to end
- [ ] Rationale noted in the workflow or in `SECURITY.md`

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Best sequenced alongside #243, since both harden the same job.

## Labels

`security`


---

# Issue #245 — Release notes hardcode the contract list, which will drift as contracts are added or removed

## Problem Statement

`release.yml` builds its release notes by echoing a fixed list:

```yaml
echo "## Contracts" >> RELEASE_NOTES.md
echo "- Escrow Contract" >> RELEASE_NOTES.md
echo "- Delivery Contract" >> RELEASE_NOTES.md
echo "- Dispute Resolution Contract" >> RELEASE_NOTES.md
echo "- Fleet Management Contract" >> RELEASE_NOTES.md
echo "- Identity Reputation Contract" >> RELEASE_NOTES.md
echo "- Settlement Contract" >> RELEASE_NOTES.md
```

The list is maintained by hand and has no connection to what was actually built.
The workspace resolves members from `contracts/*`, so adding or removing a
contract changes the artifacts without changing these lines.

## Why It Matters

The release notes are the user-facing description of what a release contains, and
they sit directly above a checksum block that *is* generated from the real
artifacts. A drifted list produces notes that contradict the checksums in the
same document — listing a contract with no corresponding binary, or omitting one
that shipped.

The consequence is confusion rather than breakage, which is why this is Trivial,
but it is a real inconsistency in published output and the fix is small and
self-contained.

## Proposed Solution

Generate the contract list from the built artifacts, the same source the checksum
step already uses:

```bash
for f in release_artifacts/*.wasm; do
  echo "- $(basename "$f" .wasm)" >> RELEASE_NOTES.md
done
```

Optionally map crate names to display names via a small lookup if the prettier
titles are worth preserving, but deriving the list from reality is the point.

## Acceptance Criteria

- [ ] The contract list in release notes is derived from the built artifacts
- [ ] Adding or removing a workspace member is reflected without editing the workflow
- [ ] The list and the checksum block always describe the same set of artifacts
- [ ] Release notes remain readable and correctly formatted
- [ ] A test release produces correct notes

## Technical Notes

- `release_artifacts/` is already populated by the preceding step and is the natural source.
- The checksum step already iterates the same directory, so the two will stay consistent by construction.
- Crate names are snake_case (`escrow_contract`); decide whether to prettify or use them verbatim.

## Relevant Files

- `.github/workflows/release.yml` — `Create Release Notes` step

## Testing Requirements

- Verification: generated notes list exactly the built artifacts
- Verification: notes and checksums agree on the artifact set
- Verification: markdown renders correctly in a GitHub release
- Edge case: a workspace with one contract added produces updated notes with no workflow edit

## Definition of Done

- [ ] Contract list generated from artifacts
- [ ] Verified on a test release
- [ ] Formatting preserved

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`bug`


---

# Issue #246 — No check that a release tag matches the version declared in the crate manifests

## Problem Statement

`release.yml` triggers on any tag matching `v*` and derives the release title
directly from the ref:

```yaml
on:
  push:
    tags:
      - 'v*'
...
echo "# FaniLab Smart Contracts ${GITHUB_REF#refs/tags/}" > RELEASE_NOTES.md
```

Nothing compares that tag to the `version` field in the contract manifests. All
six crates currently declare `version = "0.2.0"`, so pushing a tag `v0.9.0` would
publish artifacts titled `v0.9.0` built from crates that identify themselves as
`0.2.0`.

The escrow contract also exposes an on-chain `get_protocol_version()` returning
`constants::PROTOCOL_VERSION` (currently `1`), which is a third version identifier
with no relationship to either.

## Why It Matters

The published release title is what users cite when reporting issues or pinning a
deployment, and the artifacts carry no embedded version to cross-check against.
A mismatch is silent and only discoverable by decompiling or by reading the source
at the tagged commit.

The project has a documented history of version confusion — closed issue #128
covered `README.md` and `SECURITY.md` claiming 0.2.x while the manifests still
said 0.1.0. A CI guard converts that class of mistake from "discovered later by a
user" into "release fails immediately".

## Proposed Solution

Add a validation step early in `release.yml` that extracts the tag version,
compares it to the workspace crates' declared `version`, and fails the workflow on
a mismatch before anything is built or published.

Decide explicitly how `PROTOCOL_VERSION` relates to the crate version — they are
independent today, and that is defensible — and document the relationship so the
three identifiers are no longer ambiguous.

## Acceptance Criteria

- [ ] The release workflow fails when the tag version does not match the crate version
- [ ] The check runs before the build and publish steps
- [ ] The failure message names both the tag and the manifest version
- [ ] A matching tag proceeds normally through the existing steps
- [ ] The relationship between the crate version and `PROTOCOL_VERSION` is documented

## Technical Notes

- All six crates declare `version` independently in `contracts/*/Cargo.toml`; the check should confirm they agree with each other as well as with the tag.
- `cargo metadata --no-deps --format-version 1` is a reliable way to read versions without hand-parsing TOML.
- Tags are `v`-prefixed (`v0.2.0`) while manifests are not (`0.2.0`) — strip the prefix before comparing.
- `escrow_contract::constants::PROTOCOL_VERSION` is a separate on-chain concept; do not conflate them without a deliberate decision.

## Relevant Files

- `.github/workflows/release.yml`
- `contracts/*/Cargo.toml` — the six `version` declarations
- `contracts/escrow_contract/lib.rs` — `constants::PROTOCOL_VERSION`, `get_protocol_version`
- `CHANGELOG.md`

## Testing Requirements

- Verification: a mismatched tag fails the workflow with a clear message
- Verification: a matching tag proceeds to build and publish
- Verification: crates disagreeing with each other is also caught
- Edge case: a tag with a pre-release suffix (e.g. `v0.3.0-rc1`) behaves as documented
- Regression: existing release behavior unchanged for a correct tag

## Definition of Done

- [ ] Version-consistency check added and failing correctly
- [ ] Version identifier relationships documented
- [ ] Verified on both matching and mismatched tags

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`enhancement`, `security`


---

# Issue #247 — `deploy-testnet.yml` uploads an artifact pattern no script ever produces

## Problem Statement

The final step of `deploy-testnet.yml` collects two path patterns:

```yaml
- name: Save Deployment Artifacts
  uses: actions/upload-artifact@v4
  with:
    name: deployment-info
    path: |
      contract-ids-*.json
      deployment-*.json
```

Only the first matches anything. Both `scripts/deploy-all-contracts.sh` (line 11)
and `scripts/deploy-contract.sh` (line 13) write to:

```bash
OUTPUT_FILE="$PROJECT_ROOT/contract-ids-$NETWORK.json"
```

No script in the repository writes a `deployment-*.json` file. The pattern is
inert.

Note this is not the already-closed issue #139, which reported that the artifact
patterns matched *nothing* — the `contract-ids-*.json` pattern was fixed and now
matches correctly. The leftover `deployment-*.json` line is the residue.

## Why It Matters

`actions/upload-artifact@v4` warns on patterns that match no files but does not
fail the step when at least one other pattern matches, so the dead line is
silently tolerated. Its presence implies a second deployment record exists,
which misleads anyone reading the workflow to understand what deployment produces.

The cost is low, which is why this is Trivial — but leaving a pattern that
documents a non-existent output is exactly the kind of small inconsistency that
accumulates into workflows nobody trusts.

## Proposed Solution

Either remove the `deployment-*.json` pattern, or — if a richer deployment record
was intended — have the scripts emit one and keep the pattern. Removal is the
smaller and more honest change unless there is a concrete need for the second
file.

While editing, confirm `if-no-files-found` is set appropriately so a genuinely
empty upload surfaces as a failure rather than a warning.

## Acceptance Criteria

- [ ] The workflow uploads only patterns that scripts actually produce
- [ ] `contract-ids-*.json` continues to be captured
- [ ] `if-no-files-found` is configured so a missing artifact is not silently ignored
- [ ] A deployment run produces a complete, correctly named artifact bundle
- [ ] No other workflow references the removed pattern

## Technical Notes

- `actions/upload-artifact@v4` supports `if-no-files-found: error | warn | ignore`; the default is `warn`.
- `$NETWORK` resolves to `testnet` in this workflow, so the concrete filename is `contract-ids-testnet.json`.
- `scripts/deploy-contract.sh` writes the same filename as the all-contracts script, so a single-contract deploy overwrites the combined record — worth noting during the fix, though addressing it is out of scope here.

## Relevant Files

- `.github/workflows/deploy-testnet.yml` — `Save Deployment Artifacts`
- `scripts/deploy-all-contracts.sh`, `scripts/deploy-contract.sh` — `OUTPUT_FILE`

## Testing Requirements

- Verification: a deploy run uploads the contract-ids artifact successfully
- Verification: no warning about unmatched patterns appears in the log
- Verification: the step fails if no artifact is produced at all
- Regression: artifact name and contents unchanged

## Definition of Done

- [ ] Dead pattern removed or backed by a real output
- [ ] `if-no-files-found` configured deliberately
- [ ] Verified on a deployment run

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Practically verifiable only once #241 makes the workflow able to reach this step.

## Labels

`bug`


---

# Issue #248 — The TypeScript SDK is never built or tested by any CI workflow

## Problem Statement

`sdk/typescript` is a full npm package: `package.json` declares `build` (`tsc`)
and `test` (`jest`) scripts, a `tsconfig.json` is present, and there is a `src/`
tree with clients, types, and an examples directory.

No workflow touches it. Grepping all four files in `.github/workflows/` for
`node`, `npm`, or `sdk/` returns no matches. Every job is Cargo-only.

## Why It Matters

Nothing verifies the SDK compiles. A TypeScript error, a broken import, or a type
that no longer matches the contract ABI can be merged without any signal — and
issue #223 documents exactly that kind of drift already present (`EscrowStatus`
missing `Paused`).

The package declares `"version": "1.0.0"` and a `prepublish` build hook, so it is
presented as publishable. Publishing an unbuilt, untested package is a real
failure mode, and there is currently no gate that would prevent it.

This is the enabling gap behind several SDK issues in this backlog: without CI,
SDK correctness depends entirely on manual discipline.

## Proposed Solution

Add a CI job that installs dependencies, type-checks, and builds the SDK. Wire
the test script in once tests exist (issue #249), or have the job run
`tsc --noEmit` plus `npm run build` in the interim.

Scope the job to run only when `sdk/**` changes if CI time is a concern, though
running it always is simpler and the package is small.

## Acceptance Criteria

- [ ] A CI job installs SDK dependencies and builds the package
- [ ] A TypeScript compilation error fails the build
- [ ] The job runs on pull requests affecting the SDK
- [ ] The Node version used matches the `engines` field (`>=18.0.0`)
- [ ] The existing Rust jobs are unaffected

## Technical Notes

- `package.json` has no lockfile committed alongside it — check whether `package-lock.json` should be added so CI installs are reproducible, mirroring the `--locked` policy applied to Cargo.
- `actions/setup-node` is the standard action; pin it per whatever policy issue #243 establishes.
- `examples/basic-usage.ts` should be included in type-checking so documented usage cannot silently break.

## Relevant Files

- `.github/workflows/ci.yml` — where the job would live
- `sdk/typescript/package.json`, `tsconfig.json`
- `sdk/typescript/src/**`, `sdk/typescript/examples/basic-usage.ts`

## Testing Requirements

- Verification: the job fails when a deliberate type error is introduced
- Verification: the job passes on the current SDK source
- Verification: `examples/basic-usage.ts` is type-checked
- Regression: Rust CI jobs unchanged in behavior and runtime

## Definition of Done

- [ ] SDK build job added to CI
- [ ] Verified to fail on a type error
- [ ] Node version aligned with `engines`
- [ ] Lockfile decision made and documented

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`test`, `enhancement`


---

# Issue #249 — The SDK declares a `jest` test script with no jest configuration and no test files

## Problem Statement

`sdk/typescript/package.json` declares a test entry point and the full jest
toolchain:

```json
"scripts": { "test": "jest" },
"devDependencies": {
  "@types/jest": "^29.0.0",
  "jest": "^29.0.0",
  "ts-jest": "^29.0.0",
  ...
}
```

There is no `jest.config.js`, `jest.config.ts`, or `jest` key in `package.json`,
and no test files exist anywhere under `sdk/typescript` — searching for
`*.test.ts` and `*.spec.ts` returns nothing.

Running `npm test` therefore fails: jest exits non-zero when it finds no tests,
and without `ts-jest` configured it would not transform TypeScript even if tests
were added.

## Why It Matters

The package advertises a working test command that does not work. A contributor
adding their first SDK test has to discover and set up the transform
configuration before their test can run, which is friction placed exactly where
the project most wants contributions — the SDK is entirely stubbed (issue #222)
and needs test coverage as it is implemented.

It also means the declared jest dependencies are currently dead weight in the
dependency tree.

## Proposed Solution

Add a minimal `ts-jest` configuration so `npm test` runs, and add at least one
meaningful test so the command exits successfully and the setup is proven. The
type-parity check proposed in issue #223 is a good first test: it needs no
network access and guards a real invariant.

Configure `passWithNoTests` deliberately rather than by accident — preferring at
least one real test over suppressing the empty-suite failure.

## Acceptance Criteria

- [ ] `npm test` runs successfully in `sdk/typescript`
- [ ] `ts-jest` is configured so TypeScript test files are transformed
- [ ] At least one meaningful test exists and passes
- [ ] The test command is suitable for use in CI
- [ ] Declared jest devDependencies are all actually used

## Technical Notes

- `ts-jest` requires either a `preset: 'ts-jest'` entry or an explicit `transform` mapping; the preset is simpler for a package this size.
- `tsconfig.json` already exists and can be referenced by the jest config rather than duplicating compiler options.
- Coordinate with issue #248 so the CI job runs this command once it works.
- Avoid mandating a particular test framework migration — jest is already the declared choice.

## Relevant Files

- `sdk/typescript/package.json` — `scripts.test`, devDependencies
- `sdk/typescript/tsconfig.json`
- `sdk/typescript/src/**` — subject under test
- New: jest configuration and an initial test file

## Testing Requirements

- Verification: `npm test` exits zero with at least one passing test
- Verification: a deliberately failing test causes a non-zero exit
- Verification: TypeScript test files are transformed correctly
- Verification: the command works in a clean checkout after `npm install`

## Definition of Done

- [ ] jest configured and working
- [ ] Initial meaningful test added and passing
- [ ] Command verified from a clean install
- [ ] Ready to be wired into CI

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Complements #248 (CI job) and #223 (the proposed first test); each is independently landable.

## Labels

`test`


---

# Issue #250 — `docs/API.md` omits 13 public functions of `escrow_contract`, including all volume-tier and fleet configuration

## Problem Statement

`escrow_contract` exposes 41 public functions. Thirteen have no entry anywhere in
`docs/API.md`:

```
clear_settlement_contract      confirm_settlement_contract
get_dispute_resolution_contract  get_fleet_management_contract
get_pending_settlement_contract  get_sender_volume
get_slippage_tolerance           get_total_locked
get_volume_tiers                 set_dispute_resolution_contract
set_fleet_management_contract    set_volume_tiers
update_slippage_tolerance
```

The document has a populated `## Escrow Contract` section with Initialization,
Admin Operations, Escrow Lifecycle, and Query Functions subsections — so this is
not a missing-section problem (the concern of open issue #64, which reported the
table of contents promising contracts it never documented; all six contracts now
have sections).

## Why It Matters

The omissions are concentrated in the contract's configuration surface — exactly
what an integrator or operator needs. `set_volume_tiers` governs fee discounts,
`set_fleet_management_contract` governs payout routing, and the settlement
timelock trio (`confirm_settlement_contract`, `get_pending_settlement_contract`,
`clear_settlement_contract`) governs a deliberate three-day security control that
is invisible to anyone reading the reference.

`get_total_locked` is named in `docs/MONITORING.md` as a key metric but has no API
entry describing how to call it.

## Proposed Solution

Document each missing function in the existing `## Escrow Contract` section,
following the established entry format used by the functions already documented:
a one-line description, **Parameters**, **Authorization**, **Errors**, **Events**,
and an **Example** block.

Place each under the appropriate existing subsection — configuration setters
under Admin Operations, read-only accessors under Query Functions — rather than
introducing new structure.

## Acceptance Criteria

- [ ] All 13 listed functions have entries in `docs/API.md`
- [ ] Each entry follows the existing format used by already-documented functions
- [ ] Parameter names and types match the contract signatures exactly
- [ ] Authorization requirements are stated correctly (admin-gated vs open)
- [ ] The settlement timelock behavior is explained where the three related functions are documented
- [ ] Entries are placed in the appropriate existing subsections

## Technical Notes

- `set_settlement_contract` is already documented but describes a *proposal* under timelock; the confirm/get-pending/clear trio completes that story and should cross-reference it.
- `get_sender_volume` and `get_volume_tiers` are read-only and unauthenticated; `set_volume_tiers` and `update_slippage_tolerance` require admin.
- Verify each signature against `contracts/escrow_contract/lib.rs` rather than inferring from the name.
- Issue #229 covers the separate design document; this issue is scoped to the API reference only.

## Relevant Files

- `docs/API.md` — `## Escrow Contract` section
- `contracts/escrow_contract/lib.rs` — authoritative signatures

## Testing Requirements

Documentation change; verification is by review against the source:

- [ ] Each documented signature checked against the contract during review
- [ ] Authorization claims checked against the actual `require_admin` / `require_auth` calls
- [ ] Error lists checked against the `panic_with_error!` sites in each function
- [ ] Examples are syntactically plausible against the real signatures

## Definition of Done

- [ ] All 13 functions documented
- [ ] Format consistent with the rest of the section
- [ ] Signatures and authorization verified against source

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

**None**

## Labels

`documentation`


---

# Issue #251 — `docs/API.md` omits 7 public functions of `dispute_resolution_contract`, including the entire forced-resolution mechanism

## Problem Statement

`dispute_resolution_contract` exposes 22 public functions. Seven are absent from
`docs/API.md` despite the document having a populated
`## Dispute Resolution Contract` section:

```
force_resolve_dispute            get_dispute_reputation_penalty
get_dispute_resolution_limit     list_admins
set_dispute_reputation_penalty   set_dispute_resolution_limit
update_dispute_time_limit
```

## Why It Matters

`force_resolve_dispute` is the protocol's liveness backstop — the mechanism by
which a party escapes a dispute an admin never resolves. It is entirely
undocumented, so no integrator knows it exists, who may call it, or when it
becomes available.

The three configuration setters govern the timing windows and reputation penalty
that determine dispute outcomes, and `list_admins` is the only way to enumerate
who holds arbitration authority. All are operationally significant and all are
invisible in the reference.

## Proposed Solution

Document each function in the existing `## Dispute Resolution Contract` section
using the established entry format (description, **Parameters**,
**Authorization**, **Errors**, **Events**, **Example**).

For `force_resolve_dispute`, document the actual preconditions — caller must be a
party to the delivery and the resolution window must have elapsed — and note the
default 50/50 split outcome. Where behavior is currently defective (issues #205
and #206), document the intended behavior and cross-reference, rather than
enshrining the bug.

## Acceptance Criteria

- [ ] All 7 listed functions have entries in `docs/API.md`
- [ ] Each entry follows the format used by already-documented functions
- [ ] `force_resolve_dispute`'s caller restriction and timing precondition are stated
- [ ] Admin-gated setters are clearly marked as admin-only
- [ ] Signatures match the contract exactly
- [ ] Entries sit in the appropriate existing subsections

## Technical Notes

- `MIN_DISPUTE_TIME_LIMIT` (86400) is enforced at `init` but not on update — see issue #208; document the current behavior accurately or note the intended floor.
- `get_dispute_reputation_penalty` falls back to `DEFAULT_DISPUTE_REPUTATION_PENALTY` (10) when unset; that default belongs in the docs.
- `force_resolve_dispute` emits `dispute_force_resolved`, which is also missing from `docs/architecture/event-system.md` (issue #256).

## Relevant Files

- `docs/API.md` — `## Dispute Resolution Contract` section
- `contracts/dispute_resolution_contract/lib.rs` — authoritative signatures

## Testing Requirements

Documentation change; verification is by review against source:

- [ ] Each signature checked against the contract
- [ ] Authorization claims checked against the `is_admin` / party checks
- [ ] Error lists checked against `panic_with_error!` sites
- [ ] Default values checked against the module constants

## Definition of Done

- [ ] All 7 functions documented
- [ ] Format consistent with the section
- [ ] Signatures and authorization verified against source

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`documentation`


---

# Issue #252 — `docs/API.md` omits 6 public functions of `fleet_management_contract`, including both admin recovery paths

## Problem Statement

`fleet_management_contract` exposes 19 public functions. Six have no entry in
`docs/API.md`:

```
admin_force_update_treasury   admin_reassign_fleet_owner
cancel_invite                 configure_signers
deactivate_fleet              get_fleet_signers
```

## Why It Matters

`admin_reassign_fleet_owner` and `admin_force_update_treasury` are the protocol's
emergency-recovery mechanisms for a compromised fleet-owner key. Both are highly
privileged, both bypass the treasury timelock that constrains the owner's own
update path, and neither is documented anywhere in the API reference.

Undocumented emergency powers are a governance transparency problem: fleet owners
cannot evaluate what authority the protocol admin holds over their treasury, and
operators have no written procedure for using them.

`configure_signers` and `get_fleet_signers` expose the multi-signature surface
that issue #216 shows is not actually enforced — documenting them requires stating
the real authorization rule rather than the implied one.

## Proposed Solution

Document all six in the existing `## Fleet Management Contract` section. For the
two admin functions, state plainly that they are protocol-admin operations that
bypass the fleet owner and the treasury timelock, and describe their side effects
(owner reassignment resets `signers` to `[new_owner]` with threshold 1).

For `configure_signers` and `get_fleet_signers`, document the authorization rule
the contract actually implements. If issue #216 lands first, document the fixed
behavior instead.

## Acceptance Criteria

- [ ] All 6 listed functions have entries in `docs/API.md`
- [ ] The two admin recovery functions are clearly marked as protocol-admin-only
- [ ] Their bypass of the fleet-owner and timelock constraints is stated explicitly
- [ ] The signer-reset side effect of owner reassignment is documented
- [ ] `configure_signers`' documented authorization matches the implementation
- [ ] Signatures match the contract exactly

## Technical Notes

- `TREASURY_CHANGE_TIMELOCK_SECONDS` is 3 days and applies to `update_fleet_treasury` but not to `admin_force_update_treasury` — that asymmetry is the key fact to convey.
- `deactivate_fleet` affects payout routing via `get_payout_address` (see issue #217); note the consequence for in-flight escrows.
- These functions have zero test coverage (issue #224), so verify behavior by reading the implementation carefully rather than assuming.

## Relevant Files

- `docs/API.md` — `## Fleet Management Contract` section
- `contracts/fleet_management_contract/lib.rs` — authoritative signatures
- `docs/GOVERNANCE.md` — where admin powers should also be reflected

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Each signature checked against the contract
- [ ] Admin authorization checked against `is_admin` usage
- [ ] Timelock bypass verified by reading both treasury-update paths
- [ ] Signer-reset behavior verified in `admin_reassign_fleet_owner`

## Definition of Done

- [ ] All 6 functions documented
- [ ] Admin powers stated transparently
- [ ] Signatures and authorization verified against source

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`documentation`


---

# Issue #253 — `docs/API.md` omits 6 identity-reputation and 2 delivery public functions

## Problem Statement

Two contracts have smaller but similar documentation gaps in `docs/API.md`.

`identity_reputation_contract` (20 public functions, 6 undocumented):

```
get_dispute_contract    get_reputation_config    has_driver_profile
set_delivery_contract   set_dispute_contract     set_reputation_config
```

`delivery_contract` (17 public functions, 2 undocumented):

```
get_combined_state   update_delivery_metadata
```

## Why It Matters

`set_reputation_config` and `get_reputation_config` control the point values
awarded for every completed delivery — the tuning knobs for the entire reputation
economy — and are invisible in the reference.

`get_combined_state` is the protocol's cross-contract consistency check, returning
delivery state, escrow state, and a synchronization flag. It is the function an
integrator would use to detect desynchronization, and it is undocumented. (It also
currently misreports confirmed deliveries — issue #198 — which makes documenting
its contract more valuable, not less.)

`has_driver_profile` is the existence check that exists specifically to avoid the
double-registration panic pattern; undocumented, contributors will not know to use
it.

## Proposed Solution

Document all eight functions in their existing contract sections using the
established format. For `get_combined_state`, document the returned tuple shape
and the meaning of the boolean, and state which status pairs are considered
synchronized.

For the identity contract's setters, note the relationship between the named
contract fields and the separate authorization allowlist (see issue #214 — the two
are not kept in sync today).

## Acceptance Criteria

- [ ] All 6 identity-reputation functions are documented
- [ ] Both delivery functions are documented
- [ ] `get_combined_state`'s return tuple and synchronization semantics are described
- [ ] `set_reputation_config`'s effect on scoring is explained with the default values
- [ ] Admin-gated functions are marked as such
- [ ] Signatures match the contracts exactly

## Technical Notes

- `ReputationConfig` has three fields (`base_points`, `heavy_cargo_points`, `fragile_points`) with defaults 5/3/2; document both the struct and the defaults.
- `HEAVY_CARGO_GRAMS` (5000) determines when `heavy_cargo_points` applies — relevant context for `set_reputation_config`.
- `update_delivery_metadata` is restricted to the sender and only while the delivery is `Pending`; both constraints belong in the entry.

## Relevant Files

- `docs/API.md` — `## Identity Reputation Contract` and `## Delivery Contract` sections
- `contracts/identity_reputation_contract/lib.rs`, `contracts/delivery_contract/lib.rs`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Each signature checked against its contract
- [ ] State and authorization constraints checked against the implementation
- [ ] Default config values checked against the module constants
- [ ] `get_combined_state`'s synchronized pairs checked against `validate_state_sync`

## Definition of Done

- [ ] All 8 functions documented
- [ ] Return shapes and constraints described accurately
- [ ] Signatures verified against source

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`documentation`


---

# Issue #254 — `docs/API.md` contains the `## Fleet Management Contract` heading twice

## Problem Statement

`docs/API.md` declares the same top-level contract section twice:

```
974:  ## Fleet Management Contract
1016: ## Fleet Management Contract
```

Two identical `##` headings 42 lines apart produce two entries in any generated
table of contents, two anchor targets with the same intended name (the second
receiving a `-1` suffix), and an ambiguous document structure.

## Why It Matters

Anchor links to the fleet section resolve to whichever heading the generator
assigned the plain slug, so a cross-reference from another document or from the
file's own table of contents may land on the wrong block. Readers scrolling the
reference encounter the section apparently starting over.

This is a small, self-contained defect — hence Trivial — but `docs/API.md` is the
project's primary integration reference at 1,854 lines, and a duplicated
structural heading undermines navigation of the whole document.

## Proposed Solution

Inspect both blocks and merge them into a single `## Fleet Management Contract`
section, preserving all content and ordering subsections consistently with the
other contract sections in the file.

While there, verify the file's `## Table of Contents` links resolve to the merged
section, and check the remaining `##` headings for other duplicates.

## Acceptance Criteria

- [ ] Exactly one `## Fleet Management Contract` heading exists
- [ ] No content from either block is lost in the merge
- [ ] The table of contents links to the merged section correctly
- [ ] No other duplicate `##` headings remain in the file
- [ ] Subsection ordering matches the pattern used by other contract sections

## Technical Notes

- The other contract sections follow an Initialization → Operations → Query Functions ordering; match it.
- Anchor slugs are derived from heading text, so removing the duplicate changes at most the `-1` suffixed anchor, which nothing should be linking to intentionally.
- Issue #252 adds six functions to this same section — coordinate so the merge happens before or together with those additions.

## Relevant Files

- `docs/API.md` — lines ~974 and ~1016

## Testing Requirements

Documentation change; verification by review:

- [ ] Only one occurrence of the heading remains
- [ ] All previously present content is still present
- [ ] Table-of-contents links resolve correctly in rendered markdown
- [ ] No other `##` heading appears more than once

## Definition of Done

- [ ] Sections merged
- [ ] Table of contents verified
- [ ] No content lost

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Should be sequenced with #252 to avoid conflicting edits to the same section.

## Labels

`documentation`


---

# Issue #255 — `docs/API.md`'s `refund_escrow` entry contradicts the merged Holdback authorization fix

## Problem Statement

`docs/API.md` documents `refund_escrow` as follows:

```
**Parameters:**
- `caller: Address` - Sender or admin
...
**Authorization:** Sender or Admin

**Errors:**
- `InvalidState` - Escrow not in Locked or Paused state
```

The contract no longer behaves this way. After the fix merged in PR #187,
`refund_escrow` gates `Paused` **and** `Holdback` behind an admin-only check:

```rust
if record.status == EscrowStatus::Paused || record.status == EscrowStatus::Holdback {
    if !admin_authorized {
        panic_with_error!(&env, FaniLabError::Unauthorized);
    }
} else if !admin_authorized && !sender_authorized {
    panic_with_error!(&env, FaniLabError::Unauthorized);
}
```

The documented authorization ("Sender or Admin", unqualified) is wrong for two of
the three refundable states, and the documented state list omits `Holdback`
entirely — even though `Holdback` *is* refundable, just not by the sender.

## Why It Matters

This documentation describes precisely the behavior that constituted a HIGH
severity vulnerability before PR #187. An integrator reading it would build a flow
in which a sender reclaims an escrow after the recipient confirmed delivery — a
call that now correctly reverts with `Unauthorized`, but which the reference tells
them to expect to succeed.

It also understates the state machine: a reader would conclude a `Holdback` escrow
cannot be refunded at all, when in fact admin arbitration can refund it.

Because `docs/API.md` is the integration reference, this is the most
consequential single documentation inaccuracy in the repository following the fix.

## Proposed Solution

Rewrite the `refund_escrow` entry to state the state-dependent authorization
rule:

- from `Locked`: sender or admin
- from `Holdback` or `Paused`: admin only
- from `Released`, `Refunded`, `Split`: rejected with `InvalidState`

The corrected refund-authorization table already added to
`docs/contract-design/escrow-design.md` by PR #187 is the accurate source to
mirror. Check the neighbouring `release_escrow`, `raise_dispute`, and
`resolve_dispute` entries for related staleness while in the section.

## Acceptance Criteria

- [ ] `refund_escrow`'s documented authorization matches the implementation for all three refundable states
- [ ] `Holdback` is listed among the refundable states with its admin-only restriction
- [ ] The `InvalidState` error description lists the correct rejected states
- [ ] The example does not depict a sender refunding a confirmed delivery
- [ ] Adjacent escrow lifecycle entries are checked for the same class of staleness
- [ ] The entry does not contradict `docs/contract-design/escrow-design.md`

## Technical Notes

- `docs/contract-design/escrow-design.md` contains an accurate refund-authorization table added alongside the fix; reuse its wording for consistency.
- `mark_holdback_escrow` and `release_holdback_escrow` are the transitions into and out of `Holdback` — check whether they are documented and consistent.
- Do not restate exploit details beyond what the corrected authorization rule requires; the project's disclosure posture is to describe current behavior, not the historical attack.

## Relevant Files

- `docs/API.md` — `refund_escrow` entry in the Escrow Lifecycle subsection
- `contracts/escrow_contract/lib.rs` — `refund_escrow`
- `docs/contract-design/escrow-design.md` — accurate refund-authorization table

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Documented authorization matches the branch structure in `refund_escrow`
- [ ] Documented error conditions match the `panic_with_error!` sites
- [ ] Cross-checked against the design document's table for consistency
- [ ] Adjacent lifecycle entries reviewed for the same staleness

## Definition of Done

- [ ] `refund_escrow` entry corrected
- [ ] Adjacent entries checked
- [ ] No contradiction with the design document

## Complexity

**Medium**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`documentation`, `security`


---

# Issue #256 — `docs/architecture/event-system.md` omits most events the contracts actually emit

## Problem Statement

Sampling ten event topics that the contracts genuinely emit against
`docs/architecture/event-system.md` shows eight are undocumented:

```
MISSING:  escrow_holdback_marked    funds_frozen
MISSING:  untracked_balance_swept   dispute_force_resolved
MISSING:  volume_tiers_updated      signers_configured
MISSING:  delivery_metadata_updated fleet_deactivated
documented: settlement_contract_proposed, invite_accepted
```

The undocumented set includes every event emitted directly via
`Symbol::new(&env, "...")` at a call site rather than through a
`shared_types::events` helper.

## Why It Matters

The event system is the protocol's only off-chain observability surface, and this
document is its specification. Missing events mean an indexer built from the
documentation will silently drop them.

Two omissions are operationally significant. `funds_frozen` marks an escrow
entering dispute — a state change any monitoring system must observe.
`untracked_balance_swept` records the protocol's only unrestricted outbound
transfer; an operator with no visibility into it cannot reconcile treasury
movements, and issue #188 shows that sweep can move more than intended.

`escrow_holdback_marked` is the event for the delivery-confirmation transition
introduced with the holdback flow, so its absence means the document does not
describe the current escrow lifecycle at all.

## Proposed Solution

Enumerate every `env.events().publish` call site across the six contracts, and
document each event's topic, payload shape, emitting function, and meaning in the
existing document's format.

Note where a topic is constructed inline rather than via a `shared_types::events`
helper — those are the ones most likely to drift, and flagging them supports the
consolidation proposed in issues #196 and #204.

## Acceptance Criteria

- [ ] Every event emitted by any contract appears in `docs/architecture/event-system.md`
- [ ] Each entry records topic, payload fields, emitting function, and meaning
- [ ] Events emitted via inline `Symbol::new` are identified as such
- [ ] Payload shapes match the actual published values, including tuple-shaped payloads
- [ ] The document states which events are typed structs versus ad-hoc tuples
- [ ] No documented event is one the contracts do not emit

## Technical Notes

- `shared_types::events` provides topic helpers for most events; the inline `Symbol::new` sites in `escrow_contract`, `dispute_resolution_contract`, and `fleet_management_contract` are the gaps.
- Several events publish bare tuples rather than typed structs — document the actual tuple element order, since consumers must decode positionally.
- `docs/MONITORING.md` has an overlapping event list with its own defects (issues #258, #259); keep the two consistent or make one the single source.

## Relevant Files

- `docs/architecture/event-system.md`
- `contracts/*/lib.rs` — all `env.events().publish` call sites
- `contracts/shared_types/lib.rs` — `events` module and typed payload structs

## Testing Requirements

Documentation change; verification by enumeration against source:

- [ ] Every `env.events().publish` call site accounted for in the document
- [ ] Each documented payload checked against the published value
- [ ] No documented event lacks a corresponding call site
- [ ] Consider a follow-up tooling issue for a CI check that enumerates topics automatically

## Definition of Done

- [ ] All emitted events documented
- [ ] Payload shapes verified against source
- [ ] Consistency with `docs/MONITORING.md` addressed

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

**None**

## Labels

`documentation`


---

# Issue #257 — `docs/GOVERNANCE.md` describes contract pause as a future feature although it is implemented

## Problem Statement

`docs/GOVERNANCE.md` lists pause under emergency procedures as unbuilt:

```markdown
### Contract Pause (Future Feature)
In case of critical vulnerability:
1. Admin can pause contract operations
2. All state-changing functions disabled
3. Only admin can unpause
4. Query functions remain available
```

`escrow_contract` implements it today: `set_paused(admin, bool)` and
`is_paused()` exist, and `require_not_paused` gates `create_escrow`,
`create_escrows_batch`, `mark_holdback_escrow`, `release_escrow`,
`refund_escrow`, `resolve_dispute`, `resolve_dispute_split`,
`release_holdback_escrow`, and `reclaim_expired_escrow`.

The description is also inaccurate in two ways beyond the "future" label. "All
state-changing functions disabled" is false: `freeze_funds` is deliberately
exempt so a suspicious escrow can still be frozen during a halt, and
`raise_dispute` and `sweep_untracked_balance` are ungated as well (see issue
#268). And the pause only exists in `escrow_contract` — the other five contracts
have no pause concept at all (issue #232).

## Why It Matters

Governance documentation is what operators consult during an incident. Being told
a capability does not exist when it does costs response time in exactly the
scenario where time matters; being told all state changes stop when several do not
produces a false sense of containment.

The gap between "escrow is paused" and "the protocol is paused" is the operationally
critical detail, and the document currently conveys neither the capability nor its
limits.

## Proposed Solution

Rewrite the section to describe what exists: which contract implements pause,
which functions it gates, which are deliberately exempt and why, and that the
remaining five contracts continue operating.

Where the aspiration is a protocol-wide breaker, keep that as a clearly labelled
roadmap item cross-referencing issue #232, rather than describing it as current
behavior.

## Acceptance Criteria

- [ ] The section states that pause is implemented in `escrow_contract`
- [ ] The specific functions gated by `require_not_paused` are listed or accurately summarized
- [ ] Deliberate exemptions (notably `freeze_funds`) are documented with their rationale
- [ ] The document states that the other five contracts have no pause
- [ ] Any protocol-wide ambition is labelled as future work, not current behavior
- [ ] `set_paused` / `is_paused` are named so operators know the entry points

## Technical Notes

- `freeze_funds` carries an in-code comment explaining its intentional exemption — that rationale should be reflected in the governance doc.
- `is_protocol_paused` reads `DataKey::Paused` from instance storage; `set_paused` is `require_admin`-gated.
- `docs/API.md` documents `set_paused` and `is_paused`, so the governance doc can cross-reference rather than duplicate parameter detail.

## Relevant Files

- `docs/GOVERNANCE.md` — Emergency Procedures section
- `contracts/escrow_contract/lib.rs` — `set_paused`, `is_paused`, `require_not_paused`, `freeze_funds`
- `docs/API.md` — existing pause entries

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Every function claimed to be gated verified to call `require_not_paused`
- [ ] Every claimed exemption verified to lack the guard
- [ ] The five-contract gap verified by grepping for pause handling
- [ ] No claim of protocol-wide pause presented as current behavior

## Definition of Done

- [ ] Section rewritten to match implementation
- [ ] Exemptions and their rationale documented
- [ ] Roadmap ambitions clearly separated from current behavior

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Related to #232 (implementing protocol-wide pause) and #268 (unintended exemptions), but this issue documents current reality and is independently landable.

## Labels

`documentation`


---

# Issue #258 — `docs/MONITORING.md` lists Rust struct names where event topics belong

## Problem Statement

`docs/MONITORING.md`'s "Critical Events to Monitor" block mixes two naming
conventions:

```rust
// Escrow events
escrow_funded
escrow_released
escrow_refunded
delivery_disputed
...
// Admin events
ProtocolInitialized
FeeUpdated
AdminTransferred
```

The first group are real topic strings. The last three are Rust *struct* names.
The actual topics emitted for those events are snake_case, produced by
`shared_types::events` helpers:

```rust
Symbol::new(env, "protocol_initialized")
Symbol::new(env, "fee_updated")
Symbol::new(env, "admin_transferred")
```

`ProtocolInitialized` is the payload struct defined in `escrow_contract`;
`FeeUpdated` likewise. No event is ever published under those PascalCase strings.

## Why It Matters

This document is the specification an operator follows to build event monitoring.
A subscriber filtering on `ProtocolInitialized`, `FeeUpdated`, or
`AdminTransferred` matches nothing and silently receives no alerts — the failure
mode is silence, which is indistinguishable from "nothing happened".

The three affected events are precisely the governance-critical ones: protocol
initialization, fee changes, and admin transfers. A monitoring setup that misses
admin transfers is missing the single most important signal for detecting a
compromised key.

## Proposed Solution

Replace the three PascalCase entries with their real snake_case topics. Then
audit the whole list against `shared_types::events` and the inline
`Symbol::new` call sites to confirm every remaining entry is a genuine topic
rather than a type name.

Add a short note distinguishing topics from payload struct names, since the
confusion is natural and the document will be read by people who have not read
the contracts.

## Acceptance Criteria

- [ ] `ProtocolInitialized`, `FeeUpdated`, and `AdminTransferred` are replaced with their real topics
- [ ] Every entry in the list corresponds to a topic actually emitted
- [ ] The document distinguishes event topics from payload struct names
- [ ] No entry references a type that is not a topic
- [ ] The corrected names match `shared_types::events` exactly

## Technical Notes

- `shared_types::events` is the authoritative source for topic strings; each helper returns `Symbol::new(env, "<topic>")`.
- Some events are emitted with inline `Symbol::new` at the call site rather than via a helper — check those too.
- Issue #256 covers the parallel completeness gap in `docs/architecture/event-system.md`; the two documents should end up consistent.

## Relevant Files

- `docs/MONITORING.md` — "Critical Events to Monitor"
- `contracts/shared_types/lib.rs` — `events` module
- `contracts/escrow_contract/lib.rs` — `ProtocolInitialized`, `FeeUpdated` struct definitions

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Each listed topic matched to a `Symbol::new` string in the codebase
- [ ] No listed name corresponds only to a struct definition
- [ ] Corrected topics verified against `shared_types::events`
- [ ] Consistency checked against `docs/architecture/event-system.md`

## Definition of Done

- [ ] Struct names replaced with real topics
- [ ] Whole list audited
- [ ] Topic-versus-struct distinction noted in the document

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Should be consistent with #256; either can land first.

## Labels

`documentation`


---

# Issue #259 — `docs/MONITORING.md` omits the fund-movement events an operator most needs to watch

## Problem Statement

`docs/MONITORING.md`'s critical-events list covers escrow funding, release,
refund, dispute, and the main delivery lifecycle. It omits several events that
record fund movement or protective state changes:

- `funds_frozen` — an escrow entering the disputed/frozen state
- `untracked_balance_swept` — the protocol's only unrestricted outbound transfer
- `escrow_holdback_marked` — the delivery-confirmation transition into `Holdback`
- `dispute_force_resolved` — timeout-driven resolution with an automatic split
- `protocol_pause_status_changed` — the emergency circuit breaker toggling

## Why It Matters

`untracked_balance_swept` is the most consequential omission. `sweep_untracked_balance`
transfers `contract_balance - total_locked` to an admin-chosen address, and issue
#188 documents a path where legitimately escrowed funds are misclassified as
surplus. An operator whose monitoring does not watch this event has no alert on
the one call that can move user funds to an arbitrary destination.

`protocol_pause_status_changed` is the signal that the protocol has entered or
left emergency state — the document's own "Critical Alerts (Immediate Response)"
section is meaningless without it.

`escrow_holdback_marked` means the current escrow lifecycle cannot be reconstructed
from the documented event set alone: a monitor would see funding and release but
not the intermediate confirmation step.

## Proposed Solution

Add the missing events to the critical list, each with a one-line statement of
what it signals and why it warrants attention, matching the document's existing
style.

Then reconcile the alert-severity sections so the additions are reflected there:
`untracked_balance_swept` and `protocol_pause_status_changed` belong under
Critical Alerts; `funds_frozen` and `dispute_force_resolved` under High Priority.

## Acceptance Criteria

- [ ] All five listed events appear in the critical-events list
- [ ] Each has a stated meaning and monitoring rationale
- [ ] `untracked_balance_swept` and `protocol_pause_status_changed` are classified as critical alerts
- [ ] The alert-severity sections reflect the additions
- [ ] Topic names match the emitted strings exactly
- [ ] No event is listed that the contracts do not emit

## Technical Notes

- `untracked_balance_swept` is emitted with a `(token, amount, recipient)` tuple payload — the recipient is the field an alert should surface.
- `protocol_pause_status_changed` carries `(admin, paused)`.
- `escrow_holdback_marked` carries `(caller, timestamp)` with `delivery_id` in the topics.
- Issue #258 corrects naming errors in the same list; coordinate so both land coherently.

## Relevant Files

- `docs/MONITORING.md` — "Critical Events to Monitor" and the alert-priority sections
- `contracts/escrow_contract/lib.rs` — `sweep_untracked_balance`, `set_paused`, `mark_holdback_escrow`, `freeze_funds`
- `contracts/dispute_resolution_contract/lib.rs` — `force_resolve_dispute`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Each added topic verified against its `env.events().publish` call site
- [ ] Payload descriptions verified against the published values
- [ ] Alert classifications reviewed for consistency with the document's existing criteria
- [ ] Cross-checked against `docs/architecture/event-system.md`

## Definition of Done

- [ ] Missing events added with rationale
- [ ] Alert-severity sections updated
- [ ] Topics and payloads verified against source

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Overlaps #258 in the same document section; sequence to avoid conflicting edits.

## Labels

`documentation`


---

# Issue #260 — `docs/PERFORMANCE.md` documents a benchmarking API that does not exist in the SDK

## Problem Statement

`docs/PERFORMANCE.md`'s "Benchmark Tests" section presents this as the way to
measure contract cost:

```rust
#[test]
fn bench_create_delivery() {
    let env = Env::default();
    let start_instructions = env.cpu_instructions();

    contract.create_delivery(...);

    let end_instructions = env.cpu_instructions();
    println!("Instructions used: {}", end_instructions - start_instructions);
}
```

`Env::cpu_instructions()` does not exist in soroban-sdk 27 — searching the vendored
SDK source for `fn cpu_instructions` returns nothing. The example does not
compile.

The repository also contains no benchmarks: no file matches `bench_`,
`cpu_instructions`, or `budget()` anywhere under `contracts/`.

## Why It Matters

A contributor following this guide writes a test that fails to compile, with no
indication whether they have made a mistake or the documentation is wrong. Because
the surrounding guide is otherwise detailed and plausible, the natural assumption
is user error, which wastes time before the reader concludes the API is fictional.

The section is also the document's only concrete instruction for measuring
resource usage — the rest describes optimization strategies without a way to
verify their effect. Given that the project is size- and resource-sensitive (the
release profile is tuned for size, and issue #236 proposes a WASM size budget), an
unusable measurement recipe is a real gap in the contributor workflow.

## Proposed Solution

Replace the example with an API that exists in soroban-sdk 27. The SDK exposes
budget introspection through the test environment; determine the correct current
accessor and write a compiling example against one of the workspace's real
contracts.

If no suitable in-test API exists, replace the section with the on-chain
measurement approach the document already describes elsewhere (deploy to testnet
and read the transaction's resource usage) and say plainly that in-test
measurement is not currently available.

Adding one working benchmark to the repository alongside the corrected
documentation would prove the recipe and give contributors a template.

## Acceptance Criteria

- [ ] The documented benchmark example compiles against soroban-sdk 27
- [ ] The example targets a real function in one of the six contracts
- [ ] If in-test measurement is unavailable, the document says so and gives a working alternative
- [ ] At least one working benchmark exists in the repository, or the document explains why not
- [ ] No other code example in `docs/PERFORMANCE.md` references a non-existent API

## Technical Notes

- soroban-sdk 27's test environment exposes cost tracking; verify the exact accessor against the vendored crate source rather than an older tutorial.
- Benchmarks that assert on absolute instruction counts are brittle across SDK versions — prefer reporting to asserting, or assert only on relative comparisons.
- `docs/PERFORMANCE.md` also contains a "Contract Size Optimization" section that overlaps issue #236; keep the two consistent.

## Relevant Files

- `docs/PERFORMANCE.md` — "Gas Profiling" and "Benchmark Tests" sections
- `contracts/*/test.rs` — where a working benchmark would live
- The vendored `soroban-sdk` 27 source — authoritative API surface

## Testing Requirements

- Verification: the corrected example compiles when placed in a test module
- Verification: any added benchmark runs as part of `cargo test --workspace`
- Verification: every other code block in the document is checked for non-existent APIs
- Regression: existing test suite unaffected

## Definition of Done

- [ ] Non-existent API removed from documentation
- [ ] Working example or documented alternative provided
- [ ] Remaining code blocks in the document verified
- [ ] Formatting and clippy clean if a benchmark is added

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`documentation`


---

# Issue #261 — `escrow_contract::init` does not authenticate the admin it installs

## Problem Statement

Three of the six contracts install an admin without authenticating it. In
`escrow_contract`:

```rust
pub fn init(env: Env, admin: Address, token: Address, platform_fee_bps: u32) {
    if env.storage().instance().has(&StorageKey::Admin) {
        panic_with_error!(&env, FaniLabError::AlreadyInitialized);
    }
    // ... no admin.require_auth()
    env.storage().instance().set(&StorageKey::Admin, &admin);
```

The only guard is the already-initialized check. `delivery_contract::init` and
`dispute_resolution_contract::init` have the same shape.

The other three contracts do authenticate:
`identity_reputation_contract::init`, `fleet_management_contract::init`, and
`settlement_contract::init` all call `admin.require_auth()` before storing.

That split is strong evidence this is an oversight rather than a deliberate
design choice.

## Why It Matters

Between deployment and the legitimate `init` call, any address can invoke `init`
and name itself admin. Deployment and initialization are separate transactions —
`scripts/deploy-all-contracts.sh` and `scripts/initialize-all-contracts.sh` are
separate scripts, run separately — so the window is real and observable on chain.

For `escrow_contract` the captured authority is substantial: the admin sets the
platform fee, proposes and confirms the settlement contract that receives payout
routing, sets the fleet-management and dispute-resolution contracts, resolves
disputes, pauses the protocol, and calls `sweep_untracked_balance`. The attacker
also chooses `token` and `platform_fee_bps` in the same call.

The `AlreadyInitialized` guard then works against the legitimate operator: once an
attacker has initialized, the real deployer's `init` reverts and there is no
recovery path short of redeploying.

Impact is bounded by the deployment window and by operators noticing before
funding any escrows, which is why this is High rather than Critical — but the fix
is small and the current asymmetry is indefensible.

## Proposed Solution

Add `admin.require_auth()` to `init` in `escrow_contract`, `delivery_contract`,
and `dispute_resolution_contract`, matching the three contracts that already do
it. This ensures only the holder of the admin key can install that key as admin.

Deployment tooling already signs with the `deployer` identity, so the authorized
call is the one the scripts already make — no script change should be required,
though this should be verified.

## Acceptance Criteria

- [ ] `escrow_contract::init` calls `admin.require_auth()` before storing the admin
- [ ] `delivery_contract::init` and `dispute_resolution_contract::init` do the same
- [ ] An `init` call not authorized by the named admin is rejected
- [ ] A correctly authorized `init` still succeeds and stores all configuration
- [ ] The `AlreadyInitialized` guard continues to prevent re-initialization
- [ ] Initialization via `scripts/initialize-all-contracts.sh` still works

## Technical Notes

- `identity_reputation_contract::init` is the in-repo pattern: `admin.require_auth()` immediately after the already-initialized check.
- Existing tests use `env.mock_all_auths()`, so they will continue to pass; add a test that does *not* mock the admin's auth to prove the guard works.
- `dispute_resolution_contract::init` keys its already-initialized check on `DataKey::DeliveryContract` rather than an admin key, which is a separate oddity worth noting but not changing here.
- This is an ABI-compatible change: no signature changes, only an added authorization requirement.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `init`
- `contracts/delivery_contract/lib.rs` — `init`
- `contracts/dispute_resolution_contract/lib.rs` — `init`
- `contracts/identity_reputation_contract/lib.rs` — reference implementation
- `scripts/initialize-all-contracts.sh`, `scripts/initialize-contract.sh`

## Testing Requirements

- Authorization test: `init` without the admin's authorization is rejected, for each of the three contracts
- Regression test: authorized `init` succeeds and stores admin, token, and fee correctly
- Regression test: second `init` still fails with `AlreadyInitialized`
- Integration test: the standard initialization sequence used by the scripts still works
- Edge case: `init` authorized by a different address than the `admin` parameter is rejected

## Definition of Done

- [ ] `require_auth` added to all three `init` functions
- [ ] Authorization tests added and passing
- [ ] Initialization scripts verified unaffected
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`security`, `bug`


---

# Issue #262 — `sweep_untracked_balance` moves funds while the protocol is paused

## Problem Statement

`escrow_contract` gates its fund-moving entry points behind `require_not_paused`:
`create_escrow`, `create_escrows_batch`, `mark_holdback_escrow`,
`release_escrow`, `refund_escrow`, `resolve_dispute`, `resolve_dispute_split`,
`release_holdback_escrow`, and `reclaim_expired_escrow` all call it.

`sweep_untracked_balance` does not:

```rust
pub fn sweep_untracked_balance(env: Env, admin: Address, token: Address, recipient: Address) {
    admin.require_auth();
    require_admin(&env, &admin);
    // no require_not_paused
    let contract_balance = /* ... */;
    ...
    token::Client::new(&env, &token).transfer(&env.current_contract_address(), &recipient, &untracked_balance);
}
```

It is the only function in the contract that transfers tokens out without
consulting the pause flag.

`freeze_funds` is also ungated, but that is deliberate and documented in a code
comment — it moves no funds and must remain available so a suspicious escrow can
still be frozen during a halt. No such rationale exists for the sweep.

## Why It Matters

The protocol pause exists to halt fund movement during an incident. Leaving the
one unrestricted outbound transfer path available during a halt contradicts that
purpose: an operator who pauses the protocol in response to a suspected problem
has stopped every user-facing transfer while leaving the sweep open.

The interaction with issue #188 makes this concrete. Under that defect,
batch-created escrows are absent from `TotalLocked` and are therefore classified
as sweepable surplus. If a pause is triggered *because* an accounting anomaly was
noticed, the sweep — the very operation most likely to compound the problem —
remains callable.

This requires the admin key, so it is an operational-safety and defense-in-depth
gap rather than an externally exploitable flaw.

## Proposed Solution

Add `require_not_paused(&env)` to `sweep_untracked_balance`, so it behaves like
every other fund-moving function in the contract.

If there is a genuine operational reason to sweep during a halt — for example
recovering mistakenly sent tokens while the protocol is stopped — then document
that rationale in a code comment as `freeze_funds` does, rather than leaving the
exemption implicit. The current silence is the actual defect.

Review `raise_dispute` in the same pass: it is also ungated, and while it moves no
funds, whether that exemption is intentional is likewise undocumented.

## Acceptance Criteria

- [ ] `sweep_untracked_balance` is rejected with `FaniLabError::ProtocolPaused` while the protocol is paused, or its exemption is documented with a rationale
- [ ] Sweep behavior while unpaused is unchanged
- [ ] `freeze_funds` remains available while paused, with its existing rationale intact
- [ ] `raise_dispute`'s pause status is decided deliberately and documented
- [ ] Regression test covers the sweep being rejected (or permitted, per the decision) while paused

## Technical Notes

- `require_not_paused` and `is_protocol_paused` already exist in the contract; the change is one line if the gate is added.
- `freeze_funds` carries an explanatory comment describing why it is exempt — that is the documentation pattern to follow for any retained exemption.
- Existing pause tests (`test_refund_escrow_rejected_while_paused` and siblings) are the template for the new test.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `sweep_untracked_balance`, `raise_dispute`, `require_not_paused`, `freeze_funds`
- `contracts/escrow_contract/test.rs` — existing `*_rejected_while_paused` tests

## Testing Requirements

- Unit test: `sweep_untracked_balance` while paused behaves per the decision
- Unit test: `sweep_untracked_balance` while unpaused still transfers the surplus
- Unit test: `raise_dispute` while paused behaves per the decision
- Regression test: `freeze_funds` still succeeds while paused
- Regression test: all existing pause tests unchanged

## Definition of Done

- [ ] Pause behavior decided and implemented for both functions
- [ ] Any retained exemption documented in a code comment
- [ ] Tests added and passing
- [ ] Formatting and clippy clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Related to #232 (protocol-wide pause) and #197 (sweep visibility), but independently solvable.

## Labels

`security`


---

# Issue #263 — The SDK ships clients for only two of six contracts

## Problem Statement

`sdk/typescript/src/clients/` contains exactly two files:

```
delivery.client.ts
escrow.client.ts
```

There is no client for `dispute_resolution_contract`,
`identity_reputation_contract`, `fleet_management_contract`, or
`settlement_contract`. The type definitions under `src/types/` follow the same
pattern — `escrow.types.ts`, `delivery.types.ts`, and a shared
`common.types.ts`, with nothing for the other four contracts.

The package's `README.md` and `examples/basic-usage.ts` present the SDK as the
integration path for the protocol.

## Why It Matters

Four contracts have no client-side representation at all, including the ones an
integrator most needs for a complete application: raising and resolving disputes,
registering drivers and reading reputation, and managing fleets.

A consumer building on the SDK can create and release escrows but cannot dispute
one, cannot read a driver's reputation before assigning them, and cannot manage
fleet membership — so any real application must drop to raw
`@stellar/stellar-sdk` calls for those, which defeats the purpose of the wrapper.

This is scoped separately from issue #222 (the existing two clients are stubs
that log to the console): that issue is about the invocation layer being fake,
this one is about four contracts having no surface at all. Both need fixing, and
each is independently reviewable.

## Proposed Solution

Add typed clients and type definitions for the four missing contracts, mirroring
the structure of the existing two: one `*.client.ts` per contract, one
`*.types.ts` per contract with parameter interfaces, and shared enums and record
types in `common.types.ts`.

Build them on whatever invocation layer issue #222 establishes rather than adding
four more sets of stubs — if #222 has not landed, have the new methods throw an
explicit "not implemented" error instead of returning fabricated values.

Given the size, this can reasonably be split per contract if reviewers prefer;
each client is independently useful.

## Acceptance Criteria

- [ ] Clients exist for dispute resolution, identity/reputation, fleet management, and settlement
- [ ] Each client covers its contract's public functions
- [ ] Parameter and return types match the contract signatures
- [ ] Shared types are declared once in `common.types.ts` rather than duplicated
- [ ] New clients are exported from `src/index.ts`
- [ ] No method returns a fabricated value cast to a declared type

## Technical Notes

- `escrow.client.ts` is the structural template: a class taking a contract ID and network config, with one async method per contract function.
- `DisputeStatus`, `DriverTier`, `DriverFleetStatus`, `FleetProfile`, `DisputeCase`, `DriverProfile`, and `ReputationConfig` are the contract types needing TypeScript counterparts.
- Verify each signature against the contract source; several functions are undocumented in `docs/API.md` (issues #251–#253), so the contracts are the authority.
- `u64` and `i128` fields must map to `bigint`, not `number` — see the width concern raised in issue #223.

## Relevant Files

- `sdk/typescript/src/clients/` — new client files
- `sdk/typescript/src/types/` — new type files and `common.types.ts`
- `sdk/typescript/src/index.ts` — exports
- `contracts/dispute_resolution_contract/lib.rs`, `contracts/identity_reputation_contract/lib.rs`, `contracts/fleet_management_contract/lib.rs`, `contracts/settlement_contract/lib.rs`

## Testing Requirements

- Type-check: all new clients compile under `tsc`
- Unit test: parameter objects match contract signatures for a representative function per client
- Unit test: shared enums match the contract variants (extending the parity check from #223)
- Verification: `src/index.ts` exports every client
- Regression: existing escrow and delivery clients unchanged

## Definition of Done

- [ ] Four clients and their types added
- [ ] Exports wired up
- [ ] SDK builds cleanly
- [ ] No fabricated return values introduced

## Complexity

**High**

## Estimated Effort

1–2 days

## Dependencies

Builds on the invocation layer from #222; if that has not landed, new methods should throw explicitly rather than stub silently.

## Labels

`feature`


---

# Issue #264 — The SDK declares version 1.0.0 for a package with no working implementation

## Problem Statement

`sdk/typescript/package.json` declares:

```json
{
  "name": "@fanilab/sdk",
  "version": "1.0.0",
  ...
  "scripts": { "prepublish": "npm run build" }
}
```

Version `1.0.0` conventionally signals a stable, supported public API. The package
is entirely stubbed — every client method logs to the console and read methods
return `{}` cast to their declared types (issue #222) — and covers two of six
contracts (issue #263).

The six contract crates all declare `version = "0.2.0"`, so the SDK's version also
disagrees with the protocol it wraps.

The `prepublish` script is additionally a deprecated npm lifecycle hook; npm has
recommended `prepublishOnly` or `prepare` for several major versions, and
`prepublish` no longer runs on publish in modern npm.

## Why It Matters

A consumer resolving `@fanilab/sdk@^1.0.0` receives a package that silently does
nothing — writes appear to succeed and reads return empty objects that satisfy
the type system. Semantic versioning is the contract by which consumers judge
stability, and 1.0.0 here communicates the opposite of the truth.

The deprecated `prepublish` hook compounds it: if the package were published, the
build might not run, shipping a `dist/` that is stale or absent while `main` and
`types` point into it.

The repository has prior history here — closed issue #128 covered version claims
in `README.md` and `SECURITY.md` disagreeing with the manifests.

## Proposed Solution

Set the SDK version to a pre-1.0 value that reflects its maturity (`0.1.0` or
`0.2.0` to track the contracts), and replace `prepublish` with `prepare` or
`prepublishOnly`.

Add a short status note to the SDK `README.md` stating what is and is not
implemented, so the version and the documentation agree. Consider `"private": true`
until the package is genuinely publishable, which prevents accidental release
outright.

## Acceptance Criteria

- [ ] The SDK version reflects its actual maturity rather than claiming 1.0.0
- [ ] The version's relationship to the contract crate versions is decided and documented
- [ ] `prepublish` is replaced with a hook that runs on modern npm
- [ ] The SDK `README.md` states the current implementation status
- [ ] Accidental publication is prevented or made deliberate
- [ ] `npm run build` still works

## Technical Notes

- `prepare` runs on both `npm install` (from source) and `npm publish`; `prepublishOnly` runs only on publish. Either is correct; pick one deliberately.
- `"private": true` blocks `npm publish` entirely and is the strongest guard while the package is stubbed.
- If the SDK is intended to version in lockstep with the contracts, say so in the README so future bumps are mechanical.

## Relevant Files

- `sdk/typescript/package.json` — `version`, `scripts.prepublish`
- `sdk/typescript/README.md`
- `contracts/*/Cargo.toml` — the `0.2.0` crate versions for comparison

## Testing Requirements

- Verification: `npm run build` succeeds after the change
- Verification: the configured lifecycle hook runs when expected
- Verification: publication is blocked or deliberate per the chosen approach
- Documentation check: README status note matches the actual implementation state

## Definition of Done

- [ ] Version corrected and rationale documented
- [ ] Lifecycle hook modernized
- [ ] README status note added
- [ ] Build verified

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`bug`


---

# Issue #265 — `resolve_dispute_split_funds` writes the dispute record twice with identical content

## Problem Statement

`dispute_resolution_contract::resolve_dispute_split_funds` sets and persists the
dispute status, then does it again a few statements later with no intervening
change to the field:

```rust
dispute.status = DisputeStatus::Split;
dispute.resolved_at = Some(env.ledger().timestamp());
dispute.resolved_by = Some(caller.clone());
env.storage().persistent().set(&dispute_key, &dispute);
env.storage().persistent().extend_ttl(&dispute_key, /* ... */);

// ... escrow status check, delivery fetch, reputation decrease ...

dispute.status = DisputeStatus::Split;              // already Split
env.storage().persistent().set(&dispute_key, &dispute);
env.storage().persistent().extend_ttl(&dispute_key, /* ... */);
```

The second assignment is a no-op and the second write persists the same value the
first already committed.

## Why It Matters

The duplicate write costs a redundant storage write and a redundant TTL extension
on every split resolution. Soroban charges for both, so this is a small but real
per-call resource cost on a fund-moving path.

More importantly it is misleading to read. A reviewer encountering the second
assignment reasonably assumes something between the two writes modified the
status — that is the only reason the pattern would make sense — and has to trace
the intervening code to establish that nothing does. The sibling functions
`resolve_dispute_refund_sender` and `resolve_dispute_pay_driver` write once, so
this one reads as if it is doing something they are not.

No behavior is wrong, which is why this is Trivial.

## Proposed Solution

Remove the second `dispute.status = DisputeStatus::Split;` assignment and its
accompanying `set`/`extend_ttl` pair, keeping the single write that already
records status, `resolved_at`, and `resolved_by`.

Verify while doing so that the retained write happens at the right point relative
to the escrow-status check — issue #211 proposes moving that check earlier, so
coordinate if both are in flight.

## Acceptance Criteria

- [ ] The dispute record is written exactly once per `resolve_dispute_split_funds` call
- [ ] `status`, `resolved_at`, and `resolved_by` are all still persisted correctly
- [ ] TTL is extended exactly once
- [ ] Split resolution behavior is otherwise unchanged
- [ ] Existing split-resolution tests still pass unmodified

## Technical Notes

- `resolve_dispute_refund_sender` and `resolve_dispute_pay_driver` each perform a single write and are the shape to match.
- Soroban reverts all state on panic, so the earlier write is not a partial-commit safeguard — removing the duplicate cannot introduce a rollback problem.
- `test_integration_resolve_dispute_split_funds` exercises this path and should pass unchanged.

## Relevant Files

- `contracts/dispute_resolution_contract/lib.rs` — `resolve_dispute_split_funds`
- `contracts/dispute_resolution_contract/test.rs` — `test_integration_resolve_dispute_split_funds`

## Testing Requirements

- Regression test: split resolution produces the same final `DisputeCase` as before
- Regression test: `resolved_at` and `resolved_by` are populated
- Regression test: existing integration test passes unmodified
- Verification: only one storage write to the dispute key occurs per call

## Definition of Done

- [ ] Duplicate write removed
- [ ] Existing tests pass unmodified
- [ ] Formatting and clippy clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Touches the same function as #211; sequence to avoid conflicting edits.

## Labels

`refactor`


---

# Issue #266 — `delivery_contract/lib.rs` ends with a stale "implementation in progress" comment

## Problem Statement

The final line of `contracts/delivery_contract/lib.rs`, after the closing brace of
the `impl` block and the `#[cfg(test)] mod test;` declaration, is:

```rust
#[cfg(test)]
mod test;
// TTL management - implementation in progress
```

TTL management is implemented throughout the file. Every persistent write is
followed by `extend_ttl(ttl::LEDGER_TTL_THRESHOLD, ttl::LEDGER_TTL_EXTEND_TO)` —
in `create_delivery`, `create_deliveries_batch`, `update_delivery_metadata`,
`cancel_delivery`, `assign_driver`, `mark_in_transit`, `confirm_delivery`, and
`raise_dispute`, for both delivery records and the secondary indexes.

The shared `ttl` constants were introduced specifically to replace inline
magic numbers across the contracts (closed issue #115), and `delivery_contract`
was part of that migration.

## Why It Matters

The comment claims incomplete work that is in fact complete, in a file where
TTL handling is security-relevant — an entry whose TTL lapses is archived, and
`identity_reputation_contract` has a genuine TTL gap of exactly that kind (issue
#215).

A contributor auditing TTL coverage across the protocol would treat this file as
a known gap and either duplicate the existing work or, worse, deprioritize the
real gap elsewhere because this file appeared to be the known-incomplete one.

Trailing notes after the test module declaration are also easy to miss in review,
so the claim has persisted unchallenged.

## Proposed Solution

Delete the comment. If any TTL work genuinely remains in this contract, replace it
with a specific statement of what is missing and file an issue for it — but the
current blanket claim should not stand.

While confirming, verify that every `env.storage().persistent().set` in the file
is paired with an `extend_ttl`, so the deletion is backed by an actual check
rather than an assumption.

## Acceptance Criteria

- [ ] The stale comment is removed
- [ ] Every persistent write in `delivery_contract` is confirmed paired with a TTL extension
- [ ] Any genuinely missing TTL handling found during the check is filed separately
- [ ] No behavioral change
- [ ] The full test suite passes unchanged

## Technical Notes

- `shared_types::ttl::{LEDGER_TTL_THRESHOLD, LEDGER_TTL_EXTEND_TO}` are the constants in use (518400 / 1036800).
- The audit should cover both the delivery record key and the `DeliveriesBySender` / `DeliveriesByRecipient` index keys.
- Instance storage in this contract holds `StorageKey::Admin`, `DataKey::EscrowContract`, and `DataKey::IdentityReputationContract`; instance TTL extension is a separate concern from persistent entries and worth checking while in the file.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — final line, and all `extend_ttl` call sites
- `contracts/shared_types/lib.rs` — `ttl` module

## Testing Requirements

- Verification: every persistent `set` in the file has a corresponding `extend_ttl`
- Regression test: full workspace suite passes with no test changes
- Verification: no behavioral difference, since this is a comment deletion

## Definition of Done

- [ ] Comment removed
- [ ] TTL pairing verified across the file
- [ ] Any real gap found is filed separately
- [ ] Formatting and clippy clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`refactor`


---

# Issue #267 — `settlement_contract` embeds its tests inline while the other five use a separate `test.rs`

## Problem Statement

Five of the six contracts declare tests in a sibling file:

```rust
#[cfg(test)]
mod test;
```

with the implementation in `contracts/<name>/test.rs`.

`settlement_contract` instead defines its tests inline at the bottom of
`lib.rs`:

```rust
#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_init() { /* ... */ }

    #[test]
    #[should_panic(expected = "SettlementSwapNotImplemented")]
    fn test_execute_settlement_swap_panics_when_unimplemented() { /* ... */ }
}
```

`contracts/settlement_contract/` contains no `test.rs`.

## Why It Matters

Closed issue #116 standardized this contract's file layout — it was the only crate
using `src/lib.rs` while the others used a flat `lib.rs`, and it was migrated. The
test-module placement is the residue of that inconsistency and was not included.

The practical costs are small but real: contributors looking for settlement tests
in the conventional location find nothing, tooling and scripts that glob
`contracts/*/test.rs` silently skip this contract, and the contract's test file
will grow inline as Phase 3 is implemented, mixing implementation and tests in one
file where the rest of the workspace separates them.

Several issues in this backlog (#220, #221) propose settlement work that will add
tests, so establishing the conventional layout first avoids growing the inline
module further.

## Proposed Solution

Move the inline `mod test { ... }` body into `contracts/settlement_contract/test.rs`
and replace it with the `#[cfg(test)] mod test;` declaration used by the other five
contracts.

This is a pure file-movement change: the module path, the `use super::*` import,
and both existing tests carry over unchanged.

## Acceptance Criteria

- [ ] `contracts/settlement_contract/test.rs` exists and contains both existing tests
- [ ] `lib.rs` declares `#[cfg(test)] mod test;` matching the other five contracts
- [ ] Both tests still run and pass
- [ ] No test logic is changed during the move
- [ ] `cargo test -p settlement_contract` reports the same two tests as before

## Technical Notes

- The `[lib] path = "lib.rs"` entry in `Cargo.toml` is unaffected; the test module is resolved relative to `lib.rs`, so a sibling `test.rs` is found automatically.
- `use super::*;` continues to work unchanged from a separate file.
- `test_execute_settlement_swap_panics_when_unimplemented` uses `#[should_panic(expected = "...")]`, which issue #221 proposes converting to a typed-error assertion — keep the move and that change separate.

## Relevant Files

- `contracts/settlement_contract/lib.rs` — inline `mod test`
- `contracts/settlement_contract/test.rs` — new file
- Other contracts' `lib.rs` — reference for the declaration form

## Testing Requirements

- Regression test: both existing settlement tests pass after the move
- Verification: `cargo test -p settlement_contract` reports the same test count and names
- Verification: full workspace suite unaffected
- Verification: no test logic modified in the diff

## Definition of Done

- [ ] Tests relocated to `test.rs`
- [ ] Declaration matches the other five contracts
- [ ] Test count and names unchanged
- [ ] Formatting and clippy clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Should land before or alongside #220 and #221, which add settlement tests.

## Labels

`refactor`


---

# Issue #268 — `escrow_contract`'s configuration accessors have no test coverage

## Problem Statement

Seven public functions of `escrow_contract` are never referenced in
`contracts/escrow_contract/test.rs`:

```
get_dispute_resolution_contract   get_escrows_by_driver
get_escrows_by_recipient          get_escrows_by_sender
get_fleet_management_contract     get_protocol_version
set_fleet_management_contract
```

The three index accessors are covered by issue #226 and the two
fleet-configuration functions by issue #227. This issue covers the remainder:
`get_dispute_resolution_contract` and `get_protocol_version`.

`set_dispute_resolution_contract` *is* exercised — several tests call it to wire
a dispute contract before testing `freeze_funds` — but its getter is never read
back, so nothing verifies the stored value round-trips.

## Why It Matters

`get_dispute_resolution_contract` is the accessor an operator or integrator uses
to confirm which address holds the authority to call `freeze_funds` — the function
that can pause any escrow. A getter for a security-relevant configuration value
that has never been read in a test is a small but genuine gap: nothing proves it
returns what was set, or returns `None` when unset.

`get_protocol_version` is the on-chain version identifier, returned from
`ProtocolConfig::protocol_version` and set to `constants::PROTOCOL_VERSION` at
`init`. Issue #246 proposes using version identifiers as a release gate, which
makes it worth having a test pinning what this returns.

These are modest gaps, which is why this is Trivial rather than Medium — but they
are concrete and quickly closed.

## Proposed Solution

Add tests asserting that `get_dispute_resolution_contract` returns the address
set by `set_dispute_resolution_contract` and returns `None` before configuration,
and that `get_protocol_version` returns `constants::PROTOCOL_VERSION` after
`init`.

Include an authorization test for `set_dispute_resolution_contract`, which is
admin-gated and currently has no test proving a non-admin is rejected.

## Acceptance Criteria

- [ ] `get_dispute_resolution_contract` returns `None` before configuration
- [ ] It returns the configured address after `set_dispute_resolution_contract`
- [ ] `set_dispute_resolution_contract` rejects a non-admin caller
- [ ] `get_protocol_version` returns the expected constant after `init`
- [ ] Existing tests are unaffected

## Technical Notes

- `get_dispute_resolution_contract` returns `Option<Address>` and reads `DataKey::DisputeResolutionContract` from instance storage.
- `get_protocol_version` reads through `load_protocol_config`, so it panics with `NotInitialized` before `init` — worth asserting that too.
- `test_freeze_funds_unauthorized_caller_rejected` shows the existing pattern for asserting a typed authorization failure.
- Keep this issue scoped to these two accessors; the index and fleet accessors are covered by #226 and #227.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `get_dispute_resolution_contract`, `set_dispute_resolution_contract`, `get_protocol_version`, `constants::PROTOCOL_VERSION`
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: getter returns `None` before configuration
- Unit test: getter round-trips the configured address
- Authorization test: non-admin cannot call `set_dispute_resolution_contract`
- Unit test: `get_protocol_version` returns the expected constant
- Unit test: `get_protocol_version` before `init` fails with `NotInitialized`

## Definition of Done

- [ ] Tests added and passing
- [ ] Authorization boundary asserted
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Scoped to avoid overlap with #226 (index accessors) and #227 (fleet configuration).

## Labels

`test`


---

# Issue #269 — `delivery_contract`'s secondary-index accessors and identity getter are untested

## Problem Statement

Four public functions of `delivery_contract` are not referenced anywhere in
`contracts/delivery_contract/test.rs`:

```
get_deliveries_by_recipient   get_deliveries_by_sender
get_driver_profile            get_identity_reputation_contract
```

`get_driver_profile` is addressed separately by issue #202, which proposes
removing or reworking it because it fabricates data. This issue covers the other
three.

`set_identity_reputation_contract` is also never called in the delivery test
suite, which is the same gap issue #200 identifies as the reason the
repeat-delivery failure went unnoticed.

## Why It Matters

The two index accessors are the only way a client can enumerate a party's
deliveries. `create_delivery` and `create_deliveries_batch` both maintain these
indexes on every call — reading, appending, writing, and extending TTL for both
sender and recipient — so a meaningful amount of per-call work is completely
unverified.

The batch path is the specific risk: it accumulates both index vectors in memory
across the loop and flushes them once at the end, a materially different code
path from the single-delivery version. A bug there would produce deliveries that
exist but cannot be enumerated by the parties who own them.

`get_identity_reputation_contract` is the getter for configuration whose absence
of test coverage is directly implicated in issue #200.

## Proposed Solution

Add tests asserting index contents after both single and batch creation, for both
sender and recipient, including a case where the same sender creates deliveries
to multiple recipients.

Add a round-trip test for `set_identity_reputation_contract` /
`get_identity_reputation_contract`, including the `None` case before
configuration.

## Acceptance Criteria

- [ ] `get_deliveries_by_sender` returns every delivery created by a sender via both entry points
- [ ] `get_deliveries_by_recipient` does the same for recipients
- [ ] A batch creating deliveries for one sender and one recipient indexes all of them
- [ ] Both accessors return an empty vector for an address with no deliveries
- [ ] `get_identity_reputation_contract` returns `None` before configuration and the address after
- [ ] Index contents are asserted to be unchanged after a delivery reaches a terminal state

## Technical Notes

- Both accessors return an empty `Vec` via `unwrap_or_else` when the key is absent, so the empty case is a normal return rather than an error.
- Delivery IDs are `DeliveryId` (a `u64` newtype); assertions should compare against the IDs returned by `create_delivery` / `create_deliveries_batch`.
- Indexes are append-only and are not pruned on cancellation or completion — pin that intended behavior rather than assuming it.
- Issue #226 covers the equivalent escrow-side indexes; the two issues are deliberately split by contract so each is independently reviewable.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `get_deliveries_by_sender`, `get_deliveries_by_recipient`, `get_identity_reputation_contract`, `set_identity_reputation_contract`, `create_delivery`, `create_deliveries_batch`
- `contracts/delivery_contract/test.rs`

## Testing Requirements

- Unit test: single creation populates both indexes
- Unit test: batch creation populates both indexes for every element
- Unit test: empty index returns an empty vector
- Unit test: identity contract getter round-trips and returns `None` when unset
- Regression test: index contents unchanged after cancellation and after confirmation
- Edge case: one sender with deliveries to several different recipients

## Definition of Done

- [ ] Index and configuration coverage added
- [ ] Post-lifecycle index behavior pinned
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Scoped to avoid overlap with #202 (`get_driver_profile`) and #226 (escrow indexes).

## Labels

`test`


---

# Issue #270 — `identity_reputation_contract`'s reputation configuration accessors are untested

## Problem Statement

`set_reputation_config` and `get_reputation_config` are never called in
`contracts/identity_reputation_contract/test.rs`.

They control the point values awarded by `increase_reputation`:

```rust
let config = Self::get_reputation_config(env.clone());
let mut points: u32 = config.base_points;
if weight_grams > HEAVY_CARGO_GRAMS { points += config.heavy_cargo_points; }
if fragile { points += config.fragile_points; }
```

`get_reputation_config` falls back to `ReputationConfig { base_points: 5,
heavy_cargo_points: 3, fragile_points: 2 }` when unset. Nothing verifies the
defaults, that a configured value is honoured, or that the setter is admin-gated.

## Why It Matters

These values determine the entire reputation economy: how fast drivers reach
Silver and Gold tiers, and therefore who qualifies for enterprise eligibility.
They were made configurable deliberately (closed issue #105 replaced hardcoded
magic numbers with this config), and that change shipped without tests.

Because `increase_reputation` reads the config on every completed delivery, an
error here compounds across every driver in the protocol. Untested admin-gated
configuration that feeds a scoring formula is a reasonable place to want
regression coverage.

There is also no upper bound on the configured points — a `base_points` of
`u32::MAX` would saturate every driver to `MAX_REPUTATION` on their first
delivery — and nothing currently documents whether that is intended.

## Proposed Solution

Add tests covering the defaults, a configured round-trip, the effect of a changed
config on `increase_reputation`'s awarded points, and the admin authorization
boundary on the setter.

While writing them, determine whether the configured values should be bounded. If
the conclusion is that they should, file that as a separate issue rather than
expanding this one — this issue is scoped to coverage of existing behavior.

## Acceptance Criteria

- [ ] `get_reputation_config` returns the documented defaults when unset
- [ ] `set_reputation_config` stores values that `get_reputation_config` returns
- [ ] `set_reputation_config` rejects a non-admin caller with `FaniLabError::Unauthorized`
- [ ] A changed config demonstrably changes the points awarded by `increase_reputation`
- [ ] Heavy-cargo and fragile bonuses apply at the documented thresholds
- [ ] `MAX_REPUTATION` still caps the resulting score

## Technical Notes

- `HEAVY_CARGO_GRAMS` is 5000; the bonus applies strictly above it (`weight_grams > HEAVY_CARGO_GRAMS`), so 5000 exactly should not receive it — a good boundary assertion.
- `increase_reputation` is gated by `is_authorized_contract`, so tests must register an authorized caller before invoking it.
- `MAX_REPUTATION` is 100 and `register_driver` starts drivers at 50.
- `ReputationConfig` is stored in instance storage under `DataKey::ReputationConfig`.

## Relevant Files

- `contracts/identity_reputation_contract/lib.rs` — `set_reputation_config`, `get_reputation_config`, `increase_reputation`, `ReputationConfig`, `HEAVY_CARGO_GRAMS`, `MAX_REPUTATION`
- `contracts/identity_reputation_contract/test.rs`

## Testing Requirements

- Unit test: defaults returned when no config is set
- Unit test: configured values round-trip
- Authorization test: non-admin rejected by `set_reputation_config`
- Behavioral test: changed `base_points` changes awarded points
- Boundary test: `weight_grams` exactly at `HEAVY_CARGO_GRAMS` does not receive the bonus
- Boundary test: cumulative awards are capped at `MAX_REPUTATION`

## Definition of Done

- [ ] Configuration coverage added
- [ ] Behavioral effect on scoring asserted
- [ ] Authorization boundary asserted
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`test`


---

# Issue #271 — `resolve_dispute_split` bypasses `settle_escrow_funds`, charging no platform fee and skipping fleet and settlement routing

## Problem Statement

Every payout path in `escrow_contract` routes through `settle_escrow_funds`,
which deducts the platform fee and calls `payout_driver` — the function that
applies fleet-treasury redirection and settlement-contract asset preference.
`release_escrow`, `release_holdback_escrow`, and `resolve_dispute(true)` all do
this.

`resolve_dispute_split` does neither. It computes the two shares and transfers
them directly:

```rust
let sender_amount = record.amount.saturating_mul(sender_share_bps as i128) / 10000;
let driver_amount = record.amount.saturating_sub(sender_amount);
...
if driver_amount > 0 {
    token::Client::new(&env, &record.token).transfer(
        &env.current_contract_address(),
        &record.driver,          // <-- driver directly, not payout_driver
        &driver_amount,
    );
}
```

No `calculate_fee` call appears anywhere in the function, and the driver's share
goes to `record.driver` rather than through `payout_driver`.

## Why It Matters

Two distinct consequences follow from the same omission:

**No platform fee is collected on split resolutions.** A delivery settled
normally yields a fee to the admin; the same delivery settled 0/100 in the
driver's favour through a split yields none. Since `sender_share_bps` is chosen
by an admin, a split at 0 bps is economically identical to a full release but
routes the entire amount to the driver fee-free. That is an accounting
inconsistency in the protocol's revenue model, and `resolve_dispute_split`'s
own test `test_resolve_dispute_split_full_sender_share` confirms the full amount
moves without deduction.

**Fleet and settlement routing are skipped.** A driver who is an active member of
a fleet has their earnings redirected to the fleet treasury on every other payout
path. On a split resolution the money goes to the driver personally instead,
silently breaking the fleet's revenue arrangement. Likewise, a driver with a
settlement-contract asset preference receives the raw escrow token.

## Proposed Solution

Route the driver's share through the same machinery the other payout paths use,
so fee treatment and payout routing are decided in one place. The cleanest shape
is to compute the split first, then hand the driver's portion to a helper that
applies the fee and calls `payout_driver`.

Whether a platform fee *should* apply to split resolutions is a policy question
the team must answer explicitly — charging it makes splits consistent with every
other payout, while exempting them is defensible if a split is considered a
partial refund. Either way the decision should be deliberate and documented, not
an accident of a missing call.

## Acceptance Criteria

- [ ] The platform-fee treatment of split resolutions is decided and documented
- [ ] If a fee applies, it is deducted using the same computation as other payout paths
- [ ] The driver's share is routed through `payout_driver` so fleet treasury redirection applies
- [ ] Settlement-contract asset preference applies to the driver's share
- [ ] The sender's share continues to go directly to the sender
- [ ] `sender_amount + driver_amount` still equals the escrowed amount exactly (after any fee)
- [ ] Regression tests cover a split for a fleet-member driver

## Technical Notes

- `settle_escrow_funds` currently derives the fee itself; issue #190 proposes making the fee computation single-sourced, so coordinate if both are in flight.
- `payout_driver` takes an explicit amount, so it can be reused for the driver's share without restructuring.
- `resolve_dispute_split` is reached from `dispute_resolution_contract::resolve_dispute_split_funds` and from `force_resolve_dispute`'s default 50/50 outcome, so both flows inherit whatever is decided here.
- Existing split tests assert exact balances and will need updating if a fee is introduced — that is expected, not a regression.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `resolve_dispute_split`, `settle_escrow_funds`, `payout_driver`, `calculate_fee`
- `contracts/escrow_contract/test.rs` — `test_resolve_dispute_split_*`
- `contracts/dispute_resolution_contract/lib.rs` — `resolve_dispute_split_funds`, `force_resolve_dispute`
- `docs/contract-design/escrow-design.md` — fee model section

## Testing Requirements

- Unit test: split resolution applies the documented fee treatment
- Unit test: driver's share for a fleet-member driver reaches the fleet treasury
- Unit test: driver's share respects settlement-contract preference when configured
- Unit test: sender's share is unaffected by routing changes
- Property or unit test: shares plus fee sum exactly to the escrowed amount for representative bps values
- Regression test: 0 bps and 10000 bps boundary splits behave correctly

## Definition of Done

- [ ] Fee policy decided and implemented
- [ ] Driver share routed through the shared payout path
- [ ] Tests above added and passing
- [ ] Fee model documented in the escrow design doc
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

4–8 hours

## Dependencies

Interacts with #190 (single-source fee computation); either can land first, but the second should adopt the first's fee helper.

## Labels

`bug`, `security`


---

# Issue #272 — `create_escrows_batch` hardcodes `fleet_id: None`, so batched escrows can never use fleet routing

## Problem Statement

`create_escrow` accepts a `fleet_id: Option<u64>` parameter and stores it on the
`EscrowRecord`. `payout_driver` uses it to redirect the driver's payout to the
fleet treasury:

```rust
if let (Some(fleet_addr), Some(fid)) = (fleet_management_addr, fleet_id) {
    let treasury: Address = env.invoke_contract(fleet_addr, "get_payout_address", ...);
    payout_address = treasury;
}
```

`create_escrows_batch` takes `Vec<(u64, Address, i128)>` — delivery ID, driver,
amount — with no fleet parameter, and hardcodes the field:

```rust
save_escrow(&env, delivery_id, &EscrowRecord {
    ...
    fleet_id: None,
});
```

Every escrow created through the batch entry point therefore has `fleet_id: None`
permanently, and there is no setter to populate it afterwards.

## Why It Matters

Fleet operators are the users most likely to need batch creation — dispatching
many deliveries at once is the fleet use case — and they are precisely the users
for whom the batch path silently disables treasury routing.

The failure is invisible at creation time and only manifests at settlement, when
earnings arrive in individual drivers' wallets instead of the fleet treasury. By
then the escrow is `Released` and the routing cannot be corrected. A fleet that
mixes single and batch creation would see its revenue split between two
destinations with no indication why.

Because `fleet_id` is immutable after creation, there is no remediation short of
avoiding the batch entry point entirely.

## Proposed Solution

Extend the batch tuple to carry an optional fleet ID, so batch-created escrows can
express the same routing as single-created ones. A four-element tuple
`(u64, Address, i128, Option<u64>)` is the direct analogue of `create_escrow`'s
parameters.

If a single fleet applies to the whole batch — the likely common case — a single
`fleet_id: Option<u64>` function parameter alongside `recipient` and `token` would
be simpler and cheaper in call data. Either shape is acceptable; the tuple form is
more expressive, the parameter form is more ergonomic.

This changes the batch function's signature, so it is a breaking ABI change for
any existing caller and should be noted in `CHANGELOG.md`.

## Acceptance Criteria

- [ ] `create_escrows_batch` can create escrows with a populated `fleet_id`
- [ ] A batch-created escrow with a fleet ID routes its payout to the fleet treasury
- [ ] Omitting the fleet ID still produces `fleet_id: None` and direct driver payout
- [ ] The chosen signature shape is documented in `docs/API.md`
- [ ] The ABI change is recorded in `CHANGELOG.md`
- [ ] Regression test covers a batch-created fleet escrow settling to the treasury

## Technical Notes

- `EscrowRecord.fleet_id` is `Option<u64>` and is read only by `settle_escrow_funds` → `payout_driver`.
- The single-escrow path already proves the routing works; this issue is about making it reachable from the batch path.
- Soroban tuple parameters are XDR-encoded positionally, so extending the tuple is a wire-format change for existing clients.
- Issues #188, #189, and #196 also modify `create_escrows_batch`; sequence them to avoid repeated conflicting edits to one function.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `create_escrows_batch`, `create_escrow`, `payout_driver`
- `contracts/escrow_contract/test.rs`
- `docs/API.md` — batch entry point documentation
- `CHANGELOG.md`

## Testing Requirements

- Unit test: batch escrow with a fleet ID stores it on the record
- Integration test: batch-created fleet escrow settles to the fleet treasury
- Unit test: batch escrow without a fleet ID pays the driver directly
- Regression test: existing batch behavior preserved for the no-fleet case
- Edge case: mixed batch with some elements carrying a fleet ID and some not, if the tuple form is chosen

## Definition of Done

- [ ] Batch entry point supports fleet routing
- [ ] Tests above added and passing
- [ ] `docs/API.md` and `CHANGELOG.md` updated
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Shares a function with #188, #189, and #196 — coordinate ordering.

## Labels

`bug`, `enhancement`


---

# Issue #273 — Oversized escrow batches fail with `InvalidState` instead of a dedicated error

## Problem Statement

Both contracts cap batch size at 100, but report violations differently.

`delivery_contract` defines and uses a purpose-built variant:

```rust
/// A batch operation (e.g. create_deliveries_batch) exceeded MAX_BATCH_SIZE.
BatchTooLarge = 3,
...
if metadata_list.len() > MAX_BATCH_SIZE {
    panic_with_error!(&env, DeliveryError::BatchTooLarge);
}
```

`escrow_contract` reuses its generic state error:

```rust
if escrow_list.len() > constants::MAX_BATCH_SIZE {
    panic_with_error!(&env, EscrowError::InvalidState);
}
```

`EscrowError::InvalidState` is also returned for genuine state-machine violations
throughout the contract — refunding a `Released` escrow, releasing a `Paused` one,
reclaiming a non-expired one.

## Why It Matters

A client receiving `InvalidState` from `create_escrows_batch` cannot tell whether
the escrow state machine rejected the operation or the batch was simply too large.
Those call for entirely different responses: the first is a logic error, the
second is fixed by splitting the batch and retrying.

`delivery_contract` already demonstrates the right shape, so the inconsistency is
gratuitous — a caller handling both contracts' batch endpoints must special-case
the escrow one.

`docs/ERROR_CODES.md` documents per-contract error semantics, so the overloaded
meaning propagates into published documentation.

## Proposed Solution

Add a `BatchTooLarge` variant to `EscrowError` and use it for the size check,
mirroring `DeliveryError`. Append the new discriminant rather than renumbering,
since off-chain code may match on the existing values.

Update `docs/ERROR_CODES.md` with the new code.

## Acceptance Criteria

- [ ] `EscrowError` has a dedicated variant for batch-size violations
- [ ] `create_escrows_batch` returns it when the batch exceeds `MAX_BATCH_SIZE`
- [ ] Existing `EscrowError` discriminants are unchanged
- [ ] Genuine state violations still return `InvalidState`
- [ ] `docs/ERROR_CODES.md` documents the new code
- [ ] Regression test asserts the specific error for an oversized batch

## Technical Notes

- `EscrowError` currently has nine variants (1–9); append as 10.
- `MAX_BATCH_SIZE` is 100 in `escrow_contract::constants` and 100 in `delivery_contract` — the values agree, only the error handling differs.
- A batch of exactly `MAX_BATCH_SIZE` must still succeed; the check is `>`, not `>=`.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `EscrowError`, `create_escrows_batch`, `constants::MAX_BATCH_SIZE`
- `contracts/delivery_contract/lib.rs` — `DeliveryError::BatchTooLarge` reference
- `docs/ERROR_CODES.md`
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: batch of `MAX_BATCH_SIZE + 1` returns the new error
- Unit test: batch of exactly `MAX_BATCH_SIZE` succeeds
- Regression test: state-machine violations still return `InvalidState`
- Verification: existing error discriminants unchanged

## Definition of Done

- [ ] Dedicated error added and used
- [ ] `docs/ERROR_CODES.md` updated
- [ ] Tests added and passing
- [ ] Formatting and clippy clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Touches `create_escrows_batch` alongside #188, #189, #196, and #272 — coordinate ordering.

## Labels

`bug`


---

# Issue #274 — `DeliveryCreatedEvent.amount` is a dead field always published as zero

## Problem Statement

`shared_types::DeliveryCreatedEvent` carries an `amount` field, and
`delivery_contract::create_delivery` is its only typed emitter:

```rust
env.events().publish(
    (events::delivery_created(&env),),
    DeliveryCreatedEvent {
        delivery_id: delivery_id.value(),
        sender,
        amount: 0,
    },
);
```

The literal `0` is the only value ever assigned. Deliveries carry no amount — the
escrowed value lives on `EscrowRecord` in a different contract, created by a
separate `create_escrow` call — so there is no meaningful value the delivery
contract could supply.

## Why It Matters

Every consumer of `delivery_created` receives an `amount` field that is
structurally present and semantically meaningless. An integrator building an
indexer will reasonably read it as the delivery's value and record zeros, or will
waste time determining why it is always zero.

The field also costs event payload size on every delivery creation, and it invites
a future contributor to "fix" it by plumbing an amount into the delivery contract
that does not belong there — the delivery and escrow are deliberately separate
concerns.

## Proposed Solution

Remove `amount` from `DeliveryCreatedEvent` and from the emission site. Consumers
wanting the escrowed value should read `escrow_funded`, which carries a real
`amount`.

This is a wire-format change for off-chain consumers, so record it in
`CHANGELOG.md` under a breaking or behavioral-change heading.

If the field is instead wanted for a future design where deliveries carry a
declared value, leave it in place but document explicitly that it is currently
unpopulated — the present silence is the problem.

## Acceptance Criteria

- [ ] `DeliveryCreatedEvent` no longer carries a permanently-zero field, or its unpopulated status is documented
- [ ] `create_delivery`'s emission is updated accordingly
- [ ] `create_deliveries_batch`'s event is consistent with the single-creation event (see issue #204)
- [ ] `docs/architecture/event-system.md` reflects the final payload shape
- [ ] The change is recorded in `CHANGELOG.md`
- [ ] Existing event assertions in tests are updated

## Technical Notes

- `DeliveryCreatedEvent` is declared in `contracts/shared_types/lib.rs` and used only by `delivery_contract`.
- `EscrowFundedEvent` carries the real `amount` and is the correct source for value data.
- Issue #204 proposes making the batch path emit this same typed struct; coordinate so both land with one agreed payload shape.
- Removing a field changes XDR encoding — verify no other contract or test decodes this event positionally.

## Relevant Files

- `contracts/shared_types/lib.rs` — `DeliveryCreatedEvent`
- `contracts/delivery_contract/lib.rs` — `create_delivery`, `create_deliveries_batch`
- `docs/architecture/event-system.md`
- `CHANGELOG.md`

## Testing Requirements

- Regression test: `delivery_created` is still emitted with correct `delivery_id` and `sender`
- Verification: no test or contract decodes the removed field
- Regression test: full workspace suite passes
- Documentation check: event payload documented matches what is emitted

## Definition of Done

- [ ] Dead field removed or documented as unpopulated
- [ ] Event shape consistent across both creation paths
- [ ] `CHANGELOG.md` and event documentation updated
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Coordinate with #204, which changes the batch path's event emission.

## Labels

`refactor`


---

# Issue #275 — Dependabot does not cover the SDK's npm dependencies

## Problem Statement

`.github/dependabot.yml` configures two ecosystems:

```yaml
- package-ecosystem: "cargo"
  directory: "/"
- package-ecosystem: "github-actions"
  directory: "/"
```

There is no `npm` entry for `sdk/typescript`, which declares its own dependency
tree:

```json
"dependencies": { "@stellar/stellar-sdk": "^11.0.0" },
"devDependencies": {
  "@types/jest": "^29.0.0", "@types/node": "^18.0.0",
  "jest": "^29.0.0", "typescript": "^5.0.0", "ts-jest": "^29.0.0"
}
```

Those packages are never checked for updates or advisories. The Rust side is
covered by both Dependabot and `security-audit.yml`'s `cargo audit`; the
TypeScript side has neither.

## Why It Matters

`@stellar/stellar-sdk` is the SDK's only runtime dependency and the library
through which all contract interaction will flow once issue #222 implements real
invocation. A published advisory against it — or against any transitive
dependency — would go unnoticed indefinitely.

The pin is also already stale: `^11.0.0` predates the SDK generations aligned with
current Soroban protocol versions, while the contracts target soroban-sdk 27.
Nothing in the repository surfaces that divergence.

Combined with the absence of any CI job that builds the SDK (issue #248), the
TypeScript package currently has no automated quality or security signal at all.

## Proposed Solution

Add an `npm` ecosystem entry for `/sdk/typescript` to `.github/dependabot.yml`,
matching the schedule and configuration style of the existing entries.

Evaluate whether `@stellar/stellar-sdk` should be moved to a current major
version as part of the same change, since implementing real invocation (#222)
against a stale major is wasted effort. Adding a committed lockfile would make
both Dependabot updates and CI installs reproducible.

## Acceptance Criteria

- [ ] `.github/dependabot.yml` includes an npm ecosystem entry for the SDK directory
- [ ] The entry uses a schedule and configuration consistent with the existing entries
- [ ] Dependabot opens update PRs for SDK dependencies
- [ ] A decision on the `@stellar/stellar-sdk` major version is recorded
- [ ] A lockfile decision is made and documented
- [ ] The existing cargo and github-actions entries are unchanged

## Technical Notes

- Dependabot's `directory` must point at the directory containing `package.json` — `/sdk/typescript`, not `/`.
- Without a committed lockfile Dependabot can still update `package.json` ranges, but CI installs remain non-reproducible.
- Coordinate with issue #248: a CI build job makes Dependabot's PRs verifiable rather than blind.
- `security-audit.yml` could additionally run `npm audit` for the SDK, though Dependabot alerts may be sufficient.

## Relevant Files

- `.github/dependabot.yml`
- `sdk/typescript/package.json`
- `.github/workflows/security-audit.yml` — if npm auditing is added

## Testing Requirements

- Verification: Dependabot configuration is syntactically valid
- Verification: Dependabot detects the SDK manifest (observable in the repository's Dependabot logs)
- Verification: an update PR is opened for at least one outdated SDK dependency
- Regression: cargo and github-actions update behavior unchanged

## Definition of Done

- [ ] npm ecosystem configured for the SDK
- [ ] Dependency versions reviewed and decision recorded
- [ ] Lockfile decision documented
- [ ] Configuration validated

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Complements #248 (CI build) and #264 (SDK packaging); each is independently landable.

## Labels

`security`, `enhancement`


---

# Issue #276 — `deploy-contract.sh` overwrites the combined deployment record written by `deploy-all-contracts.sh`

## Problem Statement

Both deployment scripts write to the same path:

```bash
# scripts/deploy-all-contracts.sh line 11
OUTPUT_FILE="$PROJECT_ROOT/contract-ids-$NETWORK.json"

# scripts/deploy-contract.sh line 13
OUTPUT_FILE="$PROJECT_ROOT/contract-ids-$NETWORK.json"
```

`deploy-all-contracts.sh` builds a JSON object containing every deployed
contract's ID. `deploy-contract.sh` deploys one contract and writes its own
output to the identical filename, replacing the combined record.

`deploy-testnet.yml` exposes both paths through one input, so a maintainer who
deploys `all` and later redeploys a single contract through the same workflow
destroys the combined record on the second run.

## Why It Matters

`contract-ids-$NETWORK.json` is the deployment's authoritative output — it is
what `deploy-testnet.yml` uploads as its artifact and what an operator consults to
find the addresses needed for initialization and for wiring the contracts to each
other.

Redeploying a single contract is the normal way to ship a fix to one contract, and
doing so silently discards the addresses of the other five. Recovering them
requires re-reading old workflow artifacts or block explorer history. Nothing warns
that this has happened.

The single-contract script also has no way to update one entry within the combined
file, which is what an operator would actually want.

## Proposed Solution

Make single-contract deployment update the combined record rather than replace it:
read the existing `contract-ids-$NETWORK.json` if present, replace only the entry
for the contract just deployed, and write the merged result.

If merging in shell is judged too fragile, the alternative is to have
`deploy-contract.sh` write to a distinct per-contract filename and leave the
combined record untouched — but then `deploy-testnet.yml`'s artifact patterns must
be updated to collect both (see issue #247).

Either way, overwriting five contracts' addresses as a side effect of deploying
one must stop.

## Acceptance Criteria

- [ ] Deploying a single contract does not discard other contracts' recorded addresses
- [ ] The combined record remains valid JSON after a single-contract deployment
- [ ] Deploying all contracts still produces a complete record
- [ ] `deploy-testnet.yml` collects whatever files the scripts now produce
- [ ] Behavior is documented in `docs/DEPLOYMENT.md`
- [ ] Running a single-contract deploy with no pre-existing record still works

## Technical Notes

- The scripts build JSON with `echo` rather than a JSON tool; merging reliably suggests using `jq`, which would become a new dependency for the scripts — weigh that against the per-file alternative.
- `$NETWORK` is a script argument (`local`/`testnet`/`mainnet`), so records are already network-scoped.
- Issue #241 must be resolved before either script can run in CI at all, since the `deployer` identity is never created.
- `docs/DEPLOYMENT.md` documents both scripts and should describe the record's lifecycle.

## Relevant Files

- `scripts/deploy-contract.sh` — `OUTPUT_FILE`, output writing
- `scripts/deploy-all-contracts.sh` — `OUTPUT_FILE`, output writing
- `.github/workflows/deploy-testnet.yml` — artifact collection
- `docs/DEPLOYMENT.md`

## Testing Requirements

- Verification: deploy all, then deploy one — the other five addresses survive
- Verification: the resulting file is valid JSON in both cases
- Verification: single-contract deploy with no pre-existing file produces a valid record
- Verification: the workflow uploads the correct artifacts after the change
- Edge case: deploying the same single contract twice in succession

## Definition of Done

- [ ] Single-contract deployment preserves the combined record
- [ ] Workflow artifact collection updated if filenames changed
- [ ] `docs/DEPLOYMENT.md` updated
- [ ] Verified against both deployment paths

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Practically testable in CI only after #241; the script logic can be developed and tested locally without it.

## Labels

`bug`


---

# Issue #277 — `get_payout_address` panics when a fleet record is missing, blocking the entire escrow settlement

## Problem Statement

`fleet_management_contract::get_payout_address` treats a missing fleet record as a
fatal error:

```rust
Some(DriverFleetStatus::Active) => {
    let profile: FleetProfile = env
        .storage()
        .persistent()
        .get(&DataKey::Fleet(fleet_id))
        .unwrap_or_else(|| panic_with_error!(&env, FleetError::FleetNotFound));
    if profile.active { profile.treasury } else { driver }
}
Some(DriverFleetStatus::Pending) | Some(DriverFleetStatus::Removed) | None => driver,
```

Every other branch falls back to paying the driver directly. Only the case where
the driver is recorded `Active` but the fleet record cannot be loaded panics.

The two pieces of state are stored under separate persistent keys —
`DataKey::DriverFleet(fleet_id, driver)` and `DataKey::Fleet(fleet_id)` — with
independent TTLs, so they can diverge if one is archived and the other is not.

## Why It Matters

This function is called from `escrow_contract::payout_driver` during settlement.
A panic here propagates and reverts the entire `release_escrow` or
`release_holdback_escrow` transaction, so the driver cannot be paid at all — not
via the treasury, and not directly.

The escrow remains in `Holdback` or `Locked` with no way to settle it while the
inconsistency persists. A missing fleet record is exactly the situation where
falling back to paying the driver is both safe and obviously preferable to
freezing the payment.

The divergence is reachable without any malicious action: fleet records are
persistent entries whose TTL is extended on write, and a fleet with no membership
changes for an extended period may have its record archived while a driver's
status entry, written at a different time, survives.

## Proposed Solution

Fall back to the driver's own address when the fleet record cannot be loaded,
matching every other branch of the match. A payout routed to the driver instead of
a treasury is a recoverable accounting matter; a permanently unsettleable escrow
is not.

Emit an event or otherwise make the fallback observable so operators can detect
and repair the inconsistency rather than having it pass silently.

Consider whether `escrow_contract::payout_driver` should additionally tolerate a
failed cross-contract call, so no fleet-contract fault can block settlement — but
keep that as a separate consideration, since it is a broader trust-boundary
change.

## Acceptance Criteria

- [ ] A missing fleet record causes `get_payout_address` to return the driver's address rather than panicking
- [ ] An active fleet with a present record still routes to the treasury
- [ ] An inactive fleet still routes to the driver
- [ ] `Pending`, `Removed`, and `None` statuses behave as before
- [ ] The fallback is observable (event or equivalent)
- [ ] Regression test covers the divergent-state case

## Technical Notes

- `DataKey::DriverFleet(fleet_id, driver)` and `DataKey::Fleet(fleet_id)` are separate persistent entries; the roster is a third.
- Issue #217 covers a different aspect of this function — that routing is resolved at payout time rather than escrow-creation time. This issue is specifically about the panic on missing state.
- `get_payout_address` has no `require_auth`; it is a read-only accessor invoked cross-contract, so changing its failure mode does not affect authorization.
- `FleetError::FleetNotFound` remains appropriate for direct callers querying a genuinely unknown fleet — consider whether the accessor should distinguish those cases.

## Relevant Files

- `contracts/fleet_management_contract/lib.rs` — `get_payout_address`, `DataKey`
- `contracts/escrow_contract/lib.rs` — `payout_driver`
- `contracts/fleet_management_contract/test.rs`

## Testing Requirements

- Unit test: driver `Active` with the fleet record removed → returns the driver, does not panic
- Unit test: driver `Active` with an active fleet → returns the treasury
- Unit test: driver `Active` with an inactive fleet → returns the driver
- Integration test: escrow settlement succeeds when the fleet record is absent
- Regression test: existing payout routing behavior unchanged for consistent state
- Verification: the fallback is observable in emitted events

## Definition of Done

- [ ] Missing-record fallback implemented
- [ ] Fallback made observable
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Related to #217 but addresses a distinct failure mode; both are independently solvable.

## Labels

`bug`, `security`


---

# Issue #278 — `fleet_management_contract::get_fleet_roster` has no test coverage

## Problem Statement

`get_fleet_roster` is not referenced anywhere in
`contracts/fleet_management_contract/test.rs`. It is the only way to enumerate a
fleet's membership:

```rust
pub fn get_fleet_roster(env: Env, fleet_id: FleetId) -> soroban_sdk::Vec<Address> {
    let roster_key = DataKey::FleetRoster(fleet_id);
    env.storage().persistent().get(&roster_key)
        .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
}
```

The roster is mutated by four functions — `add_driver_to_fleet`,
`accept_fleet_invite`, `remove_driver_from_fleet`, and `cancel_invite` — none of
which has its roster effect verified, because the only accessor that would reveal
it is never called in a test.

## Why It Matters

The roster is the fleet's membership record, and four separate code paths write to
it. A driver added but not appearing in the roster, or removed but still listed,
would be invisible to the current test suite — the per-driver
`DataKey::DriverFleet` status is checked by some tests, but the roster is a
separate storage entry that can diverge from it.

That divergence matters: `get_payout_address` reads the per-driver status while
operators and clients read the roster, so the two disagreeing means the fleet's
apparent membership does not match who actually gets treasury routing.

Issue #28 requested an enumerable roster; the implementation landed without any
verification that enumeration returns correct results.

## Proposed Solution

Add tests asserting roster contents after each of the four mutating operations,
including that the roster and the per-driver `DriverFleetStatus` remain consistent.

Cover the empty case, since the accessor returns an empty vector rather than
failing for an unknown fleet.

## Acceptance Criteria

- [ ] `get_fleet_roster` returns an empty vector for a fleet with no drivers
- [ ] A driver added via `add_driver_to_fleet` appears in the roster
- [ ] A driver accepting an invite is reflected correctly
- [ ] A driver removed via `remove_driver_from_fleet` is reflected per the documented intent
- [ ] A cancelled invite is reflected correctly
- [ ] Roster contents and per-driver `DriverFleetStatus` remain consistent across all four operations
- [ ] An unknown fleet ID returns an empty vector rather than panicking

## Technical Notes

- `DriverFleetStatus` has `Pending`, `Active`, and `Removed` variants; the roster's documented behavior is to contain "both Pending and Active" drivers, so whether `Removed` drivers are pruned is the key question to pin.
- `MAX_ROSTER_SIZE` is 10000 — issue #219 questions whether that bound is workable; this issue only needs small rosters.
- `get_driver_fleet_status` is the per-driver accessor and is the natural cross-check.

## Relevant Files

- `contracts/fleet_management_contract/lib.rs` — `get_fleet_roster`, `add_driver_to_fleet`, `accept_fleet_invite`, `remove_driver_from_fleet`, `cancel_invite`, `get_driver_fleet_status`
- `contracts/fleet_management_contract/test.rs`

## Testing Requirements

- Unit test: empty roster for a new fleet
- Unit test: roster after adding one and several drivers
- Unit test: roster after an invite is accepted
- Unit test: roster after a driver is removed
- Unit test: roster after an invite is cancelled
- Consistency test: roster membership agrees with `get_driver_fleet_status` in every case
- Edge case: unknown fleet ID returns empty rather than panicking

## Definition of Done

- [ ] Roster coverage added for all four mutating paths
- [ ] Roster/status consistency asserted
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`test`


---

# Issue #279 — Peer-contract getters in the dispute and identity contracts are untested

## Problem Statement

Two configuration accessors are never called in their contracts' test suites:

- `dispute_resolution_contract::get_identity_reputation_contract`
- `identity_reputation_contract::get_dispute_contract`

Both return the address of a peer contract and both panic with
`FaniLabError::NotInitialized` when unset:

```rust
pub fn get_identity_reputation_contract(env: Env) -> Address {
    env.storage().instance().get(&DataKey::IdentityReputationContract)
        .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized))
}
```

Their corresponding setters are exercised — `set_identity_reputation_contract` is
called once in the reputation integration test, and `set_dispute_contract` exists
alongside `set_delivery_contract` — but nothing reads the stored values back.

## Why It Matters

These getters describe the protocol's cross-contract trust topology: which address
the dispute contract will call to adjust reputation, and which address the identity
contract regards as the dispute authority. An operator verifying a deployment's
wiring reads exactly these functions.

The `NotInitialized` panic makes coverage more valuable than for a typical getter.
`identity_reputation_contract::init` does not populate `DataKey::DisputeContract`
at all (issue #214), so `get_dispute_contract` panics on a freshly initialized
contract — a behavior no test currently observes. A round-trip test would have
surfaced that gap immediately.

These are modest additions, hence Trivial, but they close a verification gap on
security-relevant configuration.

## Proposed Solution

Add round-trip tests for both getters: assert the `NotInitialized` panic before
configuration, then assert the configured address is returned after the
corresponding setter is called.

Include the authorization boundary on the setters, since both are admin-gated and
neither has a test proving a non-admin is rejected.

## Acceptance Criteria

- [ ] `get_identity_reputation_contract` fails with `NotInitialized` before configuration
- [ ] It returns the configured address after `set_identity_reputation_contract`
- [ ] `get_dispute_contract` fails with `NotInitialized` before configuration
- [ ] It returns the configured address after `set_dispute_contract`
- [ ] Both setters reject non-admin callers with `Unauthorized`
- [ ] Tests document the post-`init` behavior of `get_dispute_contract` accurately

## Technical Notes

- `dispute_resolution_contract` uses its own multi-admin `is_admin` check; `identity_reputation_contract` uses `shared_types::is_admin` against a single stored admin. The authorization assertions differ accordingly.
- `try_` client methods are the established pattern for asserting typed failures — see `test_freeze_funds_unauthorized_caller_rejected` in the escrow suite.
- If issue #214 lands first and makes `init` populate `DataKey::DisputeContract`, the pre-configuration assertion for `get_dispute_contract` becomes unreachable and the test should be adjusted rather than deleted.

## Relevant Files

- `contracts/dispute_resolution_contract/lib.rs` — `get_identity_reputation_contract`, `set_identity_reputation_contract`
- `contracts/identity_reputation_contract/lib.rs` — `get_dispute_contract`, `set_dispute_contract`
- `contracts/dispute_resolution_contract/test.rs`, `contracts/identity_reputation_contract/test.rs`

## Testing Requirements

- Unit test: each getter panics with `NotInitialized` before configuration
- Unit test: each getter round-trips its configured address
- Authorization test: non-admin rejected by each setter
- Regression test: existing integration tests that call the setters still pass
- Edge case: reconfiguring to a different address is reflected by the getter

## Definition of Done

- [ ] Round-trip and authorization tests added for both contracts
- [ ] Post-`init` behavior documented in the tests
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Interacts with #214, which changes what `init` stores; adjust the pre-configuration assertion if that lands first.

## Labels

`test`


---

# Issue #280 — `docs/TESTING.md` prescribes a benchmark command that produces no output

## Problem Statement

`docs/TESTING.md` instructs contributors to measure performance with:

```bash
cargo test --release -- --nocapture | grep "instructions"
```

No test in the workspace prints anything containing "instructions". Searching
`contracts/` for `bench_`, `cpu_instructions`, and `budget()` returns no matches,
so the command runs the full suite in release mode and filters to nothing.

The same document also instructs `cargo add proptest --dev`, though `proptest` is
already a declared dev-dependency of `escrow_contract` and is used by three
property tests there.

## Why It Matters

A contributor following the testing guide runs a long release-mode build and
receives empty output, with no indication whether the command is wrong, their
setup is wrong, or there is genuinely nothing to report. Release-mode compilation
of the workspace is not a quick operation, so the wasted time is real.

The instruction implies benchmark instrumentation exists. It does not — and
`docs/PERFORMANCE.md` compounds this by documenting an
`env.cpu_instructions()` API that soroban-sdk 27 does not provide (issue #260).
A contributor moving between the two documents finds two mutually reinforcing but
equally unusable recipes.

## Proposed Solution

Remove or correct the benchmark command. If issue #260 establishes a working
measurement approach, reference it from here rather than duplicating it — one
document should own the recipe.

Update the `cargo add proptest --dev` instruction to reflect that the dependency
already exists for `escrow_contract`, and state where it must be added if a
contributor wants property tests in another crate (issue #237 covers extending
that coverage).

While in the file, verify the remaining commands actually work: the `cargo test`,
`cargo test -p <crate>`, and `cargo tarpaulin` invocations should each be run once
to confirm.

## Acceptance Criteria

- [ ] The benchmark command either works or is removed
- [ ] If measurement is documented, it points to a single authoritative recipe
- [ ] The `proptest` instruction reflects the current dependency state
- [ ] Every command in `docs/TESTING.md` has been executed and verified
- [ ] No command in the document produces silently empty output
- [ ] The document does not contradict `docs/PERFORMANCE.md`

## Technical Notes

- `cargo tarpaulin` is used by CI and is a real, working command — leave it.
- `cargo test -p escrow_contract` and `cargo test -p delivery_contract` are valid package names; verify the others named in the file exist.
- Issue #260 addresses the equivalent problem in `docs/PERFORMANCE.md`; the two should be reconciled so measurement guidance lives in one place.
- Issue #237 covers extending property testing beyond `escrow_contract`, which is the context for the `proptest` instruction.

## Relevant Files

- `docs/TESTING.md` — benchmark command, `cargo add proptest` instruction
- `docs/PERFORMANCE.md` — overlapping measurement guidance
- `contracts/escrow_contract/Cargo.toml` — existing `proptest` dev-dependency

## Testing Requirements

Documentation change; verification by executing the documented commands:

- [ ] Every command in the document run once and confirmed to behave as described
- [ ] The benchmark command either produces output or is gone
- [ ] Package names in `-p` invocations verified against the workspace members
- [ ] Cross-checked against `docs/PERFORMANCE.md` for consistency

## Definition of Done

- [ ] Non-functional command removed or corrected
- [ ] `proptest` instruction updated
- [ ] All documented commands verified
- [ ] No contradiction with the performance guide

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Should be reconciled with #260 so measurement guidance is not duplicated.

## Labels

`documentation`


---

# Issue #281 — `create_escrow` accepts identical sender, recipient, and driver addresses

## Problem Statement

`escrow_contract::create_escrow` takes three party addresses and validates none of
their relationships:

```rust
pub fn create_escrow(env: Env, sender: Address, recipient: Address, driver: Address,
                     delivery_id: u64, token: Address, amount: i128, fleet_id: Option<u64>) {
    sender.require_auth();
    require_not_paused(&env);
    if amount <= 0 { /* InvalidAmount */ }
    if /* duplicate delivery */ { /* DuplicateDelivery */ }
    if token != config.token { /* InvalidToken */ }
    // no check that sender, recipient, driver are distinct
```

`delivery_contract` is careful about exactly this: `assign_driver` rejects a
driver equal to the sender or recipient with `DeliveryError::InvalidDriver`, and
`confirm_delivery` re-checks the same condition as defense in depth.

The escrow contract is independently callable — nothing requires it to be reached
through `delivery_contract` — so its party constraints are not enforced by the
delivery contract's checks.

## Why It Matters

The escrow's authorization model assigns distinct powers to each party: the
recipient may `mark_holdback_escrow`, `release_escrow`, and
`release_holdback_escrow`; the sender may `refund_escrow` from `Locked`; the
driver receives the payout. Collapsing those roles onto one address gives a single
party unilateral control over the whole state machine.

The direct financial impact is limited — a self-dealing escrow moves the sender's
own funds, minus the platform fee — so this is not a theft vector. What it
produces is junk state that consumes `delivery_id` slots permanently (the
`DuplicateDelivery` guard means an ID is never reusable), pollutes the secondary
indexes, and inflates `TotalLocked` while occupying storage.

The stronger argument is parity: the protocol already decided that a driver must
not be the sender or recipient, and enforces it in one contract but not the other.
Two entry points to the same conceptual constraint disagreeing is the pattern that
produced the `Holdback` gap fixed in PR #187.

## Proposed Solution

Reject identical party addresses in `create_escrow` and `create_escrows_batch`,
mirroring `delivery_contract`'s `InvalidDriver` precedent. At minimum reject
`driver == sender` and `driver == recipient`, matching what the delivery contract
already enforces.

Whether `sender == recipient` should also be rejected is the same question issue
#201 raises for `create_delivery`; decide both consistently so the two contracts
agree.

## Acceptance Criteria

- [ ] `create_escrow` rejects `driver == sender` with a typed error
- [ ] `create_escrow` rejects `driver == recipient` with a typed error
- [ ] The `sender == recipient` decision matches whatever `delivery_contract` enforces
- [ ] `create_escrows_batch` applies the same rules
- [ ] Valid distinct-party escrows are unaffected
- [ ] The new error is documented in `docs/ERROR_CODES.md`

## Technical Notes

- `EscrowError` has nine variants; append a new one rather than renumbering.
- `delivery_contract::DeliveryError::InvalidDriver` is the naming and placement precedent.
- `create_escrows_batch` takes the driver per element and the recipient as a single parameter, so the check is per element against the shared sender and recipient.
- Existing tests generate distinct addresses via `Address::generate`, so they should be unaffected — verify rather than assume.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `create_escrow`, `create_escrows_batch`, `EscrowError`
- `contracts/delivery_contract/lib.rs` — `assign_driver`, `confirm_delivery` precedent
- `docs/ERROR_CODES.md`
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: `driver == sender` rejected
- Unit test: `driver == recipient` rejected
- Unit test: `sender == recipient` behaves per the agreed decision
- Unit test: batch path applies the same rules
- Regression test: normal distinct-party creation still succeeds
- Regression test: existing escrow tests pass unmodified

## Definition of Done

- [ ] Party validation implemented on both entry points
- [ ] Consistent with `delivery_contract`'s rules
- [ ] Error documented
- [ ] Tests added and passing

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Decide the `sender == recipient` rule jointly with #201 so the two contracts agree.

## Labels

`bug`, `security`


---

# Issue #282 — `examples/basic-usage.ts` demonstrates SDK methods that silently do nothing

## Problem Statement

`sdk/typescript/examples/basic-usage.ts` is the SDK's onboarding example and
references four escrow client methods including `createEscrow`, `releaseEscrow`,
and `getEscrow`. Every one of them is a stub:

```ts
async createEscrow(params, options?): Promise<string> {
  console.log('createEscrow', params);
  ...
}

async getEscrow(deliveryId: bigint): Promise<EscrowRecord> {
  console.log('getEscrow', deliveryId);
  return {} as EscrowRecord;
}
```

Running the example prints its arguments and completes successfully. No
transaction is submitted, and `getEscrow` returns an empty object typed as a
populated `EscrowRecord`.

## Why It Matters

This file is the first thing a prospective integrator runs. It succeeds, prints
plausible-looking output, and returns objects the type system vouches for — so the
natural conclusion is that the SDK works and the integration is done.

The failure surfaces much later, when the integrator discovers no escrows exist on
chain. Nothing in the example, the README, or the return types signals that
anything was simulated. A silently-succeeding example is worse than a missing one
because it actively produces false confidence.

Issue #222 covers implementing the invocation layer and #263 the missing clients;
this issue is specifically about the example and README misrepresenting the
package's current state, which is fixable immediately and independently of either.

## Proposed Solution

Make the SDK's current state unmistakable to anyone who reads or runs the example.
Either mark the example clearly as a non-functional API preview — a prominent
header comment plus a runtime warning — or have unimplemented methods throw an
explicit "not implemented" error, which makes the example fail loudly rather than
succeed falsely.

Throwing is the stronger option and aligns with issue #222's acceptance criteria.
Pair it with a status section in `sdk/typescript/README.md` stating which methods
are implemented, so documentation and behavior agree.

## Acceptance Criteria

- [ ] Running the example makes the SDK's non-functional state unmistakable
- [ ] No SDK method returns a fabricated object cast to a populated type
- [ ] `sdk/typescript/README.md` states which methods are implemented and which are not
- [ ] The example still compiles and type-checks
- [ ] The example remains a useful API-shape reference for future implementation
- [ ] The approach is consistent with whatever #222 establishes

## Technical Notes

- `return {} as EscrowRecord` is the specific pattern to remove: it defeats the type system exactly where an integrator relies on it.
- If methods throw, the example needs try/catch or a comment explaining the expected failure, so type-checking in CI (issue #248) still passes.
- Keep the example's structure intact — it documents the intended call shapes, which remains valuable.

## Relevant Files

- `sdk/typescript/examples/basic-usage.ts`
- `sdk/typescript/src/clients/escrow.client.ts`, `delivery.client.ts`
- `sdk/typescript/README.md`

## Testing Requirements

- Verification: the example compiles under `tsc`
- Verification: running it makes the unimplemented state obvious
- Verification: no method returns a fabricated typed object
- Documentation check: README status matches actual method behavior
- Regression: the example's demonstrated call shapes still match the client signatures

## Definition of Done

- [ ] Example and README honestly represent the SDK's state
- [ ] Fabricated return values removed
- [ ] Example type-checks
- [ ] Approach consistent with #222

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Should adopt the same convention as #222; independently landable if #222 has not started.

## Labels

`documentation`, `bug`


---

# Issue #283 — Translated READMEs have drifted from the English original

## Problem Statement

The repository maintains three translations alongside the main README:

```
README.md         389 lines
docs/fr/README.md 368 lines
docs/pt/README.md 368 lines
docs/sw/README.md 368 lines
```

All three translations are exactly 368 lines while the English original is 389 —
a 21-line divergence, identical across all three, indicating they were translated
from one earlier revision and never updated as the English file changed.

Closed issues #126, #127, and #128 all corrected content in `README.md` (a
fictional crate layout, broken badge and organization links, and incorrect version
claims). Those corrections were not propagated to the translations.

## Why It Matters

The translations were added as an explicit, prioritized roadmap item (closed issue
#79), so they represent deliberate investment in non-English-speaking
contributors. Stale translations serve those readers worse than no translation:
they present outdated repository structure, broken links, and incorrect version
information with the authority of official documentation.

Specifically, the corrections in #126–#128 are exactly the kind that mislead a
newcomer — a fictional crate layout tells them the project is organized in a way
it is not, and broken links send them to a nonexistent GitHub organization.

## Proposed Solution

Diff each translation against the English original to identify the divergent
sections, then update the translations to match current content. The 21-line gap
is consistent across all three, so the same set of sections is likely missing from
each.

Establish a maintenance approach so this does not recur — at minimum a note in
`docs/TRANSLATIONS.md` and `CONTRIBUTING.md` stating that changes to `README.md`
require corresponding translation updates, or a CI check comparing structural
markers (heading counts, link targets) across the four files.

This issue does not require the contributor to be fluent in all three languages:
identifying the divergence and updating structural content such as links, code
blocks, and directory layouts is language-independent, and prose translation can
be tracked separately if needed.

## Acceptance Criteria

- [ ] Each translation's structure matches the current English README
- [ ] Corrections from issues #126, #127, and #128 are reflected in all three translations
- [ ] Links, badges, and directory layouts match the English original
- [ ] Code blocks and commands are identical across all four files
- [ ] A maintenance expectation is documented for future README changes
- [ ] No translation claims a repository structure or version that does not exist

## Technical Notes

- The three translations being identical in length suggests a single shared base revision — diffing one may reveal the full change set for all three.
- Structural content (links, paths, commands, version numbers) is language-independent and is where the factual errors live.
- `docs/TRANSLATIONS.md` exists and is the natural place to record the maintenance expectation.
- A structural CI check comparing heading counts and link targets would catch future drift without requiring translation review.

## Relevant Files

- `README.md` — the authoritative original
- `docs/fr/README.md`, `docs/pt/README.md`, `docs/sw/README.md`
- `docs/TRANSLATIONS.md`
- `CONTRIBUTING.md`

## Testing Requirements

Documentation change; verification by comparison:

- [ ] Each translation diffed against the English original with divergences resolved
- [ ] All links in translations verified to resolve
- [ ] Version claims verified against the crate manifests
- [ ] Directory layouts verified against the actual repository structure
- [ ] Any added structural CI check verified to fail on deliberate drift

## Definition of Done

- [ ] All three translations updated to match current content
- [ ] Maintenance expectation documented
- [ ] Links and structural claims verified

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

**None**

## Labels

`documentation`


---

# Issue #284 — Crate version is duplicated across six manifests with no shared workspace metadata

## Problem Statement

The workspace root declares shared dependencies but no shared package metadata:

```toml
[workspace]
resolver = "2"
members = ["contracts/*"]

[workspace.dependencies]
soroban-sdk = "27.0.0"
```

There is no `[workspace.package]` section. Each of the six contract crates
declares its own version independently:

```toml
version = "0.2.0"
edition = "2021"
```

Releasing a new version therefore requires editing six files, and nothing prevents
them from diverging.

## Why It Matters

`soroban-sdk` was already centralized via `[workspace.dependencies]`, so the
pattern is established and the omission of `[workspace.package]` is an oversight
rather than a decision.

The project has a documented history of version drift: closed issue #128 covered
`README.md` and `SECURITY.md` claiming 0.2.x while the manifests still declared
0.1.0. Six independently maintained version fields make a partial bump — five
crates updated, one missed — easy and silent.

Issue #246 proposes a release-time check that the git tag matches the crate
version; that check is materially simpler and more meaningful when there is one
version to compare against rather than six that must first be proven equal.

## Proposed Solution

Add a `[workspace.package]` section declaring the shared `version`, `edition`, and
any other common metadata, and have each crate inherit with
`version.workspace = true` and `edition.workspace = true`.

This is a mechanical change with no effect on the built artifacts — verify by
confirming the release build still succeeds and produces the same contracts.

## Acceptance Criteria

- [ ] The root `Cargo.toml` declares `[workspace.package]` with a shared version and edition
- [ ] All six contract crates inherit both fields from the workspace
- [ ] `cargo metadata` reports the same version for every crate as before
- [ ] The workspace builds and all tests pass unchanged
- [ ] `cargo build --target wasm32v1-none --release` produces working contracts
- [ ] A single edit is sufficient to bump the whole workspace version

## Technical Notes

- `version.workspace = true` and `edition.workspace = true` are the inheritance syntax; both require the corresponding key in `[workspace.package]`.
- `shared_types` is a workspace member too and should inherit alongside the five contracts.
- Other candidates for centralization (`license`, `repository`, `authors`) are currently absent from the manifests — adding them is optional and should not expand this issue's scope.
- `Cargo.lock` will be rewritten; confirm the change is inert beyond the metadata.

## Relevant Files

- `Cargo.toml` — workspace root
- `contracts/*/Cargo.toml` — all six member manifests
- `Cargo.lock`

## Testing Requirements

- Verification: `cargo metadata --no-deps` reports unchanged versions for all crates
- Regression test: full workspace test suite passes
- Verification: release WASM build succeeds for all six contracts
- Verification: bumping `[workspace.package].version` propagates to every crate
- Verification: `cargo build --locked` still succeeds after the lockfile update

## Definition of Done

- [ ] Shared metadata centralized
- [ ] All crates inheriting
- [ ] Build and test suite unchanged
- [ ] Version bump verified to propagate from one edit

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Makes #246's release-time version check simpler; either can land first.

## Labels

`refactor`


---

# Issue #285 — `EscrowRecord` carries no `delivery_id`, unlike its delivery-side counterpart

## Problem Statement

`DeliveryRecord` identifies itself:

```rust
pub struct DeliveryRecord {
    pub delivery_id: DeliveryId,
    pub sender: Address,
    ...
}
```

`EscrowRecord` does not:

```rust
pub struct EscrowRecord {
    pub sender: Address,
    pub recipient: Address,
    pub driver: Address,
    pub token: Address,
    pub amount: i128,
    pub status: EscrowState,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub disputed_by: Option<Address>,
    pub disputed_at: Option<u64>,
    pub fleet_id: Option<u64>,
}
```

The escrow's identity lives only in its storage key, `StorageKey::Escrow(delivery_id)`.
`get_escrow(delivery_id)` returns a record that, once detached from the call, carries
no indication of which delivery it belongs to.

## Why It Matters

Cross-contract consumers receive these records detached from the key they were
fetched with. `delivery_contract::get_combined_state` returns
`(DeliveryRecord, EscrowRecord, bool)` — the delivery half is self-identifying and
the escrow half is not, so a client holding the tuple must rely on positional
correlation.

`dispute_resolution_contract` fetches `EscrowRecord` in three places
(`resolve_dispute_split_funds`, `force_resolve_dispute`, and its mock helpers) and
must track the delivery ID separately alongside each fetched record.

The asymmetry also surfaces in documentation: `docs/contract-design/escrow-design.md`
currently documents `EscrowRecord` as *having* a `delivery_id` field (issue #229
covers that inaccuracy), which suggests the field was intended.

The consequence is API ergonomics rather than incorrect behavior, which is why
this is Trivial — no current code misattributes a record.

## Proposed Solution

Add `delivery_id: u64` to `EscrowRecord` and populate it at every construction site,
matching `DeliveryRecord`'s self-identifying shape.

This changes the struct's XDR encoding, so it is a wire-format change affecting any
off-chain decoder and the SDK's `EscrowRecord` type. The contracts are pre-mainnet,
so no stored-data migration is required, but the change should be recorded in
`CHANGELOG.md` and reflected in the SDK (issue #223 already proposes auditing SDK
types against their Rust counterparts).

## Acceptance Criteria

- [ ] `EscrowRecord` carries a `delivery_id` field
- [ ] Every construction site populates it with the correct ID
- [ ] `get_escrow` returns a record whose `delivery_id` matches the requested ID
- [ ] `get_combined_state`'s two records agree on the delivery ID
- [ ] The SDK's `EscrowRecord` type is updated to match
- [ ] The wire-format change is recorded in `CHANGELOG.md`

## Technical Notes

- Construction sites are `create_escrow` and `create_escrows_batch` in `escrow_contract`, plus test helpers in `dispute_resolution_contract/test.rs` and `delivery_contract/test.rs`.
- `StorageKey::Escrow(delivery_id)` remains the storage key; the field is redundant with it by design, exactly as `DeliveryRecord.delivery_id` is redundant with `StorageKey::Delivery`.
- `DeliveryRecord` uses the `DeliveryId` newtype while escrow functions take a bare `u64` — pick one deliberately and note the choice.
- Issue #229 documents that the design doc already claims this field exists; correcting the doc and adding the field would reconcile the two.

## Relevant Files

- `contracts/shared_types/lib.rs` — `EscrowRecord`, `DeliveryRecord`
- `contracts/escrow_contract/lib.rs` — `create_escrow`, `create_escrows_batch`, `get_escrow`
- `contracts/delivery_contract/lib.rs` — `get_combined_state`
- `sdk/typescript/src/types/common.types.ts` — SDK counterpart
- `CHANGELOG.md`

## Testing Requirements

- Unit test: `get_escrow(id)` returns a record whose `delivery_id` equals `id`
- Unit test: batch-created escrows carry their correct individual IDs
- Regression test: `get_combined_state`'s records agree on the delivery ID
- Regression test: full workspace suite passes after the struct change
- Verification: SDK type updated and SDK builds

## Definition of Done

- [ ] Field added and populated everywhere
- [ ] SDK type updated
- [ ] `CHANGELOG.md` records the wire-format change
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Coordinate with #229 (which corrects the design doc's claim about this field) and #223 (SDK type parity).

## Labels

`refactor`, `enhancement`


---

# Issue #286 — `dispute_resolution_contract::init` uses a peer-contract key as its initialization sentinel

## Problem Statement

Five contracts guard re-initialization on their admin key:

```rust
if env.storage().instance().has(&StorageKey::Admin) {
    panic_with_error!(&env, FaniLabError::AlreadyInitialized);
}
```

`dispute_resolution_contract` guards on an unrelated configuration key instead:

```rust
if env.storage().instance().has(&DataKey::DeliveryContract) {
    panic_with_error!(&env, FaniLabError::AlreadyInitialized);
}
```

The contract does store admin state — `DataKey::Admin(admin)` and
`DataKey::AdminList` are both written later in the same function — so an
admin-based sentinel was available and was not used.

## Why It Matters

The sentinel is coupled to a field that has no logical relationship to whether the
contract is initialized. `DataKey::DeliveryContract` happens to be written first in
`init` today, which is the only reason the guard works.

That makes it fragile in a specific way: any future change that clears or
conditionally sets the delivery-contract address — for instance adding a setter to
repoint it, which the contract currently lacks and which
`identity_reputation_contract` already has for its equivalent fields — would
silently re-open initialization. An attacker could then call `init` again, adding
themselves to `AdminList` and gaining arbitration authority over every dispute.

Nothing exploitable exists today because no such setter exists. The issue is that
the guard's correctness depends on an unrelated invariant that nobody is
maintaining deliberately.

## Proposed Solution

Change the sentinel to a dedicated initialization marker or to the admin state the
contract already writes, matching the other five contracts.

Because `DataKey::Admin(Address)` is parameterized by address it cannot be used as
a presence check directly; `DataKey::AdminList` is the natural equivalent, or a
dedicated `DataKey::Initialized` key can be introduced.

Verify the change against the existing already-initialized test so the guard's
behavior is unchanged from the caller's perspective.

## Acceptance Criteria

- [ ] `init` guards on a sentinel that is logically tied to initialization
- [ ] A second `init` call still fails with `FaniLabError::AlreadyInitialized`
- [ ] A first `init` on a fresh contract still succeeds
- [ ] The guard no longer depends on `DataKey::DeliveryContract` being set
- [ ] The rationale is recorded in a code comment
- [ ] Existing initialization tests pass unmodified

## Technical Notes

- `DataKey::AdminList` is written unconditionally by `init` and is a reasonable sentinel.
- A dedicated `Initialized` key is clearer but adds a storage entry — either is defensible.
- Issue #261 adds `require_auth` to this same `init`; sequence the two to avoid conflicting edits.
- The contract has no setter for `DataKey::DeliveryContract` today, which is why no exploit path currently exists — do not let that absence be the thing the guard relies on.

## Relevant Files

- `contracts/dispute_resolution_contract/lib.rs` — `init`, `DataKey`
- Other contracts' `init` functions — reference for the standard pattern
- `contracts/dispute_resolution_contract/test.rs`

## Testing Requirements

- Regression test: second `init` fails with `AlreadyInitialized`
- Regression test: first `init` succeeds and stores all configuration
- Unit test: the guard holds independently of the delivery-contract address
- Regression test: existing setup helpers in the test suite still work
- Verification: no behavioral change visible to callers

## Definition of Done

- [ ] Sentinel changed to an initialization-specific marker
- [ ] Rationale documented in a comment
- [ ] Tests pass unmodified
- [ ] Formatting and clippy clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Touches the same function as #261; sequence to avoid conflicts.

## Labels

`refactor`, `security`


---

# Issue #287 — `escrow_contract` publishes the same event topics with two different payload shapes

## Problem Statement

Two escrow event topics are emitted with incompatible payloads depending on which
function emits them.

`escrow_refunded` — `refund_escrow` publishes the typed struct:

```rust
env.events().publish(
    (events::escrow_refunded(&env),),
    shared_types::EscrowRefundedEvent { delivery_id, sender: record.sender, amount: record.amount },
);
```

while `reclaim_expired_escrow` publishes a bare tuple with the ID moved into the
topics:

```rust
env.events().publish(
    (events::escrow_refunded(&env), delivery_id),
    (record.sender, record.amount),
);
```

`escrow_released` follows the same pattern: `release_escrow` emits the typed
`EscrowReleasedEvent`, while `release_holdback_escrow` emits
`(record.driver, driver_amount, platform_fee)` with `delivery_id` in the topics.

## Why It Matters

A consumer subscribing to `escrow_refunded` must handle two topic arities and two
payload encodings, and cannot tell which to expect without knowing which function
produced the event. Decoding the tuple form as the struct form, or vice versa,
fails or silently misreads fields.

`release_holdback_escrow` is not an edge case — it is the normal settlement path
for every confirmed delivery, so the divergent shape applies to the majority of
successful payouts rather than a rare branch.

This is the same class of defect as issues #196 (`escrow_funded` across the two
creation paths) and #204 (`delivery_created` across the two delivery paths), but at
two further call sites within `escrow_contract`. The typed structs exist in
`shared_types` precisely to prevent this, and closed issue #47 already flagged
unused typed event structs as a maintenance problem.

## Proposed Solution

Emit the typed `EscrowRefundedEvent` from `reclaim_expired_escrow` and the typed
`EscrowReleasedEvent` from `release_holdback_escrow`, with topic tuples matching
their counterparts.

If per-delivery topic filtering is genuinely wanted, change all emitters of a given
topic together so the shape stays uniform, and document the topic layout in
`docs/architecture/event-system.md` (issue #256 covers that document's broader
gaps).

## Acceptance Criteria

- [ ] `escrow_refunded` has one payload shape across all emitters
- [ ] `escrow_released` has one payload shape across all emitters
- [ ] Both use the typed structs from `shared_types`
- [ ] Topic tuples are consistent for each topic
- [ ] `docs/architecture/event-system.md` documents the canonical shapes
- [ ] Regression test asserts shape equivalence across emitters of each topic

## Technical Notes

- `EscrowRefundedEvent` and `EscrowReleasedEvent` are already declared in `shared_types` and already used by the typed emitters.
- `EscrowReleasedEvent` carries `delivery_id`, `driver`, `amount`, and `platform_fee` — matching what the tuple form emits, so no information is lost by switching.
- This is a wire-format change for off-chain consumers; record it in `CHANGELOG.md`.
- `resolve_dispute` and `resolve_dispute_split` emit `DisputeResolvedEvent`, which closed issue #88 already addressed — those are out of scope here.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `refund_escrow`, `reclaim_expired_escrow`, `release_escrow`, `release_holdback_escrow`
- `contracts/shared_types/lib.rs` — `EscrowRefundedEvent`, `EscrowReleasedEvent`
- `docs/architecture/event-system.md`
- `CHANGELOG.md`

## Testing Requirements

- Unit test: `refund_escrow` and `reclaim_expired_escrow` emit structurally identical events
- Unit test: `release_escrow` and `release_holdback_escrow` emit structurally identical events
- Unit test: payload fields carry correct values in every emitter
- Regression test: existing event assertions for the typed emitters still pass
- Verification: no emitter of these topics uses a bare tuple

## Definition of Done

- [ ] Both topics unified on their typed payloads
- [ ] Event shapes documented
- [ ] Tests added and passing
- [ ] `CHANGELOG.md` records the consumer-visible change
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Same class as #196 and #204; the three could share a convention but are in different functions and are independently landable.

## Labels

`bug`, `refactor`


---

# Issue #288 — `identity_reputation_contract` has no way to remove or deactivate a driver profile

## Problem Statement

`register_driver` creates a permanent `DriverProfile`:

```rust
pub fn register_driver(env: Env, driver: Address) {
    driver.require_auth();
    let key = DataKey::DriverProfile(driver.clone());
    if env.storage().persistent().has(&key) {
        panic_with_error!(&env, FaniLabError::AlreadyInitialized);
    }
    /* create and store profile */
}
```

There is no counterpart. The contract exposes no function that removes a profile,
marks a driver inactive, or suspends them. `update_driver_kyc_status` can set
`kyc_verified = false` and `decrease_reputation` can reduce a score, but the
profile itself persists indefinitely and the driver remains registered.

`fleet_management_contract` by contrast has a full membership lifecycle —
`add_driver_to_fleet`, `accept_fleet_invite`, `cancel_invite`,
`remove_driver_from_fleet`, and a `DriverFleetStatus::Removed` terminal state — and
`deactivate_fleet` for the fleet itself.

## Why It Matters

There is no on-chain mechanism to stop a driver from participating. A driver whose
key is compromised, who is banned for fraud, or who simply leaves the platform
keeps a valid profile and remains eligible for `assign_driver` — which requires
only that the caller is the admin or the driver themselves, with no check against
reputation, KYC status, or any suspension flag.

The closest available lever is driving reputation to zero via repeated
`decrease_reputation` calls, which is indirect, requires an authorized contract to
call it, and still leaves the driver assignable since nothing gates assignment on
score.

The asymmetry with fleet management is telling: the protocol already decided that
membership needs a lifecycle with a terminal state, and implemented one for fleets
but not for the identity registry those fleets draw from.

## Proposed Solution

Add an admin-gated capability to suspend or deactivate a driver profile, with a
status field on `DriverProfile` rather than deleting the record — history should be
preserved for audit, matching the `DriverFleetStatus::Removed` precedent that keeps
membership history rather than erasing it.

Keep the scope to the identity contract: adding the status and the admin function
to set it, plus an accessor. Wiring suspension into `assign_driver` is a separate
change in `delivery_contract` and should be a follow-up, since it involves a
cross-contract call that contract does not currently make for this purpose.

## Acceptance Criteria

- [ ] `DriverProfile` carries a status or active flag
- [ ] An admin-gated function can suspend and reinstate a driver
- [ ] Suspension is observable through an accessor
- [ ] Suspending preserves the profile's history rather than deleting it
- [ ] A suspension event is emitted
- [ ] Non-admin callers cannot suspend or reinstate
- [ ] Existing registration and reputation behavior is unchanged for active drivers

## Technical Notes

- `DriverProfile` is declared in `shared_types` and is shared with `delivery_contract` and `fleet_management_contract`; adding a field is a wire-format change affecting all three plus the SDK.
- `DriverFleetStatus` with its `Removed` terminal state is the in-repo precedent for preserving history.
- `register_driver` panics on re-registration, so suspension must not be implemented as deletion — a deleted profile would let a suspended driver simply re-register.
- Gating `assign_driver` on driver status is deliberately out of scope; note it as follow-up work.

## Relevant Files

- `contracts/identity_reputation_contract/lib.rs` — `register_driver`, `update_driver_kyc_status`, `get_driver_profile`
- `contracts/shared_types/lib.rs` — `DriverProfile`
- `contracts/fleet_management_contract/lib.rs` — `DriverFleetStatus` precedent
- `contracts/identity_reputation_contract/test.rs`

## Testing Requirements

- Unit test: admin can suspend a registered driver
- Unit test: admin can reinstate a suspended driver
- Authorization test: non-admin cannot suspend or reinstate
- Unit test: a suspended driver cannot re-register to reset their profile
- Unit test: suspension preserves reputation and `deliveries_completed`
- Event test: suspension and reinstatement emit correct events
- Regression test: existing registration and reputation flows unaffected

## Definition of Done

- [ ] Suspension capability implemented with history preserved
- [ ] Events emitted
- [ ] Tests above added and passing
- [ ] `docs/API.md` documents the new functions
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

**None**. Gating `assign_driver` on driver status is deliberately excluded and should be filed separately.

## Labels

`feature`


---

# Issue #289 — `get_escrow` reads storage twice and extends TTL on a read-only query

## Problem Statement

`get_escrow` performs a presence check and then a full load, which reads the same
entry again:

```rust
pub fn get_escrow(env: Env, delivery_id: u64) -> EscrowRecord {
    if !env.storage().persistent().has(&escrow_key(delivery_id)) {
        panic_with_error!(&env, EscrowError::DeliveryNotFound);
    }
    load_escrow(&env, delivery_id)
}
```

`load_escrow` already handles the missing case with the same error:

```rust
let record: EscrowRecord = env.storage().persistent().get(&key)
    .unwrap_or_else(|| panic_with_error!(env, EscrowError::DeliveryNotFound));
env.storage().persistent().extend_ttl(&key, ttl::LEDGER_TTL_THRESHOLD, ttl::LEDGER_TTL_EXTEND_TO);
```

So the `has` check is redundant — it produces the identical error `load_escrow`
would produce — and costs an extra storage read on every query.

`load_escrow` also extends the entry's TTL, which means `get_escrow`, a read-only
accessor, writes to the ledger as a side effect.

## Why It Matters

`get_escrow` is called cross-contract by `delivery_contract::get_combined_state`
and by `dispute_resolution_contract` in three places, in addition to direct client
queries. The redundant read is paid on every one of those calls.

The TTL extension is the more interesting half. It is arguably beneficial — reading
an escrow keeps it alive — but it makes a nominally read-only function mutate
ledger state, which means `get_escrow` cannot be used in a simulation-only context
without side effects, and it charges the caller for a write they did not request.

Whether that is intended is undocumented. `load_escrow` is shared by the mutating
functions, where TTL extension is clearly correct; `get_escrow` inherits it
incidentally.

## Proposed Solution

Remove the redundant `has` check, since `load_escrow` already produces the correct
error.

Then decide deliberately whether `get_escrow` should extend TTL. If read-driven
keep-alive is wanted, document it explicitly so callers understand the accessor
writes. If not, give `get_escrow` a non-extending read path separate from the one
the mutating functions use.

## Acceptance Criteria

- [ ] `get_escrow` performs a single storage read for the record
- [ ] A missing escrow still fails with `EscrowError::DeliveryNotFound`
- [ ] The TTL-extension behavior of `get_escrow` is decided and documented
- [ ] Mutating functions continue to extend TTL as they do today
- [ ] Cross-contract callers observe no behavioral change beyond the documented decision
- [ ] Regression test covers both the found and not-found paths

## Technical Notes

- `load_escrow` is used by every mutating function in the contract, so changing it directly would affect them all — prefer adjusting `get_escrow` rather than `load_escrow`.
- `delivery_contract::get_combined_state` and three sites in `dispute_resolution_contract` invoke `get_escrow` cross-contract.
- `escrow_contract` has no equivalent redundant check elsewhere; this appears to be an isolated pattern.
- Soroban charges separately for reads and for TTL extensions, so both halves of this issue have a measurable cost.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `get_escrow`, `load_escrow`
- `contracts/delivery_contract/lib.rs` — `get_combined_state`
- `contracts/dispute_resolution_contract/lib.rs` — escrow fetch sites
- `contracts/escrow_contract/test.rs` — `test_get_escrow_not_found`

## Testing Requirements

- Regression test: `get_escrow` on a missing ID still fails with `DeliveryNotFound`
- Regression test: `get_escrow` on an existing ID returns the correct record
- Unit test: TTL behavior matches the documented decision
- Regression test: cross-contract callers unaffected
- Verification: existing `test_get_escrow_not_found` passes unmodified

## Definition of Done

- [ ] Redundant read removed
- [ ] TTL behavior decided and documented
- [ ] Tests pass unmodified
- [ ] Formatting and clippy clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`performance`, `refactor`


---

# Issue #290 — `payout_driver` silently skips non-positive payouts with no event or error

## Problem Statement

`payout_driver` returns early when the amount is not positive:

```rust
fn payout_driver(env: &Env, token: &Address, driver: &Address, amount: i128,
                 fleet_management_addr: Option<&Address>, fleet_id: Option<u64>) {
    if amount <= 0 {
        return;
    }
    ...
}
```

The caller, `settle_escrow_funds`, does not check the return — the function returns
`()` — so a skipped payout is indistinguishable from a completed one. The
surrounding `release_escrow` and `release_holdback_escrow` still mark the escrow
`Released`, decrement `TotalLocked`, and emit `escrow_released` reporting a
`driver_amount` that was never transferred.

`resolve_dispute_split` guards its transfers the same way (`if sender_amount > 0`,
`if driver_amount > 0`) with the same silence.

## Why It Matters

The condition is reachable. If the platform fee equals or exceeds the escrowed
amount — a 10% fee on an amount of 9 or less, given integer division in
`calculate_fee` — `driver_amount` becomes zero and the driver receives nothing
while the escrow is recorded as successfully released to them.

`create_escrow` only requires `amount > 0`, so a 1-unit escrow is accepted, and
`create_escrows_batch` currently validates nothing at all (issue #189). The emitted
`escrow_released` event reports the computed `driver_amount`, so off-chain
accounting records a payout that did not occur.

The early return is defensively correct — attempting a zero transfer would be
wasteful or rejected — but doing it silently, while the caller reports success, is
what makes it a correctness problem rather than an optimization.

## Proposed Solution

Make a skipped payout observable. The lightest approach is to emit a distinct
event, or to include the actually-transferred amount in `escrow_released` so the
event reflects reality rather than the pre-transfer computation.

Alternatively, reject the condition earlier: enforce a minimum escrow amount at
creation such that `amount - platform_fee > 0` always holds, which removes the
reachable case entirely. That is arguably the better fix, since a zero-payout
release is not a meaningful protocol outcome.

Either way the current combination — silently skip, then report success — should
not stand.

## Acceptance Criteria

- [ ] A zero or negative driver payout is observable rather than silent
- [ ] The `escrow_released` event does not report an amount that was not transferred
- [ ] The equivalent condition in `resolve_dispute_split` is handled consistently
- [ ] Normal positive payouts are unchanged
- [ ] If a minimum amount is enforced, it is documented and validated at creation
- [ ] Regression test covers an escrow whose fee consumes the entire amount

## Technical Notes

- `calculate_fee` uses integer division: `amount * fee_bps / 10_000`, so small amounts round the fee down and `driver_amount` reaches zero only when the fee equals the full amount.
- `MAX_PLATFORM_FEE_BPS` is 1000 (10%), so with a 1-unit escrow the fee is 0 and the driver receives 1 — the reachable cases involve the fee equalling the amount, which requires specific small values; construct the test case deliberately rather than assuming.
- The platform-fee transfer in `settle_escrow_funds` is separately guarded by `if platform_fee > 0`, with the same silence.
- Issue #190 changes how the fee is computed and passed; coordinate if both are in flight.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `payout_driver`, `settle_escrow_funds`, `calculate_fee`, `resolve_dispute_split`, `release_escrow`, `release_holdback_escrow`
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: an escrow whose fee consumes the full amount produces an observable outcome
- Unit test: the emitted event's amounts match the amounts actually transferred
- Unit test: `resolve_dispute_split` with a zero share on one side behaves consistently
- Regression test: normal payouts with positive amounts unchanged
- Edge case: minimum viable escrow amount at the maximum fee rate
- Regression test: platform-fee transfer skipped at zero fee remains correct

## Definition of Done

- [ ] Silent skip made observable, or the condition made unreachable by validation
- [ ] Event amounts reflect reality
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Interacts with #190 (fee computation) and #189 (batch amount validation); independently solvable.

## Labels

`bug`


---

# Issue #291 — `DeliveryCounter` is never TTL-extended; if archived it resets to zero and overwrites existing deliveries

## Problem Statement

`delivery_contract` allocates delivery IDs from a counter held in **persistent**
storage:

```rust
let mut counter: u64 = env.storage().persistent()
    .get(&DataKey::DeliveryCounter).unwrap_or(0);
counter += 1;
env.storage().persistent().set(&DataKey::DeliveryCounter, &counter);
```

`DataKey::DeliveryCounter` appears at four sites in the file (init, both read
sites, both write sites) and **not one of them calls `extend_ttl`**. Every other
persistent entry in the contract — delivery records, `DeliveriesBySender`,
`DeliveriesByRecipient` — is extended immediately after being written.

If the counter entry is archived, the `unwrap_or(0)` fallback silently restarts
allocation from 1.

## Why It Matters

`create_delivery` has no duplicate guard. Unlike `escrow_contract::create_escrow`,
which rejects an existing `delivery_id` with `EscrowError::DuplicateDelivery`,
`create_delivery` writes unconditionally:

```rust
let key = delivery_key(delivery_id);
env.storage().persistent().set(&key, &record);
```

So a reset counter causes the next delivery to be written to
`StorageKey::Delivery(1)`, **overwriting the existing delivery record at that ID**
— its sender, recipient, driver, status, and timestamps all replaced. The escrow
keyed on the same `delivery_id` continues to reference funds that now belong to a
completely different delivery.

The counter is the one persistent entry in the contract that is never refreshed by
ordinary activity: delivery records and indexes are extended on every write, but
the counter is only touched by writes that do not extend it. A quiet period long
enough for archival is exactly the scenario the TTL constants exist to prevent.

## Proposed Solution

Extend the counter's TTL at every write site, matching the pattern used for
delivery records and the secondary indexes.

Consider additionally moving the counter to instance storage, whose lifetime is
tied to the contract itself rather than to a per-entry TTL — that removes the
failure mode structurally rather than relying on every future write site
remembering to extend.

As defense in depth, add a duplicate guard to `create_delivery` mirroring
`create_escrow`'s, so a counter fault can never silently overwrite a record.

## Acceptance Criteria

- [ ] `DataKey::DeliveryCounter` has its TTL extended at every write site
- [ ] The counter survives a long ledger advance without resetting
- [ ] `create_delivery` rejects an ID that already has a delivery record
- [ ] `create_deliveries_batch` applies the same guard
- [ ] Existing delivery-creation behavior is otherwise unchanged
- [ ] Regression test covers a counter that would otherwise have been archived

## Technical Notes

- `shared_types::ttl::{LEDGER_TTL_THRESHOLD, LEDGER_TTL_EXTEND_TO}` are already imported and used elsewhere in the file.
- Write sites are `init` (line ~113), `create_delivery` (line ~186), and `create_deliveries_batch` (line ~321).
- `escrow_contract::create_escrow`'s `DuplicateDelivery` check is the model for the defensive guard; `DeliveryError` would need a matching variant.
- Instance storage is extended by `extend_ttl` on the instance as a whole, which several contracts already do in admin functions — evaluate whether the counter belongs there.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `DataKey::DeliveryCounter`, `init`, `create_delivery`, `create_deliveries_batch`
- `contracts/shared_types/lib.rs` — `ttl` constants
- `contracts/escrow_contract/lib.rs` — `DuplicateDelivery` guard precedent
- `contracts/delivery_contract/test.rs`

## Testing Requirements

- Unit test: counter value survives a substantial ledger advance
- Unit test: creating a delivery at an already-used ID is rejected
- Regression test: sequential ID allocation is unchanged under normal use
- Regression test: batch creation allocates sequential IDs correctly
- Edge case: `init` followed immediately by creation allocates ID 1

## Definition of Done

- [ ] Counter TTL handled consistently with the contract's other persistent entries
- [ ] Duplicate guard added to both creation paths
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

4–8 hours

## Dependencies

**None**

## Labels

`bug`, `security`

---

# Issue #292 — Driver-initiated disputes always fail because `delivery_contract` rejects the caller the dispute contract accepts

## Problem Statement

`dispute_resolution_contract::raise_dispute` explicitly permits all three parties:

```rust
if caller != delivery.sender
    && caller != delivery.recipient
    && Some(caller.clone()) != delivery.driver
{
    panic_with_error!(&env, FaniLabError::Unauthorized);
}
```

It then cross-calls the delivery contract to advance the delivery state:

```rust
let _: () = env.invoke_contract(
    &delivery_contract_addr,
    &Symbol::new(&env, "raise_dispute"),
    soroban_sdk::vec![&env, caller.into_val(&env), delivery_id.into_val(&env)],
);
```

`delivery_contract::raise_dispute` permits only two:

```rust
let is_sender = caller == delivery.sender;
let is_recipient = caller == delivery.recipient;
if !is_sender && !is_recipient {
    panic_with_error!(&env, FaniLabError::Unauthorized);
}
```

A driver passes the dispute contract's check and is then rejected by the delivery
contract, reverting the whole transaction.

## Why It Matters

Driver access to disputes was added deliberately — closed issue #100 reported that
"drivers are structurally excluded from the entire dispute process", and the fix
extended `dispute_resolution_contract`'s authorization. That fix is inert: the
delivery contract's own check was never widened to match, so the capability does
not work end to end.

The driver is the party with the most at stake in a contested delivery — they have
performed the work and are awaiting payment — and they currently have no way to
initiate a dispute through any path. The failure surfaces as a bare
`Unauthorized` originating in a different contract, which makes it look like a
permissions misconfiguration rather than a protocol gap.

`docs/protocol/delivery-protocol.md` compounds the confusion by documenting the
transition as available to "sender or driver" (see issue #302), which is neither
what the delivery contract enforces nor what the dispute contract enforces.

## Proposed Solution

Widen `delivery_contract::raise_dispute` to accept the assigned driver alongside
the sender and recipient, so the two contracts agree.

Confirm the intended authorization set deliberately: `dispute_resolution_contract`
allows sender, recipient, and driver, so matching that is the natural target.
Update the protocol documentation in the same change so all three sources agree.

## Acceptance Criteria

- [ ] A driver can raise a dispute through `dispute_resolution_contract` end to end
- [ ] `delivery_contract::raise_dispute` accepts the assigned driver
- [ ] Sender and recipient continue to be accepted
- [ ] A non-party is still rejected with `Unauthorized`
- [ ] An address that is not the *assigned* driver for that delivery is rejected
- [ ] `docs/protocol/delivery-protocol.md` documents the actual authorization set
- [ ] Regression test drives a driver-initiated dispute through both contracts

## Technical Notes

- `delivery.driver` is `Option<Address>`; the check must handle an unassigned delivery, where there is no driver to authorize.
- The dispute contract already compares with `Some(caller.clone()) != delivery.driver`, which is the pattern to mirror.
- `escrow_contract::raise_dispute` accepts sender, recipient, and driver, so the escrow layer is already consistent with the intended set — only the delivery contract diverges.
- Note the interaction with issue #193: for a `Delivered` delivery the escrow is in `Holdback`, which `escrow_contract::raise_dispute` currently rejects, so the end-to-end driver path also needs that fix to work post-delivery.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `raise_dispute`
- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute`
- `contracts/escrow_contract/lib.rs` — `raise_dispute` (already permits all three)
- `docs/protocol/delivery-protocol.md`

## Testing Requirements

- Integration test: driver raises a dispute through `dispute_resolution_contract` and it succeeds
- Unit test: `delivery_contract::raise_dispute` accepts the assigned driver
- Authorization test: a non-party is rejected
- Authorization test: an address that is not the assigned driver is rejected
- Edge case: raising a dispute on an unassigned delivery with no driver
- Regression test: sender- and recipient-initiated disputes unchanged

## Definition of Done

- [ ] Authorization aligned across both contracts
- [ ] End-to-end driver dispute test passing
- [ ] Protocol documentation corrected
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

4–8 hours

## Dependencies

For post-delivery disputes the driver path also requires #193; pre-delivery disputes work independently.

## Labels

`bug`, `security`

---

# Issue #293 — `freeze_funds` reports success without freezing when the escrow is in a terminal state

## Problem Statement

`escrow_contract::freeze_funds` guards its state change with an `if` that has no
`else`:

```rust
let mut record = load_escrow(&env, delivery_id);
if record.status == EscrowStatus::Locked || record.status == EscrowStatus::Holdback {
    record.status = EscrowStatus::Paused;
    record.disputed_at = Some(env.ledger().timestamp());
    save_escrow(&env, delivery_id, &record);
    env.events().publish(/* funds_frozen */);
}
// no else — function returns successfully having done nothing
```

If the escrow is `Released`, `Refunded`, `Split`, or already `Paused`, the
function returns `()` normally. No error is raised, no event is emitted, and the
caller cannot distinguish this from a successful freeze.

Every other state-guarded function in the contract panics with
`EscrowError::InvalidState` when its precondition is not met.

## Why It Matters

`freeze_funds` is called by `dispute_resolution_contract::raise_dispute` as the
step that secures the funds before a dispute is opened:

```rust
let _: () = env.invoke_contract(&escrow_addr, &Symbol::new(&env, "freeze_funds"), ...);

let dispute_key = DataKey::Dispute(delivery_id);
if env.storage().persistent().has(&dispute_key) { /* DuplicateDelivery */ }
/* create and store the DisputeCase */
```

The return value is discarded, so a silent no-op is indistinguishable from
success. The dispute contract proceeds to record an `Open` `DisputeCase` while
the escrow was never actually frozen — and if the escrow was already `Released`,
the funds are gone.

The result is a dispute that exists on chain, appears actionable, and can never
be resolved: `resolve_dispute_split_funds` requires the escrow to be `Paused` and
will revert, while `resolve_dispute_refund_sender` and `resolve_dispute_pay_driver`
will revert inside the escrow's own guard. The dispute is stuck `Open` with no
path forward.

## Proposed Solution

Panic with `EscrowError::InvalidState` when the escrow is not in a freezable
state, matching every other state-guarded function in the contract. The dispute
contract's transaction then reverts cleanly instead of recording an unresolvable
dispute.

Decide explicitly how an already-`Paused` escrow should behave: treating a
re-freeze as a successful no-op is defensible, but it should be a documented
decision rather than a side effect of the missing `else`.

## Acceptance Criteria

- [ ] `freeze_funds` rejects a `Released`, `Refunded`, or `Split` escrow with a typed error
- [ ] The behavior for an already-`Paused` escrow is decided and documented
- [ ] Freezing a `Locked` or `Holdback` escrow works exactly as today
- [ ] `dispute_resolution_contract::raise_dispute` reverts rather than recording an unfreezable dispute
- [ ] The caller-restriction to the configured dispute contract is unchanged
- [ ] Regression test covers each terminal state

## Technical Notes

- `freeze_funds` is intentionally exempt from `require_not_paused` so escrows can be frozen during a protocol halt — that exemption is documented in a code comment and must be preserved.
- The caller check restricting this to `DataKey::DisputeResolutionContract` is correct and should not change.
- `dispute_resolution_contract::raise_dispute` calls `freeze_funds` *after* advancing the delivery to `Disputed`, so a revert also rolls back that transition — verify the ordering still produces coherent state.
- `test_freeze_funds_remains_available_while_paused` and `test_freeze_funds_unauthorized_caller_rejected` are the existing tests to extend.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `freeze_funds`
- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute`
- `contracts/escrow_contract/test.rs` — existing `freeze_funds` tests

## Testing Requirements

- Unit test: `freeze_funds` on a `Released` escrow → typed error
- Unit test: same for `Refunded` and `Split`
- Unit test: already-`Paused` escrow behaves per the documented decision
- Integration test: raising a dispute against a released delivery reverts rather than creating a stuck dispute
- Regression test: freezing `Locked` and `Holdback` escrows still works
- Regression test: existing pause-exemption and authorization tests unchanged

## Definition of Done

- [ ] Silent no-op replaced with a typed error
- [ ] Re-freeze semantics documented
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`bug`, `security`

---

# Issue #294 — `create_escrow` never verifies that the delivery it funds exists

## Problem Statement

`escrow_contract::create_escrow` accepts an arbitrary `delivery_id` and makes no
cross-contract call to confirm a corresponding delivery record exists. The
function performs zero `invoke_contract` calls before writing the escrow:

```rust
sender.require_auth();
require_not_paused(&env);
if amount <= 0 { /* InvalidAmount */ }
if env.storage().persistent().has(&escrow_key(delivery_id)) { /* DuplicateDelivery */ }
let config = load_protocol_config(&env);
if token != config.token { /* InvalidToken */ }
token::Client::new(&env, &token).transfer(&sender, env.current_contract_address(), &amount);
save_escrow(/* ... */);
```

The `recipient` and `driver` are likewise taken on the sender's word rather than
read from the delivery record.

## Why It Matters

An escrow can be created for a `delivery_id` that does not exist, or that exists
with entirely different parties. Nothing reconciles the two records at creation
time, and the mismatch only surfaces later:
`delivery_contract::get_combined_state` would report desynchronization, and
`confirm_delivery`'s call to `mark_holdback_escrow` would operate on an escrow
whose recipient is not the delivery's recipient.

The practical consequence is orphaned or mismatched escrows that consume a
`delivery_id` permanently — the `DuplicateDelivery` guard means the ID can never
be reused, so a mistyped ID blocks the real delivery from ever being funded.

This is the mirror of issue #295: the delivery contract assumes an escrow exists,
and the escrow contract assumes a delivery exists, and neither verifies. The two
records are only ever correlated by convention.

Impact is bounded — the sender is escrowing their own funds and cannot take
anyone else's — so this is a data-integrity and liveness concern rather than a
theft vector.

## Proposed Solution

Have `create_escrow` cross-call `delivery_contract::get_delivery` to confirm the
delivery exists, and verify that the supplied `recipient` and `driver` match the
delivery record.

This requires the escrow contract to know the delivery contract's address, which
it does not currently store. Adding a `set_delivery_contract` admin setter
mirrors how `delivery_contract` already stores `DataKey::EscrowContract`, and how
`escrow_contract` already stores the dispute and fleet contract addresses.

Make the check conditional on the delivery contract being configured, so existing
deployments and tests that do not wire it continue to work — matching how the
fleet and settlement integrations are already optional.

## Acceptance Criteria

- [ ] `escrow_contract` can be configured with the delivery contract's address
- [ ] When configured, `create_escrow` rejects a `delivery_id` with no delivery record
- [ ] When configured, it rejects a `recipient` or `driver` that disagrees with the delivery record
- [ ] When not configured, behavior is unchanged
- [ ] The setter is admin-gated
- [ ] Regression test covers both configured and unconfigured paths

## Technical Notes

- `delivery_contract::get_delivery` panics with `FaniLabError::DeliveryNotFound` for an unknown ID, so a failed lookup naturally reverts the escrow creation.
- The delivery's `driver` is `Option<Address>` and is `None` until `assign_driver` runs, so the driver comparison must tolerate an unassigned delivery or the ordering constraint must be documented.
- `create_escrows_batch` should receive the same treatment for consistency.
- This adds a cross-contract call to the hot creation path; weigh the resource cost against the integrity benefit and note the decision.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `create_escrow`, `create_escrows_batch`, `DataKey`
- `contracts/delivery_contract/lib.rs` — `get_delivery`, `DataKey::EscrowContract` precedent
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: escrow creation for a nonexistent delivery is rejected when configured
- Unit test: mismatched recipient is rejected
- Unit test: mismatched driver is rejected, or the unassigned case is handled as documented
- Regression test: creation with no delivery contract configured behaves as today
- Authorization test: only an admin can set the delivery contract address
- Integration test: normal create-delivery-then-create-escrow flow still works

## Definition of Done

- [ ] Optional delivery verification implemented
- [ ] Setter added and documented in `docs/API.md`
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Conceptually paired with #295; each is independently solvable.

## Labels

`bug`, `enhancement`

---

# Issue #295 — `cancel_delivery` cannot cancel a delivery that has no escrow

## Problem Statement

`delivery_contract::cancel_delivery` unconditionally cross-calls the escrow
contract before updating its own state:

```rust
validate_transition(delivery.status, DeliveryStatus::Cancelled)
    .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));

let escrow_address: Address = /* ... */;
let _: () = env.invoke_contract(
    &escrow_address,
    &soroban_sdk::Symbol::new(&env, "refund_escrow"),
    soroban_sdk::vec![&env, sender.into_val(&env), u64::from(delivery_id).into_val(&env)],
);

delivery.status = DeliveryStatus::Cancelled;
```

`escrow_contract::refund_escrow` begins with `load_escrow`, which panics with
`EscrowError::DeliveryNotFound` when no escrow record exists. The panic propagates
and reverts the whole cancellation.

Delivery creation and escrow creation are separate calls on separate contracts —
`create_delivery` never funds an escrow (see issue #203) — so a delivery with no
escrow is an ordinary, reachable state.

## Why It Matters

A sender who creates a delivery and then does not fund an escrow — because they
changed their mind, the funding transaction failed, or they simply never got to it
— has a delivery record they can never cancel. It remains `Pending` permanently,
occupying a `delivery_id` and appearing in the sender's and recipient's secondary
indexes indefinitely.

There is no alternative exit. `cancel_delivery` is the only path to
`DeliveryStatus::Cancelled`, and the other transitions require a driver
assignment and a funded escrow to be meaningful.

The failure is also opaque: the sender receives `DeliveryNotFound` from the escrow
contract when cancelling a delivery that plainly exists, which reads as a bug in
the caller rather than a missing precondition.

## Proposed Solution

Make the escrow refund conditional on an escrow existing. The escrow contract
would need a non-panicking existence check — `has_escrow(delivery_id) -> bool`, or
`get_escrow` returning `Option` — so the delivery contract can skip the refund
when there is nothing to refund and still complete the cancellation.

Alternatively, tolerate the specific `DeliveryNotFound` failure from the escrow
call and proceed, though Soroban's error handling makes a positive existence check
the cleaner shape.

Preserve the existing ordering guarantee: the escrow call must still run before
the delivery's state is mutated, so a genuine refund failure cannot leave the
delivery cancelled with funds still locked.

## Acceptance Criteria

- [ ] A delivery with no escrow can be cancelled
- [ ] A delivery with an escrow still triggers the refund before its state changes
- [ ] A genuine refund failure still reverts the whole cancellation
- [ ] The cancelled delivery's state and events are correct in both cases
- [ ] Authorization is unchanged — only the sender may cancel
- [ ] Regression test covers cancellation both with and without an escrow

## Technical Notes

- `escrow_contract` currently exposes no non-panicking existence check; `get_escrow` panics via an explicit `has` guard and `load_escrow` panics on a miss.
- The `#[cfg(test)] MockEscrowContract` in `delivery_contract/test.rs` will need a matching method for whichever accessor is added.
- Issue #204's note about cross-contract-call ordering applies: the escrow interaction deliberately precedes local state mutation so a failure rolls everything back.
- Closed issue #95 added rollback coverage for failing escrow calls; extend that suite rather than duplicating it.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `cancel_delivery`
- `contracts/escrow_contract/lib.rs` — `refund_escrow`, `load_escrow`, `get_escrow`
- `contracts/delivery_contract/test.rs` — `MockEscrowContract`

## Testing Requirements

- Unit test: cancelling a delivery with no escrow succeeds and sets `Cancelled`
- Regression test: cancelling a delivery with an escrow still refunds the sender
- Regression test: a failing refund still reverts the cancellation
- Authorization test: a non-sender still cannot cancel
- State test: cancellation from `Pending` and from `Active` both behave correctly
- Event test: `delivery_cancelled` emitted in both the escrow and no-escrow cases

## Definition of Done

- [ ] Cancellation works without an escrow
- [ ] Refund ordering and rollback behavior preserved
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Conceptually paired with #294; each is independently solvable.

## Labels

`bug`

---

# Issue #296 — `create_escrow` never validates `fleet_id`, letting the sender choose where the driver's payout is routed

## Problem Statement

`create_escrow` takes `fleet_id: Option<u64>` from the caller and stores it on the
`EscrowRecord` without any validation — the identifier appears exactly once in the
function, at the point it is written into the record.

At settlement, that stored value decides the payout destination:

```rust
if let (Some(fleet_addr), Some(fid)) = (fleet_management_addr, fleet_id) {
    let treasury: Address = env.invoke_contract(
        fleet_addr, &Symbol::new(env, "get_payout_address"),
        soroban_sdk::vec![env, driver.into_val(env), fid.into_val(env)]);
    payout_address = treasury;
}
```

`fleet_management_contract` keys membership as
`DataKey::DriverFleet(fleet_id, driver)`, so a driver may be `Active` in any
number of fleets simultaneously. Nothing constrains which of them the sender may
name, and nothing ties the chosen fleet to the delivery.

## Why It Matters

The sender — the party paying, and the party whose interests are opposite the
driver's on payout — unilaterally selects the fleet whose treasury receives the
driver's earnings.

Two concrete consequences follow. A sender can **omit** `fleet_id` for a driver
who is an active fleet member, routing the payment to the driver personally and
bypassing the fleet's arrangement entirely. Or a sender can **name** a fleet the
driver belongs to but which had nothing to do with this delivery, diverting the
earnings to that fleet's treasury.

Neither the driver nor the fleet consents to or can observe the choice: the
`fleet_id` is fixed at escrow creation, before the driver is necessarily even
assigned, and `EscrowRecord.fleet_id` is immutable thereafter.

This is a real authorization gap rather than a theft vector — the funds reach a
legitimate party either way — but "which legitimate party" is precisely what fleet
routing exists to determine, and it is currently the sender's unilateral call.

## Proposed Solution

Validate the claimed fleet relationship at settlement rather than trusting the
stored value. `get_payout_address` already receives both the driver and the fleet
ID and already returns the driver's own address when membership is not `Active`,
so the membership check exists — what is missing is any constraint on the sender's
ability to pick a fleet, or to decline to.

The more robust direction is to stop taking `fleet_id` from the sender at all and
resolve the driver's fleet at settlement time from the fleet contract. That
requires a driver-to-fleet lookup the contract does not currently expose, since
membership is keyed by `(fleet_id, driver)` rather than by driver.

Whichever direction is chosen, the outcome should be that the driver's fleet
affiliation determines routing, not the sender's declaration.

## Acceptance Criteria

- [ ] A sender cannot route a driver's payout to a fleet the driver is not active in
- [ ] A sender cannot bypass a driver's active fleet arrangement by omitting `fleet_id`
- [ ] Routing for a driver with no fleet membership is unchanged
- [ ] Routing for a driver in exactly one fleet is unchanged
- [ ] The behavior for a driver active in multiple fleets is defined and documented
- [ ] Regression test covers a sender naming a fleet the driver does not belong to

## Technical Notes

- `DataKey::DriverFleet(FleetId, Address)` means membership lookup requires knowing the fleet ID; a driver-to-fleets index does not exist and would need adding for the settlement-time resolution approach.
- `get_payout_address` already returns the driver's address for `Pending`, `Removed`, and `None` statuses, so an invalid claim currently degrades to a direct payout rather than failing — that is the existing safety net.
- Issue #217 covers a related but distinct problem: that routing is resolved at payout time from mutable fleet state. This issue is about who gets to assert the fleet in the first place.
- Issue #272 proposes adding `fleet_id` to the batch creation path; whatever validation is agreed here should apply there too.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `create_escrow`, `payout_driver`, `settle_escrow_funds`
- `contracts/fleet_management_contract/lib.rs` — `get_payout_address`, `DataKey::DriverFleet`
- `contracts/shared_types/lib.rs` — `EscrowRecord.fleet_id`

## Testing Requirements

- Integration test: sender names a fleet the driver is not a member of → payout does not reach that treasury
- Integration test: sender omits `fleet_id` for an active fleet driver → behavior matches the agreed policy
- Regression test: driver with no fleet is paid directly
- Regression test: driver in one fleet routes to that treasury
- Edge case: driver active in two fleets — documented behavior asserted
- Authorization test: the driver's own fleet membership governs the outcome

## Definition of Done

- [ ] Fleet routing determined by the driver's membership rather than the sender's claim
- [ ] Multi-fleet behavior documented
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

1–2 days

## Dependencies

Related to #217 and #272; each addresses a different aspect of fleet routing and all three are independently solvable.

## Labels

`security`, `bug`

---

# Issue #297 — `mark_in_transit` advances a delivery without verifying the escrow is funded

## Problem Statement

`delivery_contract::mark_in_transit` checks the caller is the assigned driver and
that the transition `Active → InTransit` is legal, then writes the new status. It
makes no cross-contract call and never consults the escrow:

```rust
match &delivery.driver {
    Some(assigned) if *assigned == driver => {}
    _ => panic_with_error!(&env, FaniLabError::Unauthorized),
}
validate_transition(delivery.status, DeliveryStatus::InTransit)
    .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));
delivery.status = DeliveryStatus::InTransit;
```

`assign_driver` behaves the same way. Because escrow creation is a separate call
on a separate contract, a delivery can reach `InTransit` with no escrow at all, or
with an escrow that was already refunded.

## Why It Matters

A driver has no on-chain assurance that funds exist before they begin work. The
protocol's value proposition is that the driver is paid from an escrow secured
before delivery, and nothing enforces that the escrow is actually there and
`Locked` at the moment the driver commits.

The reverse desynchronization is also reachable: `reclaim_expired_escrow` can
refund a `Locked` escrow without touching the delivery (issue #299), so a driver
could mark a delivery in transit against an escrow that has already been returned
to the sender.

`get_combined_state` exists precisely to detect these mismatches, which
acknowledges they occur — but detection after the fact does not help a driver who
has already collected the package.

## Proposed Solution

Have `mark_in_transit` verify the escrow is present and `Locked` before advancing,
via a cross-call to `escrow_contract::get_escrow`. The delivery contract already
stores `DataKey::EscrowContract` and cross-calls it in `cancel_delivery`,
`confirm_delivery`, and `raise_dispute`, so the wiring exists.

Consider whether `assign_driver` warrants the same check — assignment is a weaker
commitment than transit, so gating transit alone may be the right balance. Decide
deliberately and document it.

## Acceptance Criteria

- [ ] `mark_in_transit` rejects a delivery with no corresponding escrow
- [ ] It rejects a delivery whose escrow is not `Locked`
- [ ] A delivery with a `Locked` escrow transitions exactly as today
- [ ] The failure carries a typed, diagnosable error
- [ ] The decision on `assign_driver` is documented
- [ ] Regression test covers transit attempted against a missing and a refunded escrow

## Technical Notes

- `escrow_contract::get_escrow` panics with `DeliveryNotFound` for a missing escrow, so a bare cross-call surfaces the missing case — but a non-panicking existence accessor (issue #311) would give a cleaner error.
- The `MockEscrowContract` in `delivery_contract/test.rs` returns a hardcoded `Locked` escrow, so existing tests would pass unchanged; issue #231 covers making that mock model real state.
- Adding a cross-call to `mark_in_transit` increases its resource cost on a hot path — weigh and note the trade-off.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `mark_in_transit`, `assign_driver`, `DataKey::EscrowContract`
- `contracts/escrow_contract/lib.rs` — `get_escrow`
- `contracts/delivery_contract/test.rs` — `MockEscrowContract`

## Testing Requirements

- Unit test: `mark_in_transit` with no escrow → typed rejection
- Unit test: `mark_in_transit` with a `Refunded` escrow → typed rejection
- Regression test: `mark_in_transit` with a `Locked` escrow succeeds
- Authorization test: a non-assigned driver is still rejected
- Integration test against the real escrow contract, not only the mock

## Definition of Done

- [ ] Escrow verification added to the transit transition
- [ ] `assign_driver` decision documented
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Benefits from #311's existence accessor and #231's realistic mock; solvable without either.

## Labels

`bug`, `security`

---

# Issue #298 — Three admin setters in `escrow_contract` do not extend instance-storage TTL

## Problem Statement

`escrow_contract` stores its configuration in instance storage. Some admin
functions extend the instance TTL after writing and some do not:

| Function | extends instance TTL |
|---|---|
| `update_platform_fee` | yes |
| `set_dispute_resolution_contract` | yes |
| `propose_admin` | yes |
| `update_slippage_tolerance` | **no** |
| `set_fleet_management_contract` | **no** |
| `set_paused` | **no** |

The three that do call
`env.storage().instance().extend_ttl(ttl::LEDGER_TTL_THRESHOLD, ttl::LEDGER_TTL_EXTEND_TO)`;
the three that do not simply write and return.

## Why It Matters

Instance storage holds the admin address, `ProtocolConfig`, the paused flag, and
every peer-contract address. If the instance entry is archived, the contract
reverts to an uninitialized state — `load_protocol_config` panics with
`NotInitialized`, and `is_protocol_paused` falls back to `unwrap_or(false)`.

That fallback is the sharpest edge: **a paused protocol would silently become
unpaused** if the instance entry lapsed, because absence is read as "not paused".
`set_paused` is the one function whose write most needs to persist, and it is one
of the three that does not extend.

In practice ordinary escrow activity extends the instance TTL through the
functions that do call it, so this is a latent risk rather than an active fault —
but a protocol paused during an incident is precisely the period when ordinary
activity has stopped.

Closed issue #25 previously reported that instance TTL was extended by only two of
many admin writers; that fix covered three functions and left these three behind.

## Proposed Solution

Add the instance TTL extension to the three functions that lack it, matching the
three that have it.

Consider extracting a small helper so the pattern is applied uniformly and future
admin functions cannot omit it — the repetition across six call sites is what
allowed the gap to persist through one round of fixes.

## Acceptance Criteria

- [ ] `update_slippage_tolerance`, `set_fleet_management_contract`, and `set_paused` extend instance TTL
- [ ] The three functions that already extend are unchanged
- [ ] The paused flag survives a long ledger advance with no other activity
- [ ] A shared helper or equivalent guard prevents future omissions
- [ ] Regression test covers pause state persisting across a ledger advance

## Technical Notes

- `shared_types::ttl::{LEDGER_TTL_THRESHOLD, LEDGER_TTL_EXTEND_TO}` are already imported and used in this file.
- `is_protocol_paused` reads with `unwrap_or(false)`, which is why archival fails open rather than closed — worth a comment noting the consequence.
- Other contracts have the same pattern of per-function extension; a follow-up audit across all six would be reasonable but should be filed separately rather than expanding this issue.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `update_slippage_tolerance`, `set_fleet_management_contract`, `set_paused`, `is_protocol_paused`
- `contracts/shared_types/lib.rs` — `ttl` constants

## Testing Requirements

- Unit test: paused state persists after a substantial ledger advance
- Unit test: slippage tolerance persists similarly
- Unit test: fleet contract address persists similarly
- Regression test: existing admin function behavior unchanged
- Verification: every instance-writing function in the contract extends TTL

## Definition of Done

- [ ] TTL extension applied consistently
- [ ] Guard against future omissions in place
- [ ] Tests above added and passing
- [ ] Formatting and clippy clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**. Distinct from the closed issue #25, which covered a different subset of functions.

## Labels

`bug`, `security`

---

# Issue #299 — `reclaim_expired_escrow` refunds the sender but leaves the delivery record stranded

## Problem Statement

`reclaim_expired_escrow` is permissionless and refunds an expired `Locked` escrow
to the sender. It makes **zero** cross-contract calls — the delivery record is
never informed:

```rust
record.status = EscrowStatus::Refunded;
save_escrow(&env, delivery_id, &record);
/* TotalLocked decremented, tokens transferred to sender */
env.events().publish((events::escrow_refunded(&env), delivery_id), (record.sender, record.amount));
```

The corresponding `DeliveryRecord` retains whatever status it held — `Pending`,
`Active`, or `InTransit` — indefinitely.

## Why It Matters

The protocol ends up in a state its own consistency check classifies as invalid.
`validate_state_sync` maps `Cancelled → Refunded` as the only synchronized pairing
involving a refund, so a reclaimed escrow leaves combinations like
`(Active, Refunded)` that `get_combined_state` reports as desynchronized —
correctly, but with no mechanism to resolve it.

The delivery is also functionally stuck. Its escrow is gone, so `confirm_delivery`
would fail at `mark_holdback_escrow`, and `cancel_delivery` would fail at
`refund_escrow` because the escrow is no longer `Locked` (and see issue #295 for
the missing-escrow case). A driver may still be assigned and believe the job is
live.

Because `reclaim_expired_escrow` is callable by anyone, this state can be induced
by any third party once the 30-day expiry has passed, without the sender's or
driver's involvement.

## Proposed Solution

Have the reclaim path transition the delivery to `Cancelled`, restoring the
synchronized pairing the state machine already defines for a refund.

That requires the escrow contract to hold the delivery contract's address and
cross-call it, which it does not do today — the same wiring issue #294 proposes.
An alternative is a delivery-side `reclaim` entry point that drives both
contracts in the correct order, keeping the cross-contract direction consistent
with the existing `delivery → escrow` flow.

Whichever direction is chosen, `validate_transition` must permit the resulting
delivery transition: `InTransit → Cancelled` is not currently legal and would need
to be added deliberately, or the reclaim restricted to deliveries in states from
which cancellation is already valid.

## Acceptance Criteria

- [ ] Reclaiming an expired escrow leaves delivery and escrow states synchronized
- [ ] `get_combined_state` reports synchronized after a reclaim
- [ ] The permitted delivery transitions are decided explicitly and reflected in `validate_transition`
- [ ] Reclaim remains permissionless
- [ ] The expiry precondition and `Locked`-only guard are unchanged
- [ ] Regression test asserts post-reclaim synchronization from each reachable delivery status

## Technical Notes

- `validate_transition` currently allows `Pending → Cancelled` and `Active → Cancelled` but **not** `InTransit → Cancelled`; a reclaim of an in-transit delivery therefore has no legal target state today.
- `EscrowRecord.expires_at` is set to `created_at + 30 days` at creation and is only consulted by this function.
- The cross-contract direction matters: every existing call runs delivery → escrow, so adding escrow → delivery introduces a new dependency edge — weigh that against a delivery-side entry point.
- Issue #198 covers `validate_state_sync`'s missing `Holdback` case; this issue concerns a different unsynchronized pairing.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `reclaim_expired_escrow`
- `contracts/delivery_contract/lib.rs` — `validate_transition`, `cancel_delivery`, `validate_state_sync`
- `contracts/escrow_contract/test.rs` — `test_reclaim_expired_escrow_refunds_sender`

## Testing Requirements

- Integration test: reclaim an expired escrow, assert delivery and escrow are synchronized
- Unit test: reclaim from each reachable delivery status behaves per the agreed design
- Regression test: refund amount and `TotalLocked` decrement unchanged
- Regression test: reclaim still rejected before expiry and for non-`Locked` escrows
- Edge case: reclaim of an `InTransit` delivery, given the transition gap

## Definition of Done

- [ ] Delivery and escrow remain synchronized after reclaim
- [ ] Transition rules updated deliberately
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Shares the escrow → delivery wiring question with #294; the two should agree on direction.

## Labels

`bug`

---

# Issue #300 — `create_escrows_batch` has essentially no test coverage

## Problem Statement

`create_escrows_batch` is referenced only twice in
`contracts/escrow_contract/test.rs`, and neither reference tests its core
behavior — the existing coverage asserts that the function is rejected while the
protocol is paused.

Nothing verifies that a batch actually creates the escrows it claims to, that the
returned count is correct, that funds are transferred, that records are written
with the right fields, or that the secondary indexes are populated.

By contrast `create_deliveries_batch` is referenced seven times in the delivery
contract's suite.

## Why It Matters

The absence of coverage is why several defects in this backlog survived review.
Issues #188 (no `TotalLocked` update), #189 (no amount or token validation), #196
(divergent event payload), #272 (`fleet_id` hardcoded to `None`), and #273 (wrong
error type) are all in this one function, and every one of them would have been
caught by a test that simply created a batch and asserted the resulting state.

The batch path also has genuinely different mechanics from the single path — it
accumulates driver indexes in an in-memory `Map` and flushes them after the loop —
so single-escrow coverage does not transfer.

## Proposed Solution

Add a test module covering the function's core contract: escrows created, count
returned, tokens transferred, records correct, indexes populated, and batch-size
limit enforced.

Write the tests against current behavior and mark with a comment any assertion
that encodes a known defect, so the tests can be tightened as #188, #189, #272,
and #273 land rather than blocking on them.

## Acceptance Criteria

- [ ] A batch creates one escrow per element with correct sender, recipient, driver, token, amount, and status
- [ ] The returned count equals the number of escrows created
- [ ] Tokens are transferred from the sender for the full batch total
- [ ] All three secondary indexes are populated for every element
- [ ] A batch exceeding `MAX_BATCH_SIZE` is rejected
- [ ] A duplicate `delivery_id` within or across batches is rejected
- [ ] `expires_at` and `created_at` are set correctly

## Technical Notes

- `MAX_BATCH_SIZE` is 100 in `escrow_contract::constants`; a batch of exactly 100 must succeed and 101 must fail.
- The driver-index flush uses `soroban_sdk::Map<DataKey, Vec<u64>>`, so a batch containing the same driver twice is the edge case most likely to expose a bug.
- Issue #226 covers index accessor coverage more broadly; scope this issue to the batch creation path to avoid overlap.
- Existing single-escrow tests are the model for balance and record assertions.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `create_escrows_batch`
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: batch of 3 creates 3 correct escrow records
- Unit test: returned count matches
- Unit test: sender's balance decreases by the batch total
- Unit test: all three indexes contain every delivery ID
- Unit test: batch of `MAX_BATCH_SIZE` succeeds, `MAX_BATCH_SIZE + 1` rejected
- Unit test: duplicate delivery ID rejected with no partial state written
- Edge case: batch containing the same driver twice

## Definition of Done

- [ ] Core batch behavior covered by tests
- [ ] Known-defect assertions clearly marked
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

**None**. Landing this first makes #188, #189, #272, and #273 safer to implement.

## Labels

`test`

---

# Issue #301 — `shared_types::DeliveryId` conversions and comparisons have no dedicated tests

## Problem Statement

`DeliveryId` is a `u64` newtype with a hand-written API used across every contract:

```rust
pub struct DeliveryId(pub u64);
impl DeliveryId { pub fn new(value: u64) -> Self; pub fn value(self) -> u64; }
impl From<u64> for DeliveryId
impl From<DeliveryId> for u64
impl PartialEq<u64> for DeliveryId
impl PartialEq<DeliveryId> for u64
```

None of these six items has a dedicated test. The type is exercised incidentally
wherever contracts convert between representations, but nothing asserts that
`DeliveryId::from(n).value() == n`, that the two `PartialEq` directions agree, or
that round-tripping through `u64` is lossless.

## Why It Matters

`DeliveryId` is the correlation key between the delivery and escrow contracts.
`delivery_contract` uses the newtype in its public signatures while
`escrow_contract` takes a bare `u64`, so every cross-contract call converts —
`u64::from(delivery_id)` appears at each escrow invocation site.

The two `PartialEq` implementations are asymmetric hand-written code, and the
`value(self)` method takes `self` by value on a `Copy` type. These are small
surfaces, but they sit on the path that ties a delivery to the money escrowed
against it, and a defect would misroute that correlation silently.

The cost of covering them is very low, which is why this is Trivial — but
`shared_types` is the crate every contract depends on, so its primitives
warranting no tests at all is a gap worth closing.

## Proposed Solution

Add a small test module covering construction, both conversion directions, both
equality directions, and round-trip fidelity at boundary values.

Include `u64::MAX` and `0` explicitly, since the type wraps an unbounded counter
and the delivery counter's reset behavior is itself a concern (issue #291).

## Acceptance Criteria

- [ ] `DeliveryId::new(n).value()` returns `n`
- [ ] `DeliveryId::from(n)` and `u64::from(id)` round-trip losslessly
- [ ] Both `PartialEq` directions agree for equal and unequal values
- [ ] Boundary values `0` and `u64::MAX` behave correctly
- [ ] `delivery_key` and `escrow_key` produce distinct storage keys for the same ID
- [ ] Tests live with the other `shared_types` tests

## Technical Notes

- `shared_types` already has a `#[cfg(test)]` module, so there is an established home for these tests.
- `delivery_key(id)` and `escrow_key(id)` both take `impl Into<DeliveryId>` and produce different `StorageKey` variants — asserting they differ guards against a key-collision regression.
- `value(self)` consumes `self`; `DeliveryId` derives `Copy`, so this is ergonomic rather than a defect, but a test documents the intent.

## Relevant Files

- `contracts/shared_types/lib.rs` — `DeliveryId` and its impls, `delivery_key`, `escrow_key`
- `contracts/shared_types/lib.rs` — existing `#[cfg(test)]` module

## Testing Requirements

- Unit test: construction and `value()` round-trip
- Unit test: `From<u64>` and `From<DeliveryId>` round-trip
- Unit test: both `PartialEq` directions, equal and unequal
- Unit test: boundary values `0` and `u64::MAX`
- Unit test: `delivery_key` and `escrow_key` differ for the same ID

## Definition of Done

- [ ] Test module added and passing
- [ ] Boundary values covered
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`test`

---

# Issue #302 — `docs/protocol/delivery-protocol.md` states that `confirm_delivery` calls `release_escrow`

## Problem Statement

The protocol document describes delivery confirmation as releasing funds:

> Transitions status to `Delivered` and calls the escrow contract's
> `release_escrow` to release funds to the driver.

`delivery_contract::confirm_delivery` calls `mark_holdback_escrow`, not
`release_escrow`:

```rust
let _: () = env.invoke_contract(
    &escrow_address,
    &soroban_sdk::Symbol::new(&env, "mark_holdback_escrow"),
    soroban_sdk::vec![&env, recipient.into_val(&env), u64::from(delivery_id).into_val(&env)],
);
```

That call moves the escrow to `Holdback`. Funds reach the driver only on a
subsequent `release_holdback_escrow`, which the recipient or an admin must call
separately. The document does not mention `Holdback` at all.

## Why It Matters

This is the single most load-bearing fact about the payment flow, and the document
states the opposite of what happens. A reader concludes that confirming delivery
pays the driver, when confirmation only earmarks the funds — the driver is not
paid until a second, separate transaction.

The gap matters operationally: nothing obliges the recipient to make that second
call, which is the liveness problem issue #192 describes. A reader relying on this
document would not know the second step exists, let alone that it can be skipped.

The omission of `Holdback` also means the document's escrow model is a state
behind the implementation, which affects every reader reasoning about dispute
timing and fund availability.

## Proposed Solution

Correct the confirmation description to state that it calls
`mark_holdback_escrow` and transitions the escrow to `Holdback`, and document the
separate `release_holdback_escrow` step that actually pays the driver.

Add `Holdback` to the document's description of escrow states so the delivery and
escrow state machines it presents are consistent with
`docs/contract-design/escrow-design.md`, which was updated with the accurate
state machine.

## Acceptance Criteria

- [ ] The document states that `confirm_delivery` calls `mark_holdback_escrow`
- [ ] The `Holdback` state and its meaning are described
- [ ] The separate release step required to pay the driver is documented
- [ ] The document does not claim confirmation releases funds
- [ ] Descriptions are consistent with `docs/contract-design/escrow-design.md`
- [ ] Any other escrow interaction described in the file is verified against the source

## Technical Notes

- `docs/contract-design/escrow-design.md` carries an accurate state machine and refund-authorization table; reuse its terminology.
- `release_holdback_escrow` is callable by the recipient or an admin.
- Issues #303 and #304 cover further inaccuracies in this same file; coordinate so the three do not conflict.

## Relevant Files

- `docs/protocol/delivery-protocol.md`
- `contracts/delivery_contract/lib.rs` — `confirm_delivery`
- `contracts/escrow_contract/lib.rs` — `mark_holdback_escrow`, `release_holdback_escrow`
- `docs/contract-design/escrow-design.md`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Every escrow function named in the document verified to exist and be called as described
- [ ] State names verified against `shared_types::EscrowState`
- [ ] Cross-checked against the escrow design document for consistency

## Definition of Done

- [ ] Confirmation flow described accurately
- [ ] `Holdback` documented
- [ ] No contradiction with the escrow design document

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Same file as #303 and #304; sequence to avoid conflicting edits.

## Labels

`documentation`

---

# Issue #303 — `docs/protocol/delivery-protocol.md` documents the wrong parties for raising a dispute

## Problem Statement

The protocol document's transition table and dispute section both state that
disputes may be raised by the **sender or driver**:

> | `InTransit` | `Disputed` | Sender or driver raises a dispute |
> - `InTransit` → `Disputed` (sender or driver can raise)

`delivery_contract::raise_dispute` authorizes the **sender or recipient**, and
explicitly not the driver:

```rust
let is_sender = caller == delivery.sender;
let is_recipient = caller == delivery.recipient;
if !is_sender && !is_recipient {
    panic_with_error!(&env, FaniLabError::Unauthorized);
}
```

The document names one party who cannot call it and omits one who can.

## Why It Matters

This is an authorization claim, so being wrong in both directions is consequential.
A driver reading the protocol specification would expect to be able to raise a
dispute and finds their call rejected; a recipient would not know the capability
is available to them at all.

The confusion is compounded by genuine inconsistency in the code:
`dispute_resolution_contract::raise_dispute` *does* permit drivers, and
`escrow_contract::raise_dispute` permits all three parties — only the delivery
contract excludes the driver. Issue #292 covers that functional gap; this issue
covers the documentation being wrong about all of it.

Documentation that misstates who holds a permission is worse than silence, because
readers act on it.

## Proposed Solution

Correct the transition table and the dispute section to state the parties each
contract actually authorizes, noting where they differ.

If issue #292 lands first and widens the delivery contract to include drivers,
document the unified set instead. Either way the document must describe the
implementation rather than an intended design.

## Acceptance Criteria

- [ ] The transition table names the parties `delivery_contract::raise_dispute` actually authorizes
- [ ] The dispute section matches
- [ ] Any divergence between the delivery, dispute, and escrow contracts' authorization is noted
- [ ] No party is documented as authorized when the code rejects them
- [ ] The description remains accurate if #292 lands
- [ ] Other authorization claims in the file are verified against source

## Technical Notes

- The three authorization sets today: `delivery_contract` allows sender/recipient; `dispute_resolution_contract` allows sender/recipient/driver; `escrow_contract` allows sender/recipient/driver.
- `dispute_resolution_contract` cross-calls the delivery contract, so its broader set is not actually reachable — see issue #292.
- Closed issue #100 covered driver exclusion from disputes and prompted the dispute contract's widening; the delivery contract was not updated.

## Relevant Files

- `docs/protocol/delivery-protocol.md` — transition table and dispute section
- `contracts/delivery_contract/lib.rs` — `raise_dispute`
- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute`
- `contracts/escrow_contract/lib.rs` — `raise_dispute`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Each documented party checked against the contract's authorization branch
- [ ] Divergence between the three contracts verified and reflected
- [ ] Remaining authorization claims in the file audited

## Definition of Done

- [ ] Dispute authorization documented accurately
- [ ] Cross-contract divergence noted
- [ ] Other authorization claims verified

## Complexity

**Medium**

## Estimated Effort

1–2 hours

## Dependencies

Should be reconciled with #292 if that lands first; same file as #302 and #304.

## Labels

`documentation`

---

# Issue #304 — `docs/protocol/delivery-protocol.md` calls `Delivered` terminal while the state machine allows `Delivered → Disputed`

## Problem Statement

The protocol document states:

> `Delivered` and `Cancelled` are **terminal states** — no further transitions are
> permitted.

`validate_transition` explicitly permits a transition out of `Delivered`:

```rust
| (DeliveryStatus::Delivered, DeliveryStatus::Disputed)
| (DeliveryStatus::Disputed, DeliveryStatus::Delivered)
```

The document's own ASCII diagram reinforces the error by labelling `Delivered` as
`(terminal)`, and its transition table omits the `Delivered → Disputed` row while
listing `Disputed → Delivered`.

## Why It Matters

`Delivered → Disputed` is the post-delivery dispute window — the mechanism that
lets a recipient contest goods after accepting them, governed by the configurable
`dispute_time_limit`. Documenting `Delivered` as terminal states that this window
does not exist.

The document is internally inconsistent as well: it lists `Disputed → Delivered`
as valid, which means a delivery can leave `Delivered`, become `Disputed`, and
return — impossible if `Delivered` were genuinely terminal.

The practical effect is that readers do not learn the dispute window exists, and
the protocol has a real functional gap in that area (issues #193 and #292) which
nobody would think to look for while believing the state is terminal.

## Proposed Solution

Correct the terminal-state claim to cover only `Cancelled`, add the
`Delivered → Disputed` row to the transition table, and update the diagram so
`Delivered` is not labelled terminal.

Document the `dispute_time_limit` bound on that transition, since it is what makes
`Delivered` effectively terminal after the window elapses — that is likely the
intent behind the original wording and is worth stating precisely.

## Acceptance Criteria

- [ ] Only `Cancelled` is described as terminal
- [ ] `Delivered → Disputed` appears in the transition table
- [ ] The diagram no longer labels `Delivered` as terminal
- [ ] The `dispute_time_limit` bound on the transition is documented
- [ ] The document's transition set matches `validate_transition` exactly
- [ ] No transition is documented that the code rejects

## Technical Notes

- `validate_transition` in `contracts/delivery_contract/lib.rs` is the authoritative set — nine pairs in total; the document should match it item for item.
- The time bound is enforced in `dispute_resolution_contract::raise_dispute`'s `Delivered` branch, not in `validate_transition`, so the document should attribute it correctly.
- Issue #193 records that this transition is currently unreachable end to end because the escrow rejects `raise_dispute` from `Holdback`; note the state of affairs rather than documenting the transition as fully working.

## Relevant Files

- `docs/protocol/delivery-protocol.md` — state diagram, transition table, terminal-state claim
- `contracts/delivery_contract/lib.rs` — `validate_transition`
- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute` time-limit branch

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Every pair in `validate_transition` present in the document
- [ ] Every documented transition present in `validate_transition`
- [ ] Terminal-state claims verified against the transition set
- [ ] Time-limit attribution verified against the dispute contract

## Definition of Done

- [ ] Terminal-state claim corrected
- [ ] Transition table complete and matching the code
- [ ] Diagram updated

## Complexity

**Medium**

## Estimated Effort

1–2 hours

## Dependencies

Same file as #302 and #303; sequence to avoid conflicting edits.

## Labels

`documentation`

---

# Issue #305 — `docs/architecture/smart-contract-architecture.md` documents Proof-of-Delivery hashing that does not exist

## Problem Statement

The architecture document describes `delivery_contract`'s responsibilities as:

> **Responsibilities**: Creation of delivery, Assignment of drivers, In-Transit
> updates, and Proof of Delivery (PoD) hashing.

There is no proof-of-delivery mechanism in the contract. Searching
`contracts/delivery_contract/lib.rs` for `proof_of_delivery`, `pod_hash`, `PoD`,
and `delivery_proof` returns zero matches. `confirm_delivery` takes only the
recipient's address and the delivery ID, and stores no proof artifact.

The only hash-bearing feature in the protocol is
`dispute_resolution_contract::add_evidence_hash`, which stores `BytesN<32>`
evidence hashes against a dispute — a different contract and a different purpose.

## Why It Matters

Proof of delivery is a substantive trust primitive: it is what would let a driver
demonstrate they delivered, independent of the recipient's cooperation. Documenting
it as an existing responsibility misrepresents the protocol's trust model.

The absence matters concretely given the rest of this backlog. Confirmation is
entirely at the recipient's discretion, and nothing obliges them to confirm or to
release the holdback afterwards (issue #192). A reader who believes PoD hashing
exists would assume the driver has recourse; they do not.

The architecture document is also the highest-level entry point for new
contributors, so an incorrect responsibility list here propagates into every
mental model built from it.

## Proposed Solution

Remove the PoD claim from the responsibilities list, or mark it explicitly as
planned rather than implemented.

If proof of delivery is genuinely wanted — and the driver-recourse gap suggests it
would be valuable — file it as a separate feature issue with a concrete design
rather than leaving it implied by an architecture document. Do not expand this
issue into implementing it.

While in the file, verify the other per-contract responsibility lists against
their implementations; issue #306 covers one further claim in the same document.

## Acceptance Criteria

- [ ] The PoD claim is removed or clearly marked as unimplemented
- [ ] Every remaining responsibility listed for `delivery_contract` exists in the code
- [ ] If PoD is retained as a roadmap item, it is visually distinguished from shipped functionality
- [ ] A separate feature issue is filed if the capability is still wanted
- [ ] Other contracts' responsibility lists are verified in the same pass

## Technical Notes

- `confirm_delivery`'s signature is `(env, recipient, delivery_id)` — there is no parameter through which a proof artifact could be supplied.
- `DeliveryRecord` has no field for a proof hash; adding one would be a `shared_types` wire-format change.
- `dispute_resolution_contract::add_evidence_hash` is the closest existing mechanism and is dispute-scoped, not delivery-scoped.

## Relevant Files

- `docs/architecture/smart-contract-architecture.md` — `delivery_contract` section
- `contracts/delivery_contract/lib.rs` — `confirm_delivery`, `DeliveryRecord` usage
- `contracts/shared_types/lib.rs` — `DeliveryRecord`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Each listed responsibility traced to a function that implements it
- [ ] Absence of PoD confirmed by search across the contract
- [ ] Other contracts' responsibility lists spot-checked

## Definition of Done

- [ ] PoD claim removed or marked unimplemented
- [ ] Responsibility lists verified against code
- [ ] Follow-up feature issue filed if the capability is wanted

## Complexity

**Medium**

## Estimated Effort

1–2 hours

## Dependencies

Same file as #306; can be done together.

## Labels

`documentation`

---

# Issue #306 — Architecture document claims `delivery_contract` verifies driver tier, which it never does

## Problem Statement

The architecture document states that `delivery_contract`:

> **Interacts with**: `identity_reputation_contract` (to verify driver tier),
> `escrow_contract` (to trigger payment upon completion).

`delivery_contract` contains zero references to `get_driver_tier` or `DriverTier`.
Its only calls into `identity_reputation_contract` are `register_user` during
delivery creation and `increase_reputation` during confirmation.

`assign_driver` performs no reputation, tier, or KYC check of any kind — it
verifies only that the caller is the admin or the driver themselves, and that the
driver is not the sender or recipient.

## Why It Matters

Tier verification at assignment is the mechanism that would make the reputation
system consequential — without it, reputation is recorded but never gates
anything, and a driver at reputation zero is as assignable as one at the Gold
threshold.

Documenting the check as an existing interaction obscures that gap. A reader
evaluating the protocol's quality controls would conclude driver vetting happens
at assignment when nothing of the kind occurs.

Closed issue #44 reported that the tier system was never wired into
`assign_driver`; the architecture document still describes it as wired. This issue
is about the documentation's claim, not about implementing the check.

## Proposed Solution

Correct the interaction description to state what `delivery_contract` actually
calls: `register_user` on creation and `increase_reputation` on confirmation.

If tier gating is still wanted, reference the existing closed issue #44 discussion
or file a fresh feature issue — but do not implement it here. The scope of this
issue is making the document accurate.

Note the related gap that `kyc_verified` is likewise recorded but never read
anywhere (issue #313), so any claim about identity-based gating in this document
should be checked against that too.

## Acceptance Criteria

- [ ] The documented interaction matches the actual cross-contract calls
- [ ] No identity-based gating is claimed that the code does not perform
- [ ] The `escrow_contract` interaction description is verified in the same pass
- [ ] If tier gating remains desired, it is tracked as an explicit follow-up
- [ ] Other contracts' "Interacts with" lists are spot-checked

## Technical Notes

- `delivery_contract`'s cross-contract calls are: `register_user` (creation and batch creation), `mark_holdback_escrow` (confirmation), `refund_escrow` (cancellation), `raise_dispute` (dispute), `increase_reputation` (confirmation), and `get_escrow` (combined state).
- The escrow interaction is also loosely described — "to trigger payment upon completion" is inaccurate post-holdback, per issue #302.
- `get_driver_tier` and `is_eligible_for_enterprise` live in `identity_reputation_contract` and are called by nothing.

## Relevant Files

- `docs/architecture/smart-contract-architecture.md` — `delivery_contract` section
- `contracts/delivery_contract/lib.rs` — all `invoke_contract` sites
- `contracts/identity_reputation_contract/lib.rs` — `get_driver_tier`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Every claimed interaction traced to an `invoke_contract` call
- [ ] Every actual cross-contract call represented in the document
- [ ] Absence of tier checks confirmed by search
- [ ] Other contracts' interaction lists spot-checked

## Definition of Done

- [ ] Interaction description matches implementation
- [ ] Follow-up tracked if tier gating is still wanted
- [ ] Other sections spot-checked

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Same file as #305; can be done together.

## Labels

`documentation`

---

# Issue #307 — `PRODUCTION_READINESS.md` lists resolved issues as outstanding blockers

## Problem Statement

The readiness assessment lists blockers that have since been fixed:

```
- ❌ **Issue #7**: freeze_funds function lacks access control (unauthenticated)
- ❌ **Issue #8**: Dispute resolution path has structural issues
- ❌ **Reentrancy tests**: No reentrancy-specific test cases exist
- ⚠️ Access control incomplete on all privileged functions
```

All three claims are stale. `freeze_funds` now restricts callers to the configured
dispute contract and has `test_freeze_funds_unauthorized_caller_rejected` covering
it. A reentrancy test exists —
`test_release_escrow_rejects_reentrant_call_during_settlement_swap`, built on a
`MaliciousSettlementContract` mock. GitHub issues #7 and #8 are both closed.

The document simultaneously scores several categories as complete — "Code Quality
✅ (10/10)", "Testing ✅ (10/10)", "CI/CD ✅ (10/10)" — while listing ❌ blockers,
so it contradicts itself as well as the codebase.

## Why It Matters

This is the document a reviewer or operator consults to judge deployment
readiness. Listing fixed vulnerabilities as open understates the project's actual
state and wastes reviewer time re-investigating closed work.

The self-contradiction is the more corrosive problem: a document that scores
testing 10/10 while asserting no reentrancy tests exist cannot be trusted in
either direction, so its genuine warnings — and there are genuine ones in this
backlog — carry no weight.

Closed issue #34 previously reported that this document's claims contradicted the
codebase. It has drifted again, now in the opposite direction: rather than
overstating readiness, it understates it by citing resolved issues.

## Proposed Solution

Reconcile the document against the current codebase and the GitHub issue tracker:
remove or update claims about issues that are closed, verify each remaining ❌ and
⚠️ item against the source, and resolve the contradiction between the category
scores and the blocker list.

Establish where the document's authority comes from — if the blocker list is meant
to mirror open GitHub issues, say so and reference them by link so drift is
visible. A scoring section that is maintained by hand and never revisited will
drift again.

## Acceptance Criteria

- [ ] No closed issue is listed as an outstanding blocker
- [ ] Each remaining blocker is verified to still exist in the code
- [ ] Category scores are consistent with the blocker list
- [ ] Claims such as "Test coverage > 80%" are verified against actual coverage
- [ ] The document states how and when it should be updated
- [ ] Remaining genuine gaps are linked to their tracking issues

## Technical Notes

- `freeze_funds` gained its caller restriction and test; `escrow_contract/test.rs` contains both `test_freeze_funds_unauthorized_caller_rejected` and `test_freeze_funds_remains_available_while_paused`.
- The reentrancy test is `test_release_escrow_rejects_reentrant_call_during_settlement_swap`; issue #238 in this backlog notes that coverage is limited to one call site, which is a genuine remaining gap worth citing accurately.
- Coverage is enforced by `codecov.yml` at an 80% project target, so the coverage claim is checkable rather than aspirational.
- Several open GitHub issues (#64, #140–#143) are genuine blockers and could anchor the list.

## Relevant Files

- `PRODUCTION_READINESS.md`
- `contracts/escrow_contract/lib.rs` — `freeze_funds`
- `contracts/escrow_contract/test.rs` — the freeze and reentrancy tests
- `codecov.yml`

## Testing Requirements

Documentation change; verification by review against source and tracker:

- [ ] Each ❌ and ⚠️ item traced to current code or removed
- [ ] Each cited GitHub issue's state checked
- [ ] Coverage claim checked against the enforced threshold
- [ ] Category scores reconciled with the surviving blocker list

## Definition of Done

- [ ] Stale blockers removed or corrected
- [ ] Internal contradictions resolved
- [ ] Maintenance expectation documented

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`documentation`

---

# Issue #308 — `MIGRATION_GUIDE.md` and `UPGRADE_GUIDE.md` cover the same topic with no cross-reference

## Problem Statement

The repository maintains two documents on contract versioning:

- `docs/UPGRADE_GUIDE.md` (175 lines) — "Guide for upgrading FaniLab smart contracts on Stellar Soroban."
- `docs/MIGRATION_GUIDE.md` (186 lines) — "This guide demonstrates how to safely migrate contract state when upgrading to new contract versions."

Neither links to the other. A reader arriving at either has no indication the
other exists, and the two overlap substantially — upgrading a Soroban contract and
migrating its state are steps in one procedure, not separable topics.

`UPGRADE_GUIDE.md` documents a `migrate_to_v2` pattern that no contract implements
(the subject of closed issue #81), which is itself state-migration content sitting
in the upgrade document.

## Why It Matters

Contract upgrade is a high-stakes, infrequently-performed operation. Splitting its
documentation across two unlinked files means an operator following one may never
see prerequisites or warnings recorded in the other — and the consequences of a
partial upgrade procedure on a contract holding escrowed funds are severe.

The duplication also guarantees divergence: two documents describing overlapping
procedures, maintained independently, will disagree over time. They already do, in
that one documents a migration pattern the other does not mention.

The cost of fixing this is low, which is why it is Trivial — but the risk it
mitigates is not.

## Proposed Solution

Decide whether the two documents should merge or remain separate with clear scopes.

If they remain separate, give each an explicit scope statement and a prominent
cross-reference, and move the `migrate_to_v2` state-migration content into the
migration guide where it belongs.

If they merge, preserve all content and leave a stub or redirect at the removed
path so existing links do not break.

## Acceptance Criteria

- [ ] The relationship between the two documents is explicit
- [ ] Each document states its scope and links to the other, or they are merged
- [ ] State-migration content lives in one place
- [ ] No procedure step exists in only one document without a pointer from the other
- [ ] Existing links to either path continue to resolve
- [ ] The unimplemented `migrate_to_v2` pattern is marked as such

## Technical Notes

- No contract implements `migrate_to_v2` or calls `update_current_contract_wasm`; closed issue #81 covers the missing tooling, so the documentation should be explicit that the pattern is aspirational.
- `docs/API.md` and `README.md` may link to one or both guides — check before renaming or removing a path.
- This issue is documentation organization only; implementing migration tooling is out of scope.

## Relevant Files

- `docs/MIGRATION_GUIDE.md`
- `docs/UPGRADE_GUIDE.md`
- `README.md`, `docs/API.md` — for existing links

## Testing Requirements

Documentation change; verification by review:

- [ ] Both documents' scopes stated and non-overlapping, or merged
- [ ] Cross-references present and resolving
- [ ] All inbound links from other documents still resolve
- [ ] No procedural step orphaned in one document

## Definition of Done

- [ ] Relationship clarified or documents merged
- [ ] Migration content consolidated
- [ ] Inbound links verified

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`documentation`

---

# Issue #309 — Issue templates do not prompt for a complexity classification

## Problem Statement

The repository has three issue templates —
`.github/ISSUE_TEMPLATE/bug_report.md`, `feature_request.md`, and
`security_vulnerability.md`. None of them prompts for complexity or effort:
grepping all three for `complexity`, `effort`, and `points` returns zero
matches.

Maintainers triaging incoming issues need an explicit complexity classification
in order to size and prioritise them — the backlog issues each carry a
`## Complexity` section of Trivial, Medium, or High, and externally filed issues
arrive with no equivalent.

## Why It Matters

Contributors filing issues through the templates produce entries with no size
signal at all. A maintainer then has to assess and classify each one by hand
before it can be prioritised, or the issue sits untriaged.

The templates are the natural place to capture this at source. They already prompt
for reproduction steps, expected behavior, and environment — adding two more
fields is a small change that removes recurring manual work.

The backlog's own structure is the ready-made answer: `## Complexity`,
`## Estimated Effort`, and `## Acceptance Criteria` are exactly the fields
maintainers need and are already the established convention in this repository.

## Proposed Solution

Add a complexity field to the templates, offering the three levels used
throughout the backlog, along with an estimated-effort prompt.

Consider aligning the templates more broadly with the backlog's issue structure so
externally filed issues and internally authored ones are shaped alike — but keep
the change proportionate, since templates that demand too much deter contributors.

Do not add automatic label defaults to the template front matter. Labels that
enrol an issue in an external programme must stay a deliberate, per-issue
decision by a maintainer rather than something the templates apply silently.

## Acceptance Criteria

- [ ] Templates prompt for a complexity classification with the three levels
- [ ] Templates prompt for estimated effort
- [ ] The prompts explain what the levels mean
- [ ] Existing template fields are preserved
- [ ] Templates add no automatic label defaults via front matter
- [ ] Templates remain short enough not to deter contributors

## Technical Notes

- The `security_vulnerability.md` template may warrant different handling — severity matters more than complexity there, and it should continue to direct reporters to the disclosure process in `SECURITY.md`.
- The backlog's `## Complexity` / `## Estimated Effort` sections are the wording to reuse.

## Relevant Files

- `.github/ISSUE_TEMPLATE/bug_report.md`
- `.github/ISSUE_TEMPLATE/feature_request.md`
- `.github/ISSUE_TEMPLATE/security_vulnerability.md`
- `SECURITY.md` — disclosure process referenced by the security template

## Testing Requirements

Configuration change; verification by use:

- [ ] Each template renders correctly when filing a new issue
- [ ] The complexity prompt is present and understandable
- [ ] The security template still routes reporters to the disclosure process

## Definition of Done

- [ ] Complexity and effort prompts added
- [ ] No automatic label defaults introduced
- [ ] Templates render correctly

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`enhancement`, `documentation`

---

# Issue #310 — SDK `tsconfig.json` excludes test files from type-checking

## Problem Statement

`sdk/typescript/tsconfig.json` excludes test files from compilation:

```json
"exclude": ["node_modules", "dist", "**/*.test.ts"]
```

`npm run build` runs `tsc`, so any file matching `*.test.ts` is never
type-checked. The package declares a jest toolchain and a `"test": "jest"` script
but currently contains no test files at all and no jest configuration (issue
#249).

## Why It Matters

The exclusion means that when SDK tests are eventually written — which issues
#222, #249, and #263 all call for — they will compile only under `ts-jest` at test
time, never under the project's own build. A type error in a test would not surface
in `npm run build`, and if CI runs only the build (issue #248), it would not
surface in CI either.

Excluding tests from the *emitted output* is correct and desirable; excluding them
from *type-checking* is not. The two are usually separated by keeping the exclusion
narrow and running a `tsc --noEmit` pass that includes tests.

This is small and currently latent — there are no tests to check — which is why it
is Trivial. Fixing it before tests exist means the first test written is covered
from the start.

## Proposed Solution

Keep test files out of the build output but include them in type-checking, either
by adding a separate type-check script that does not apply the exclusion, or by
splitting into a base `tsconfig.json` and a `tsconfig.build.json` that adds the
exclusion for emit only.

Wire whichever type-check command results into the CI job proposed by issue #248,
so test type errors fail the build.

## Acceptance Criteria

- [ ] Test files are type-checked
- [ ] Test files are not emitted into `dist/`
- [ ] `npm run build` still produces a correct build output
- [ ] A type error in a test file causes a non-zero exit from the type-check command
- [ ] The type-check command is suitable for CI
- [ ] `examples/basic-usage.ts` is also type-checked

## Technical Notes

- A common shape is a base `tsconfig.json` with no test exclusion plus a `tsconfig.build.json` that extends it and adds `exclude`; `npm run build` points at the latter.
- `"strict": true` is already enabled, so type-checking tests will be meaningfully strict.
- `examples/` is not currently excluded, so the example file should already be type-checked — verify rather than assume.
- Coordinate with issue #249, which adds the jest configuration, and #248, which adds the CI job.

## Relevant Files

- `sdk/typescript/tsconfig.json`
- `sdk/typescript/package.json` — `build` script
- `sdk/typescript/examples/basic-usage.ts`

## Testing Requirements

- Verification: a deliberate type error in a `.test.ts` file fails the type-check
- Verification: `dist/` contains no compiled test files after a build
- Verification: `npm run build` succeeds on the current source
- Verification: the example file is included in type-checking

## Definition of Done

- [ ] Tests type-checked but not emitted
- [ ] Type-check command verified to fail on a test type error
- [ ] Build output unchanged

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Most useful alongside #249 (jest config) and #248 (CI job); independently landable.

## Labels

`test`, `enhancement`

---

# Issue #311 — No contract exposes a non-panicking existence check for escrows or disputes

## Problem Statement

Both record-fetching accessors fail hard when a record is absent, and neither
contract offers a way to ask whether one exists.

`escrow_contract::get_escrow` panics with `EscrowError::DeliveryNotFound`, and
`dispute_resolution_contract::get_dispute` panics with
`FaniLabError::DeliveryNotFound`:

```rust
pub fn get_dispute(env: Env, delivery_id: DeliveryId) -> DisputeCase {
    env.storage().persistent().get(&dispute_key)
        .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::DeliveryNotFound))
}
```

`identity_reputation_contract` already solves this pattern with
`has_driver_profile(driver) -> bool`, added so callers could check existence
before acting rather than triggering a panic.

## Why It Matters

The absence forces callers into awkward shapes. `delivery_contract::cancel_delivery`
cannot check whether an escrow exists before cross-calling `refund_escrow`, which
is exactly why a delivery with no escrow becomes uncancellable (issue #295). Issue
#297 hits the same wall for `mark_in_transit`.

Off-chain clients are similarly affected: an indexer or front end that wants to
display "escrow: not yet funded" must call `get_escrow` and catch a contract
panic, which is both awkward and indistinguishable from other failure modes at the
transaction level.

The `has_driver_profile` precedent shows the team already recognises this problem;
the accessors were simply never added for the other two record types.

## Proposed Solution

Add `has_escrow(delivery_id) -> bool` to `escrow_contract` and
`has_dispute(delivery_id) -> bool` to `dispute_resolution_contract`, following
`has_driver_profile`'s shape — a simple storage presence check with no
authorization requirement and no panic.

Keep the existing panicking accessors unchanged so no caller breaks.

## Acceptance Criteria

- [ ] `escrow_contract::has_escrow` returns true for an existing escrow and false otherwise
- [ ] `dispute_resolution_contract::has_dispute` behaves equivalently
- [ ] Neither panics for an unknown ID
- [ ] Neither requires authorization
- [ ] `get_escrow` and `get_dispute` behavior is unchanged
- [ ] Both are documented in `docs/API.md`

## Technical Notes

- `identity_reputation_contract::has_driver_profile` uses `.get::<_, DriverProfile>(&key).is_some()`; a plain `.has(&key)` is cheaper and sufficient for a presence check.
- `escrow_contract::get_escrow` already performs a `has` check internally before loading (see issue #289), so the storage call pattern is established.
- These accessors unblock the cleaner fixes proposed in #295 and #297; adding them first makes those changes simpler.
- Keep this scoped to the two accessors — it is deliberately a single issue rather than one per contract, since they are the same one-line addition and would be reviewed together.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `get_escrow`, new `has_escrow`
- `contracts/dispute_resolution_contract/lib.rs` — `get_dispute`, new `has_dispute`
- `contracts/identity_reputation_contract/lib.rs` — `has_driver_profile` precedent
- `docs/API.md`

## Testing Requirements

- Unit test: `has_escrow` true after creation, false for an unknown ID
- Unit test: `has_dispute` true after a dispute is raised, false otherwise
- Unit test: neither panics for an unknown ID
- Unit test: both callable without authorization
- Regression test: `get_escrow` and `get_dispute` still panic as before for unknown IDs

## Definition of Done

- [ ] Both accessors added
- [ ] Documented in `docs/API.md`
- [ ] Tests added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**. Landing this first simplifies #295 and #297.

## Labels

`enhancement`

---

# Issue #312 — Disputes cannot be enumerated, so no operator view of open cases exists

## Problem Statement

`dispute_resolution_contract` stores each case under
`DataKey::Dispute(delivery_id)` and exposes exactly one accessor,
`get_dispute(delivery_id)`, which requires the caller to already know the delivery
ID.

There is no index of disputes by status, by party, or by age, and no counter. An
admin cannot ask which disputes are currently `Open`, which are approaching their
resolution deadline, or how many exist.

Both `escrow_contract` and `delivery_contract` maintain secondary indexes for
exactly this purpose — `EscrowsBySender`, `EscrowsByDriver`,
`DeliveriesByRecipient`, and so on — so the pattern is established in the codebase
and simply absent here.

## Why It Matters

Dispute resolution is admin-driven: `resolve_dispute_refund_sender`,
`resolve_dispute_pay_driver`, and `resolve_dispute_split_funds` all require an
admin to act. An admin who cannot enumerate open disputes has no way to discover
the work queue on chain — they must reconstruct it from `dispute_raised` events
off chain, and any gap in that indexing silently drops a case.

The consequence is directly connected to the forced-resolution mechanism.
`force_resolve_dispute` exists because admins may fail to resolve disputes in time
(and is itself defective — issues #205 and #206). Making the queue invisible makes
that failure more likely, not less.

`docs/MONITORING.md` names dispute activity among the metrics to track, with no
on-chain accessor to support it.

## Proposed Solution

Add a secondary index of disputes, following the pattern already used by the
escrow and delivery contracts: maintain a list on `raise_dispute` and expose an
accessor.

Indexing all disputes with their status is the simplest correct approach — callers
filter client-side. Maintaining a separate `Open` list has better read
characteristics but requires careful removal on every resolution path, of which
there are four, and a missed removal produces a misleading queue.

Note the growth concern that issue #234 raises for the existing indexes: a single
unbounded vector has the same scaling ceiling here, so the design should account
for it rather than repeat it.

## Acceptance Criteria

- [ ] Disputes can be enumerated without prior knowledge of delivery IDs
- [ ] Open disputes are distinguishable from resolved ones
- [ ] The index is updated on `raise_dispute`
- [ ] The index remains correct across all four resolution paths
- [ ] Growth characteristics are bounded or explicitly documented
- [ ] Existing `get_dispute` behavior is unchanged

## Technical Notes

- The four terminal paths are `resolve_dispute_refund_sender`, `resolve_dispute_pay_driver`, `resolve_dispute_split_funds`, and `force_resolve_dispute` — any index requiring removal must be updated in all four.
- `DisputeCase` already carries `status`, `resolved_at`, and `resolved_by`, so a status-bearing index needs no struct change.
- `escrow_contract::get_escrows_by_sender` and its siblings are the established accessor shape.
- Issue #234 proposes pagination for the existing indexes; a new index should not repeat the unbounded-vector pattern.

## Relevant Files

- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute`, `get_dispute`, the four resolution paths, `DataKey`
- `contracts/escrow_contract/lib.rs` — secondary index pattern
- `docs/MONITORING.md`, `docs/API.md`

## Testing Requirements

- Unit test: a raised dispute appears in the enumeration
- Unit test: enumeration is empty before any dispute is raised
- Unit test: a resolved dispute is distinguishable from an open one, via each of the four resolution paths
- Unit test: multiple disputes enumerate correctly
- Edge case: enumeration behavior at a substantial dispute count
- Regression test: `get_dispute` unchanged

## Definition of Done

- [ ] Dispute enumeration implemented
- [ ] Index correct across all resolution paths
- [ ] Growth characteristics addressed
- [ ] Documented in `docs/API.md`
- [ ] Tests added and passing

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Should adopt whatever indexing approach #234 settles on to avoid repeating the unbounded-vector pattern.

## Labels

`feature`, `enhancement`

---

# Issue #313 — `kyc_verified` is recorded and administered but never read by any contract

## Problem Statement

`DriverProfile` carries a `kyc_verified` flag. `identity_reputation_contract`
provides an admin function to set it:

```rust
pub fn update_driver_kyc_status(env: Env, admin: Address, driver: Address, kyc_verified: bool) {
    /* admin-gated */
    profile.kyc_verified = kyc_verified;
    /* store, emit KycStatusUpdatedEvent */
}
```

The flag is never read as a condition anywhere in the protocol. Searching all six
contracts finds `kyc_verified` only at write sites: `false` at registration,
assignment in `update_driver_kyc_status`, and the event payload. The single
occurrence in `delivery_contract` is the fabricated default inside
`get_driver_profile` (issue #202).

`fleet_management_contract` and `escrow_contract` contain zero references.

## Why It Matters

KYC verification exists to gate participation — that is its only purpose. A flag
that is set by an admin, emitted in an event, and consulted by nothing provides no
protection: an unverified driver can be assigned to any delivery, join any fleet,
and receive any payout exactly as a verified one can.

The administrative surface makes this actively misleading. An operator who
verifies a driver's identity and sets the flag reasonably believes they have
changed something about what that driver may do. They have not — they have written
a value that no code path reads.

This is the same shape as the multi-signature finding in issue #216: a control
that is configured, exposed through an API, and emits events, but is never
enforced. It sits alongside the unenforced driver-tier system (closed issue #44),
so the identity contract currently has two recorded-but-unenforced gating
mechanisms.

## Proposed Solution

Decide where KYC should gate, then enforce it at that point. The natural candidate
is `delivery_contract::assign_driver`, which today checks only that the caller is
the admin or the driver and that the driver is not the sender or recipient.

Enforcement requires a cross-call to `identity_reputation_contract`, which
`delivery_contract` already performs for `register_user` and
`increase_reputation`, so the wiring exists.

Make the gate configurable rather than absolute — a `require_kyc` protocol flag
lets deployments opt in without breaking existing test fixtures and testnet
flows, and makes the policy explicit rather than implicit.

If the decision is that KYC should not gate anything on chain, remove the flag and
its admin function rather than leaving a control that implies protection it does
not provide.

## Acceptance Criteria

- [ ] `kyc_verified` either gates a concrete action or is removed
- [ ] If it gates, the enforcement point is documented
- [ ] If enforcement is configurable, the default is stated explicitly
- [ ] An unverified driver is rejected at the gate when enforcement is enabled
- [ ] A verified driver proceeds normally
- [ ] Existing flows are unaffected when enforcement is disabled
- [ ] `docs/GOVERNANCE.md` or `docs/API.md` describes the policy

## Technical Notes

- `assign_driver` currently performs no cross-contract call; adding one puts the identity contract on the assignment path and introduces a failure mode if it is unreachable — weigh that and consider making the gate skip cleanly when no identity contract is configured, as the reputation calls already do.
- `get_driver_profile` in `identity_reputation_contract` panics with `ProviderNotFound` for an unregistered driver, so the gate must handle drivers with no profile.
- Closed issue #44 covers the parallel unenforced tier system; a single gating mechanism could reasonably address both, but that is a larger design decision than this issue should settle.
- `KycStatusUpdatedEvent` already exists and should continue to be emitted.

## Relevant Files

- `contracts/identity_reputation_contract/lib.rs` — `update_driver_kyc_status`, `register_driver`, `get_driver_profile`
- `contracts/delivery_contract/lib.rs` — `assign_driver`
- `contracts/shared_types/lib.rs` — `DriverProfile.kyc_verified`
- `docs/GOVERNANCE.md`

## Testing Requirements

- Unit test: unverified driver rejected at the gate when enforcement is enabled
- Unit test: verified driver accepted
- Unit test: behavior when no identity contract is configured
- Unit test: behavior for a driver with no profile at all
- Regression test: existing assignment flows unaffected with enforcement disabled
- Authorization test: only an admin can change KYC status
- Event test: `KycStatusUpdatedEvent` still emitted

## Definition of Done

- [ ] KYC either enforced at a documented point or removed
- [ ] Policy and default documented
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Related to the closed issue #44's unenforced tier system; the two could share a gating mechanism but this issue does not depend on that.

## Labels

`security`, `feature`
