#!/usr/bin/env bash
# scripts/check_claims.sh
#
# Validates docs/claims-audit.md against the test suite.
#
# Checks:
# 1. Every VERIFIED row has a test name in backticks.
# 2. Every named test exists as `fn <name>` in test.rs (whole-word match).
# 3. Every status is VERIFIED, REWORDED, or REMOVED.
#
# Usage:
#   bash scripts/check_claims.sh
#
# Exit codes:
#   0 — all checks passed
#   1 — one or more checks failed

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLAIMS_FILE="$REPO_ROOT/docs/claims-audit.md"
TEST_FILE="$REPO_ROOT/contracts/upgrade-manager/src/test.rs"

if [[ ! -f "$CLAIMS_FILE" ]]; then
    echo "ERROR: claims audit file not found: $CLAIMS_FILE"
    exit 1
fi

if [[ ! -f "$TEST_FILE" ]]; then
    echo "ERROR: test file not found: $TEST_FILE"
    exit 1
fi

ERRORS=0

# ---------------------------------------------------------------------------
# 1. Check that every VERIFIED row has a test name (non-empty backtick field)
# 2. Check that every status is valid
# 3. Check that every named test exists in test.rs
# ---------------------------------------------------------------------------

# Extract table rows (lines starting with |). Skip header and separator rows.
# Columns: | # | claim | source | function | test_name | status |
# We expect at least 6 pipe-separated fields per data row.

while IFS= read -r line; do
    # Skip header separator rows (contain only dashes and pipes)
    if [[ "$line" =~ ^\|[[:space:]]*--- ]] || [[ "$line" =~ ^\|[[:space:]]*#[[:space:]]+\| ]]; then
        continue
    fi

    # Skip section headers, empty lines, and non-table lines
    if [[ -z "$line" ]] || [[ "$line" != "|"* ]]; then
        continue
    fi

    # Remove leading and trailing | then split by |
    trimmed="${line#"|"}"
    trimmed="${trimmed%"|"}"

    IFS='|' read -ra FIELDS <<< "$trimmed"
    # Clean up whitespace from each field
    for i in "${!FIELDS[@]}"; do
        FIELDS[$i]="$(echo "${FIELDS[$i]}" | tr -s '[:space:]' ' ' | sed 's/^ //;s/ $//')"
    done

    num_fields=${#FIELDS[@]}

    # For the main claims tables, we expect columns:
    # # | claim | source | function | test_name | status = 6 fields
    # For the REWORDED/REMOVED table:
    # # | original_claim | source | what_wrong | action = 5 fields
    # We only validate status and test_name for the 6-field tables.

    if [[ "$num_fields" -ge 6 ]]; then
        # 6-field table: status is last field, test_name is second-to-last
        status="${FIELDS[$((num_fields - 1))]}"
        test_name_raw="${FIELDS[$((num_fields - 2))]}"

        # Validate status
        if [[ "$status" != "VERIFIED" && "$status" != "REWORDED" && "$status" != "REMOVED" ]]; then
            # Skip if it looks like a separator (all dashes)
            if [[ "$status" == *---* ]]; then
                continue
            fi
            echo "ERROR: Invalid status '$status' in row: $line"
            ERRORS=$((ERRORS + 1))
            continue
        fi

        # For VERIFIED rows, test_name must be non-empty and exist in test.rs
        if [[ "$status" == "VERIFIED" ]]; then
            # Extract test names from backtick-quoted strings
            # May be "N/A" for design notes or contain multiple test names
            test_names=$(echo "$test_name_raw" | grep -oP '`\Ktest_\w+(?=`)' || true)

            if [[ -z "$test_names" ]]; then
                # Allow N/A for design-note rows (e.g., "N/A (design note)")
                if [[ "$test_name_raw" == "N/A"* ]]; then
                    continue
                fi
                echo "ERROR: VERIFIED row has no test function name in backticks: $line"
                ERRORS=$((ERRORS + 1))
                continue
            fi

            # Check each test name exists in test.rs
            while IFS= read -r tname; do
                if [[ -z "$tname" ]]; then
                    continue
                fi
                # Whole-word match: fn <name> in test.rs
                if ! grep -qE "fn ${tname}\b" "$TEST_FILE"; then
                    echo "ERROR: Test function '${tname}' not found in test.rs"
                    ERRORS=$((ERRORS + 1))
                fi
            done <<< "$test_names"
        fi
    fi

done < "$CLAIMS_FILE"

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

if [[ "$ERRORS" -gt 0 ]]; then
    echo ""
    echo "FAILED: $ERRORS error(s) found in claims audit."
    exit 1
fi

echo "OK: All claims audit checks passed."
exit 0
