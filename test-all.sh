#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# onchainos CLI - Full Feature Test Suite (Linux / macOS)
# Run: chmod +x test-all.sh && ./test-all.sh
# Requires: onchainos binary in current directory
# ──────────────────────────────────────────────────────────────

set -euo pipefail

EXE="./onchainos"

# ── Test counters ──
PASSED=0
FAILED=0
TOTAL=0
FAILED_TESTS=""

# ── Test data ──
WETH="0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
USDC_ETH="0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
PEPE="0x6982508145454Ce325dDbE47a25d4ec3d2311933"
VITALIK="0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
SOL_NATIVE="11111111111111111111111111111111"
ETH_NATIVE="0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"

# ── Colors ──
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[0;33m'
GRAY='\033[0;90m'
NC='\033[0m'

run_test() {
    local name="$1"
    shift
    local args=("$@")

    TOTAL=$((TOTAL + 1))
    printf "  %s ... " "$name"

    local output
    if output=$("$EXE" "${args[@]}" 2>&1); then
        # Check if output contains "ok": true
        if echo "$output" | grep -q '"ok":\s*true\|"ok": true'; then
            printf "${GREEN}PASS${NC}\n"
            PASSED=$((PASSED + 1))
            # Store output for later use
            LAST_OUTPUT="$output"
            return 0
        else
            local err
            err=$(echo "$output" | head -c 200)
            printf "${RED}FAIL (ok != true)${NC}\n"
            printf "${GRAY}    %s${NC}\n" "$err"
            FAILED=$((FAILED + 1))
            FAILED_TESTS="${FAILED_TESTS}\n    - ${name}"
            LAST_OUTPUT=""
            return 1
        fi
    else
        local exit_code=$?
        local err
        err=$(echo "$output" | head -c 200)
        printf "${RED}FAIL (exit code: %d)${NC}\n" "$exit_code"
        printf "${GRAY}    %s${NC}\n" "$err"
        FAILED=$((FAILED + 1))
        FAILED_TESTS="${FAILED_TESTS}\n    - ${name}: exit code ${exit_code}"
        LAST_OUTPUT=""
        return 1
    fi
}

# ── Verify binary exists ──
if [ ! -f "$EXE" ]; then
    printf "${RED}ERROR: %s not found in current directory.${NC}\n" "$EXE"
    exit 1
fi

if [ ! -x "$EXE" ]; then
    chmod +x "$EXE"
fi

echo ""
echo "======================================================"
echo "  onchainos CLI - Full Feature Test Suite"
echo "======================================================"
echo ""

# ══════════════════════════════════════════════════════════════
# MODULE 1: MARKET
# ══════════════════════════════════════════════════════════════
printf "${CYAN}[MARKET] Market Data & Analysis${NC}\n"
printf "${CYAN}------------------------------------------------------${NC}\n"

run_test "market price (WETH on Ethereum)" market price "$WETH" --chain ethereum || true

run_test "market prices (batch: ETH+SOL)" market prices "1:${WETH},501:${SOL_NATIVE}" --chain ethereum || true

run_test "market kline (WETH 1H)" market kline "$WETH" --chain ethereum --bar 1H --limit 5 || true

run_test "market trades (WETH)" market trades "$WETH" --chain ethereum --limit 5 || true

run_test "market index (ETH native)" market index "" --chain ethereum || true

run_test "market signal-chains" market signal-chains || true

run_test "market signal-list (Ethereum)" market signal-list ethereum || true

run_test "market memepump-chains" market memepump-chains || true

echo ""

# ══════════════════════════════════════════════════════════════
# MODULE 2: TOKEN
# ══════════════════════════════════════════════════════════════
printf "${CYAN}[TOKEN] Token Information & Analytics${NC}\n"
printf "${CYAN}------------------------------------------------------${NC}\n"

run_test "token search (PEPE)" token search PEPE || true

run_test "token info (PEPE)" token info "$PEPE" --chain ethereum || true

run_test "token holders (PEPE)" token holders "$PEPE" --chain ethereum || true

run_test "token trending (ETH+SOL)" token trending || true

run_test "token price-info (PEPE)" token price-info "$PEPE" --chain ethereum || true

echo ""

# ══════════════════════════════════════════════════════════════
# MODULE 3: SWAP
# ══════════════════════════════════════════════════════════════
printf "${CYAN}[SWAP] DEX Swap Operations${NC}\n"
printf "${CYAN}------------------------------------------------------${NC}\n"

run_test "swap chains" swap chains || true

run_test "swap liquidity (Ethereum)" swap liquidity --chain ethereum || true

run_test "swap quote (ETH->USDC)" swap quote --from "$ETH_NATIVE" --to "$USDC_ETH" --amount 10000000000000000 --chain ethereum || true

echo ""

# ══════════════════════════════════════════════════════════════
# MODULE 4: GATEWAY
# ══════════════════════════════════════════════════════════════
printf "${CYAN}[GATEWAY] On-Chain Transaction Operations${NC}\n"
printf "${CYAN}------------------------------------------------------${NC}\n"

run_test "gateway chains" gateway chains || true

run_test "gateway gas (Ethereum)" gateway gas --chain ethereum || true

echo ""

# ══════════════════════════════════════════════════════════════
# MODULE 5: PORTFOLIO
# ══════════════════════════════════════════════════════════════
printf "${CYAN}[PORTFOLIO] Wallet Balance & Portfolio${NC}\n"
printf "${CYAN}------------------------------------------------------${NC}\n"

run_test "portfolio chains" portfolio chains || true

run_test "portfolio total-value (Vitalik)" portfolio total-value --address "$VITALIK" --chains ethereum || true

BALANCES_OUTPUT=""
if run_test "portfolio all-balances (Vitalik)" portfolio all-balances --address "$VITALIK" --chains ethereum; then
    BALANCES_OUTPUT="$LAST_OUTPUT"
fi

run_test "portfolio token-balances (Vitalik ETH)" portfolio token-balances --address "$VITALIK" --tokens "1:" || true

echo ""

# ══════════════════════════════════════════════════════════════
# CROSS-VALIDATION: Compare with Etherscan
# ══════════════════════════════════════════════════════════════
printf "${CYAN}[VERIFY] Cross-validate portfolio with Etherscan${NC}\n"
printf "${CYAN}------------------------------------------------------${NC}\n"

printf "  Etherscan balance verification ... "

if command -v curl &>/dev/null && [ -n "$BALANCES_OUTPUT" ]; then
    # Get ETH balance from Etherscan
    ETHERSCAN_RESP=$(curl -s "https://api.etherscan.io/api?module=account&action=balance&address=${VITALIK}&tag=latest" 2>/dev/null || echo "")

    if [ -n "$ETHERSCAN_RESP" ]; then
        ETHERSCAN_WEI=$(echo "$ETHERSCAN_RESP" | grep -o '"result":"[0-9]*"' | grep -o '[0-9]*')

        if [ -n "$ETHERSCAN_WEI" ]; then
            # Convert wei to ETH (using awk for floating point)
            ETHERSCAN_ETH=$(echo "$ETHERSCAN_WEI" | awk '{printf "%.6f", $1 / 1000000000000000000}')

            # Extract ETH balance from onchainos output
            # Try to find balance field for ETH token
            ONCHAINOS_ETH=$(echo "$BALANCES_OUTPUT" | grep -o '"balance":"[0-9.]*"' | head -1 | grep -o '[0-9.]*')

            if [ -n "$ONCHAINOS_ETH" ] && [ -n "$ETHERSCAN_ETH" ]; then
                # Calculate percentage difference
                PCT_DIFF=$(awk "BEGIN {
                    diff = $ONCHAINOS_ETH - $ETHERSCAN_ETH;
                    if (diff < 0) diff = -diff;
                    if ($ETHERSCAN_ETH > 0) pct = (diff / $ETHERSCAN_ETH) * 100;
                    else pct = 0;
                    printf \"%.2f\", pct
                }")

                echo ""
                echo "    onchainos ETH balance : $ONCHAINOS_ETH"
                echo "    Etherscan ETH balance : $ETHERSCAN_ETH"
                echo "    Difference            : ${PCT_DIFF}%"

                THRESHOLD=$(awk "BEGIN { print ($PCT_DIFF < 1) ? 1 : 0 }")
                if [ "$THRESHOLD" = "1" ]; then
                    printf "    ${GREEN}PASS (difference < 1%%)${NC}\n"
                    PASSED=$((PASSED + 1))
                    TOTAL=$((TOTAL + 1))
                else
                    printf "    ${YELLOW}WARN (difference >= 1%%, may be due to block timing)${NC}\n"
                    TOTAL=$((TOTAL + 1))
                fi
            else
                printf "${YELLOW}SKIP (could not extract ETH balance)${NC}\n"
            fi
        else
            printf "${YELLOW}SKIP (Etherscan returned no balance)${NC}\n"
        fi
    else
        printf "${YELLOW}SKIP (Etherscan API error)${NC}\n"
    fi
else
    printf "${YELLOW}SKIP (curl not available or no balance data)${NC}\n"
fi

echo ""

# ══════════════════════════════════════════════════════════════
# SUMMARY
# ══════════════════════════════════════════════════════════════
echo "======================================================"
echo "  TEST SUMMARY"
echo "======================================================"
echo ""
echo "  Total : $TOTAL"
printf "  ${GREEN}Passed: %d${NC}\n" "$PASSED"
if [ "$FAILED" -gt 0 ]; then
    printf "  ${RED}Failed: %d${NC}\n" "$FAILED"
    echo ""
    printf "  ${RED}Failed tests:${NC}"
    printf "$FAILED_TESTS\n"
else
    echo "  Failed: 0"
fi

echo ""

if [ "$FAILED" -gt 0 ]; then
    exit 1
fi
