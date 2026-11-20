# What `scripts/check_claims.sh` checks — and what it does NOT prove

This document explains `scripts/check_claims.sh`, which validates
`docs/claims-audit.md` against the test suite in
`contracts/upgrade-manager/src/test.rs`.

## Purpose

`docs/claims-audit.md` maps every behavioral claim in the project's
documentation (README, walkthrough, CONTRIBUTING, and source doc comments) to
the function that implements it and the test that verifies it. Each row carries
a status: `VERIFIED`, `REWORDED`, or `REMOVED`.

`scripts/check_claims.sh` is a sanity check that keeps that audit honest. It
verifies the **structure** and **internal consistency** of the audit table — it
does **not** validate that the documented behavior is actually correct.

---

## What the script checks

The script reads `docs/claims-audit.md` and
`contracts/upgrade-manager/src/test.rs`, then walks each table row (lines
starting with `|`, skipping header/separator rows). For each 6-field claims
table row (`# | claim | source | function | test_name | status`), it performs
three checks:

### 1. Every `VERIFIED` row has a test name in backticks

For any row whose status is `VERIFIED`, the `test_name` column must be
non-empty and not `N/A`. The script extracts test names from backtick-quoted
strings (e.g. `` `test_create_proposal` ``) using `grep -oP '`\\Ktest_\\w+(?=`)'`.

If a `VERIFIED` row has no test function name in backticks, the check fails.

### 2. Every named test exists as `fn <name>` in `test.rs`

Each test name extracted from backticks is checked against
`contracts/upgrade-manager/src/test.rs`. The script uses a whole-word match:

```bash
grep -qE "fn ${tname}\b" "$TEST_FILE"
```

If a referenced test function does not exist in `test.rs`, the check fails.

### 3. Every status is `VERIFIED`, `REWORDED`, or `REMOVED`

The `status` field of each row must be one of `VERIFIED`, `REWORDED`, or
`REMOVED`. Any other value causes the check to fail.

If all three checks pass for every row, the script prints `OK` and exits `0`.
Otherwise it prints `FAILED: N error(s) found in claims audit.` and exits `1`.

---

## What the script does NOT prove

This is important: `scripts/check_claims.sh` validates the **audit document's
structure**, not the **correctness of the claims**. Specifically, it does NOT:

### It does not run the tests

The script never executes `cargo test`. It only checks that a function named
`fn <name>` exists in `test.rs`. A test can exist, compile, and be referenced
by the audit while still being wrong, skipped, or failing. Passing
`check_claims.sh` is not a substitute for `cargo test --workspace`.

### It does not verify that the test assertions match the claim

The script checks only that a test function with the right name exists. It does
not read the test body, does not check what the test asserts, and does not
confirm that the test actually verifies the claim listed in the `claim` column.
A test named `test_create_proposal` could assert something unrelated to
proposal creation and the script would still pass.

### It does not check for missing claims

The script only validates rows that already exist in `docs/claims-audit.md`.
It does not detect claims that are missing from the audit entirely. If a
behavior is documented in the README or walkthrough but has no corresponding
audit row, the script will not report it.

### Summary table

| Question | Answered by check_claims.sh? |
|----------|------------------------------|
| Does every VERIFIED row name a real test? | Yes |
| Is every audit status valid? | Yes |
| Do the tests pass? | **No** (run `cargo test`) |
| Does each test assert what its claim says? | **No** (manual review needed) |
| Are there undocumented/missing claims? | **No** (manual review needed) |

---

## Exit codes

| Exit code | Meaning |
|-----------|---------|
| `0` | All checks passed — the claims audit is structurally consistent. |
| `1` | One or more checks failed — see the `ERROR:` lines printed by the script. |

Usage (from repo root):

```bash
bash scripts/check_claims.sh
```

The script is also run in CI (see `.github/workflows/ci.yml`, "Verify claims
audit" step).

---

## Relationship to `cargo test`

`check_claims.sh` is a **documentation hygiene** check, not a test runner.
The correct sequence for validating claims is:

1. `bash scripts/check_claims.sh` — audit table is structurally consistent.
2. `cargo test --workspace` — the referenced tests actually pass.
3. Manual review — each test's assertions genuinely match the claim it is
   supposed to verify.

A clean `check_claims.sh` run plus a green `cargo test` run together give
confidence that the audit is complete, consistent, and exercised — but only
manual review confirms that each test asserts what its claim says.