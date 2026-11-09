# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This acceptance run requires native Windows' }
Write-Host ([System.Runtime.InteropServices.RuntimeInformation]::OSDescription)
. (Join-Path $PSScriptRoot 'native-migration-runner.ps1')

$taskRoot = Join-Path $PSScriptRoot '../migration-safety-windows'
$taskManifest = @(Get-Content (Join-Path $taskRoot 'SHA256SUMS'))
if ($taskManifest.Count -ne 4) { throw 'Expected exactly four native test artifacts' }
foreach ($taskLine in $taskManifest) {
    $taskFields = $taskLine -split '\s+', 2
    $taskFile = Join-Path $taskRoot (Split-Path $taskFields[1] -Leaf)
    $taskHash = (Get-FileHash -Algorithm SHA256 $taskFile).Hash.ToLowerInvariant()
    if ($taskHash -ne $taskFields[0]) { throw 'Native test artifact checksum mismatch' }
}

$taskControl = Join-Path $taskRoot 'migration_snapshot_safety.exe'
foreach ($taskCase in @(
    @{ Argument = '--list'; Error = 'NO_EXECUTED_TESTS:' },
    @{ Argument = '--invalid-option-for-failure-control'; Error = 'TEST_BACKEND_FAILED:' }
)) {
    $taskObserved = $false
    try {
        Invoke-RustTest -Executable $taskControl -TestArguments @($taskCase.Argument) `
            -Log (Join-Path $taskRoot ('control-' + $taskCase.Argument.TrimStart('-') + '.log'))
    } catch {
        if (-not $_.Exception.Message.StartsWith($taskCase.Error)) { throw }
        $taskObserved = $true
    }
    if (-not $taskObserved) { throw 'Negative control did not reject invalid validation' }
    Write-Host "Observed expected rejection: $($taskCase.Error)"
}

$taskFailures = @()
foreach ($taskName in @('migration_snapshot_safety', 'migration_commit_durability', 'shred_profile_scope', 'profile_directory_authority')) {
    try {
        Invoke-RustTest -Executable (Join-Path $taskRoot ($taskName + '.exe')) `
            -TestArguments @('--nocapture') -Log (Join-Path $taskRoot ($taskName + '.log'))
    } catch {
        $taskFailures += $taskName
        Write-Host ("Native suite failed: " + $taskName + ": " + $_.Exception.Message)
    }
}
if ($taskFailures.Count -ne 0) { throw ('Native suites failed: ' + ($taskFailures -join ', ')) }
