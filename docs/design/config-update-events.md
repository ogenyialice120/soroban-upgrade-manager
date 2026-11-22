# Design: Events for Config Updates with New Values (Issue #124)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #124
**Branch:** `design/config-update-events`
**Type:** Manager contract event-payload change
**Date:** 2026-11-22

---

## 1. Problem

`emit_config_updated` (in `governance.rs`) publishes the `config_updated` event
with topics `(symbol_short!("config"), symbol_short!("updated"))` and data
`caller.clone()` — the *caller's address only*. It does not include the new
`voting_period`, `timelock_delay`, `quorum`, or `approval_threshold_pct`. As a
result, config changes cannot be audited from event logs alone: a consumer
must query storage (or keep its own snapshot) to learn what actually changed.

Including the new config — and the old config, for diff auditing — in the event
data makes each `config_updated` event self-contained for auditing.

## 2. Existing guarantee touched

> **`config_updated` event data payload.** The data is currently
> `(caller: Address)`.

This design changes the data payload to `(caller, old_config, new_config)`.
The event **topics are unchanged**, so topic-based subscriptions/filters keep
working. The on-chain `update_config` function signature is unchanged, so
on-chain callers are unaffected. The breakage is limited to **off-chain event
consumers** (indexers/dashboards) that parse the data payload positionally —
those need to read two `Config` values instead of one `Address`. This should
be flagged as a **breaking change for event-data consumers**.

## 3. Design

### 3.1 Event data payload

Keep topics `(symbol_short!("config"), symbol_short!("updated"))` (unchanged).

Change the data from `caller` to a positional tuple:

```rust
data = (caller: Address, old_config: Config, new_config: Config)
```

`Config` is a `#[contracttype]` and serializes to a `Val`, so a tuple of
`(Address, Config, Config)` is valid Soroban event data.

### 3.2 Include the old config too (diff auditing)

**Yes — include both old and new.** The event's stated purpose is auditability
("audit config changes from event logs without querying storage"). With only
`new_config`, a consumer still cannot tell *what changed* without a prior event
or a storage snapshot. Storage history is not queryable on-chain, so the event
is the only place to capture the pre-image. Carrying `old_config` and
`new_config` makes the diff computable from the event alone.

Cost: `Config` is four `u32` fields; two configs plus one `Address` is small, so
there is no meaningful gas concern.

### 3.3 Implementation change

`emit_config_updated` gains two parameters and `update_config` loads the
pre-image before overwriting:

```rust
// governance.rs
pub fn emit_config_updated(env: &Env, caller: &Address, old: &Config, new: &Config) {
    env.events().publish(
        (symbol_short!("config"), symbol_short!("updated")),
        (caller.clone(), old.clone(), new.clone()),
    );
}

// lib.rs update_config
let old_config = load_config(&env)?;          // capture pre-image BEFORE save
let config = Config { voting_period, timelock_delay, quorum, approval_threshold_pct };
validate_config(&config)?;
save_config(&env, &config);
emit_config_updated(&env, &caller, &old_config, &config);
```

`load_config` already returns `Result<Config, Error>` and `update_config`
already requires the admin (which requires initialization), so `old_config` is
always present.

### 3.4 Tuple vs. `Map` for the data

- **Positional tuple** `(caller, old, new)` — matches every other event in the
  contract (`proposal_created` → `(id, proposer, target, wasm_hash)`,
  `vote_cast` → `(proposal_id, voter, approve, weight)`, etc.); cheaper;
  consumers read by position.
- **`Map<Symbol, Val>`** `{caller, old, new}` — self-describing; consumers do
  not rely on field order.

**Recommendation: positional tuple**, for consistency with the existing event
style. (If the project later adopts self-describing event data broadly, a
`Map` could be revisited across all events at once.)

### 3.5 Initial config at `initialize`

`initialize` sets the initial config but emits the `initialized` event (data =
admin), not `config_updated`. The initial config is therefore not captured in
any event today. This design scopes itself to the `config_updated` event only;
capturing the initial config (e.g. extending the `initialized` event data, or
emitting a `config_updated` with `old = default/empty` and `new = initial`) is
a separate follow-up — see §5.

## 4. Tests needed

The only existing test that references the `config_updated` event is
`test_event_config_updated` (`src/test.rs`), which currently asserts only that
at least one `(config, updated)` event was emitted. It is the only test that
needs updating.

- **Update `test_event_config_updated`**: assert the emitted event's data
  equals `(caller, old_config, new_config)` — `old_config` matches the config
  before the call, `new_config` matches the values passed to `update_config`
  (`voting_period=50`, `timelock_delay=1000`, `quorum=1`,
  `approval_threshold_pct=51`), and the changed fields differ between old and
  new.
- **New `test_event_config_updated_payload_round_trip`**: `old_config` equals
  `get_config()` before the call and `new_config` equals `get_config()` after.
- **New `test_event_config_updated_includes_all_fields`**: change several
  fields at once and assert each appears in `new_config` and differs from
  `old_config`.
- **New `test_event_config_updated_only_on_update_config`**: `config_updated`
  is emitted by `update_config` and not by `transfer_admin` (which emits
  `admin_transferred`) or any other function.
- **Unaffected**: the other event tests (`initialized`, `proposal_created`,
  `vote_cast`, `finalized`, `cancelled`, `admin_transferred`) do not parse
  `config_updated` data and need no changes.

## 5. Out of scope

- Changing the `config_updated` **topics** (kept stable so filters keep working).
- Capturing the initial config at `initialize` (follow-up).
- Changing any other event's payload (only `config_updated`).
- Code: no contract code is written until this design is approved by a
  maintainer.
