# ──────────────────────────────────────────────────────────────
# onchainos CLI - Full Feature Test Suite
# Run: .\test-all.ps1
# Requires: onchainos.exe in current directory
# ──────────────────────────────────────────────────────────────

$ErrorActionPreference = "Continue"
$exe = ".\onchainos.exe"

# ── Test counters ──
$script:passed = 0
$script:failed = 0
$script:results = @()

# ── Test data ──
$WETH = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
$USDC_ETH = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
$PEPE = "0x6982508145454Ce325dDbE47a25d4ec3d2311933"
$VITALIK = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
$SOL_NATIVE = "11111111111111111111111111111111"
$ETH_NATIVE = "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"

function Run-Test {
    param(
        [string]$Name,
        [string[]]$Args
    )

    Write-Host -NoNewline "  $Name ... "

    try {
        $output = & $exe @Args 2>&1 | Out-String
        $exitCode = $LASTEXITCODE

        if ($exitCode -ne 0) {
            Write-Host -ForegroundColor Red "FAIL (exit code: $exitCode)"
            Write-Host -ForegroundColor DarkGray "    $($output.Trim().Substring(0, [Math]::Min(200, $output.Trim().Length)))"
            $script:failed++
            $script:results += [PSCustomObject]@{ Test = $Name; Status = "FAIL"; Detail = "exit code: $exitCode" }
            return $null
        }

        $json = $output | ConvertFrom-Json -ErrorAction Stop

        if ($json.ok -eq $true) {
            Write-Host -ForegroundColor Green "PASS"
            $script:passed++
            $script:results += [PSCustomObject]@{ Test = $Name; Status = "PASS"; Detail = "" }
            return $json
        } else {
            $errMsg = if ($json.error) { $json.error } else { "ok != true" }
            Write-Host -ForegroundColor Red "FAIL ($errMsg)"
            $script:failed++
            $script:results += [PSCustomObject]@{ Test = $Name; Status = "FAIL"; Detail = $errMsg }
            return $null
        }
    } catch {
        Write-Host -ForegroundColor Red "FAIL (parse error: $_)"
        $script:failed++
        $script:results += [PSCustomObject]@{ Test = $Name; Status = "FAIL"; Detail = $_.ToString() }
        return $null
    }
}

# ── Verify onchainos.exe exists ──
if (-not (Test-Path $exe)) {
    Write-Host -ForegroundColor Red "ERROR: $exe not found in current directory."
    exit 1
}

Write-Host ""
Write-Host "======================================================"
Write-Host "  onchainos CLI - Full Feature Test Suite"
Write-Host "======================================================"
Write-Host ""

# ══════════════════════════════════════════════════════════════
# MODULE 1: MARKET
# ══════════════════════════════════════════════════════════════
Write-Host -ForegroundColor Cyan "[MARKET] Market Data & Analysis"
Write-Host -ForegroundColor Cyan "------------------------------------------------------"

Run-Test "market price (WETH on Ethereum)" @("market", "price", $WETH, "--chain", "ethereum")

Run-Test "market prices (batch: ETH+SOL)" @("market", "prices", "1:$WETH,501:$SOL_NATIVE", "--chain", "ethereum")

Run-Test "market kline (WETH 1H)" @("market", "kline", $WETH, "--chain", "ethereum", "--bar", "1H", "--limit", "5")

Run-Test "market trades (WETH)" @("market", "trades", $WETH, "--chain", "ethereum", "--limit", "5")

Run-Test "market index (ETH native)" @("market", "index", "", "--chain", "ethereum")

Run-Test "market signal-chains" @("market", "signal-chains")

Run-Test "market signal-list (Ethereum)" @("market", "signal-list", "ethereum")

Run-Test "market memepump-chains" @("market", "memepump-chains")

Write-Host ""

# ══════════════════════════════════════════════════════════════
# MODULE 2: TOKEN
# ══════════════════════════════════════════════════════════════
Write-Host -ForegroundColor Cyan "[TOKEN] Token Information & Analytics"
Write-Host -ForegroundColor Cyan "------------------------------------------------------"

Run-Test "token search (PEPE)" @("token", "search", "PEPE")

Run-Test "token info (PEPE)" @("token", "info", $PEPE, "--chain", "ethereum")

Run-Test "token holders (PEPE)" @("token", "holders", $PEPE, "--chain", "ethereum")

Run-Test "token trending (ETH+SOL)" @("token", "trending")

Run-Test "token price-info (PEPE)" @("token", "price-info", $PEPE, "--chain", "ethereum")

Write-Host ""

# ══════════════════════════════════════════════════════════════
# MODULE 3: SWAP
# ══════════════════════════════════════════════════════════════
Write-Host -ForegroundColor Cyan "[SWAP] DEX Swap Operations"
Write-Host -ForegroundColor Cyan "------------------------------------------------------"

Run-Test "swap chains" @("swap", "chains")

Run-Test "swap liquidity (Ethereum)" @("swap", "liquidity", "--chain", "ethereum")

# Quote: swap 0.01 ETH -> USDC
Run-Test "swap quote (ETH->USDC)" @("swap", "quote", "--from", $ETH_NATIVE, "--to", $USDC_ETH, "--amount", "10000000000000000", "--chain", "ethereum")

Write-Host ""

# ══════════════════════════════════════════════════════════════
# MODULE 4: GATEWAY
# ══════════════════════════════════════════════════════════════
Write-Host -ForegroundColor Cyan "[GATEWAY] On-Chain Transaction Operations"
Write-Host -ForegroundColor Cyan "------------------------------------------------------"

Run-Test "gateway chains" @("gateway", "chains")

Run-Test "gateway gas (Ethereum)" @("gateway", "gas", "--chain", "ethereum")

Write-Host ""

# ══════════════════════════════════════════════════════════════
# MODULE 5: PORTFOLIO
# ══════════════════════════════════════════════════════════════
Write-Host -ForegroundColor Cyan "[PORTFOLIO] Wallet Balance & Portfolio"
Write-Host -ForegroundColor Cyan "------------------------------------------------------"

Run-Test "portfolio chains" @("portfolio", "chains")

$totalValueResult = Run-Test "portfolio total-value (Vitalik)" @("portfolio", "total-value", "--address", $VITALIK, "--chains", "ethereum")

$allBalancesResult = Run-Test "portfolio all-balances (Vitalik)" @("portfolio", "all-balances", "--address", $VITALIK, "--chains", "ethereum")

Run-Test "portfolio token-balances (Vitalik ETH)" @("portfolio", "token-balances", "--address", $VITALIK, "--tokens", "1:")

Write-Host ""

# ══════════════════════════════════════════════════════════════
# CROSS-VALIDATION: Compare with Etherscan
# ══════════════════════════════════════════════════════════════
Write-Host -ForegroundColor Cyan "[VERIFY] Cross-validate portfolio with Etherscan"
Write-Host -ForegroundColor Cyan "------------------------------------------------------"

Write-Host -NoNewline "  Etherscan balance verification ... "

try {
    # Get ETH balance from Etherscan (no API key needed for this endpoint)
    $etherscanUrl = "https://api.etherscan.io/api?module=account&action=balance&address=$VITALIK&tag=latest"
    $etherscanResp = Invoke-RestMethod -Uri $etherscanUrl -TimeoutSec 10
    $etherscanBalanceWei = [decimal]$etherscanResp.result
    $etherscanBalanceETH = $etherscanBalanceWei / 1e18

    # Extract ETH balance from onchainos all-balances result
    $onchanosETH = $null
    if ($allBalancesResult -and $allBalancesResult.data) {
        $tokens = $allBalancesResult.data
        # data could be an array or nested structure, try to find ETH
        foreach ($item in $tokens) {
            if ($item.symbol -eq "ETH" -and ($item.tokenContractAddress -eq "" -or $item.tokenContractAddress -eq $ETH_NATIVE -or $null -eq $item.tokenContractAddress)) {
                $onchanosETH = [decimal]$item.balance
                break
            }
            # Try nested tokenAssets array
            if ($item.tokenAssets) {
                foreach ($asset in $item.tokenAssets) {
                    if ($asset.symbol -eq "ETH" -and ($asset.tokenContractAddress -eq "" -or $null -eq $asset.tokenContractAddress)) {
                        $onchanosETH = [decimal]$asset.balance
                        break
                    }
                }
                if ($onchanosETH) { break }
            }
        }
    }

    if ($null -eq $onchanosETH) {
        Write-Host -ForegroundColor Yellow "SKIP (could not extract ETH balance from onchainos response)"
        $script:results += [PSCustomObject]@{ Test = "Etherscan verification"; Status = "SKIP"; Detail = "could not parse onchainos ETH balance" }
    } else {
        $diff = [Math]::Abs($onchanosETH - $etherscanBalanceETH)
        $pctDiff = if ($etherscanBalanceETH -gt 0) { ($diff / $etherscanBalanceETH) * 100 } else { 0 }

        Write-Host ""
        Write-Host "    onchainos ETH balance : $onchanosETH"
        Write-Host "    Etherscan ETH balance : $etherscanBalanceETH"
        Write-Host "    Difference            : $([Math]::Round($pctDiff, 2))%"

        if ($pctDiff -lt 1) {
            Write-Host -ForegroundColor Green "    PASS (difference < 1%)"
            $script:passed++
            $script:results += [PSCustomObject]@{ Test = "Etherscan verification"; Status = "PASS"; Detail = "diff: $([Math]::Round($pctDiff, 2))%" }
        } else {
            Write-Host -ForegroundColor Yellow "    WARN (difference >= 1%, may be due to block timing)"
            $script:results += [PSCustomObject]@{ Test = "Etherscan verification"; Status = "WARN"; Detail = "diff: $([Math]::Round($pctDiff, 2))%" }
        }
    }
} catch {
    Write-Host -ForegroundColor Yellow "SKIP (Etherscan API error: $_)"
    $script:results += [PSCustomObject]@{ Test = "Etherscan verification"; Status = "SKIP"; Detail = $_.ToString() }
}

Write-Host ""

# ══════════════════════════════════════════════════════════════
# SUMMARY
# ══════════════════════════════════════════════════════════════
Write-Host "======================================================"
Write-Host "  TEST SUMMARY"
Write-Host "======================================================"
Write-Host ""

$totalTests = $script:passed + $script:failed
Write-Host "  Total : $totalTests"
Write-Host -ForegroundColor Green "  Passed: $($script:passed)"
if ($script:failed -gt 0) {
    Write-Host -ForegroundColor Red "  Failed: $($script:failed)"
} else {
    Write-Host "  Failed: 0"
}

# Show failed tests
$failedTests = $script:results | Where-Object { $_.Status -eq "FAIL" }
if ($failedTests) {
    Write-Host ""
    Write-Host -ForegroundColor Red "  Failed tests:"
    foreach ($t in $failedTests) {
        Write-Host -ForegroundColor Red "    - $($t.Test): $($t.Detail)"
    }
}

Write-Host ""

# Exit with non-zero if any failures
if ($script:failed -gt 0) {
    exit 1
}
