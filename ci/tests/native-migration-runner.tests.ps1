# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../native-migration-runner.ps1')
$taskShell = (Get-Process -Id $PID).Path
$taskTemporary = Join-Path ([IO.Path]::GetTempPath()) ('vauchi-runner-' + [guid]::NewGuid())
New-Item -ItemType Directory $taskTemporary | Out-Null
try {
    $taskCases = @(
        @{ Name = 'backend'; Command = "Write-Output 'running 1 test'; exit 17"; Expected = 'TEST_BACKEND_FAILED: exit 17' },
        @{ Name = 'empty'; Command = "Write-Output 'running 0 tests'; exit 0"; Expected = 'NO_EXECUTED_TESTS:' },
        @{ Name = 'listing'; Command = "Write-Output 'one_test: test'; exit 0"; Expected = 'NO_EXECUTED_TESTS:' },
        @{ Name = 'early-exit'; Command = "Write-Output 'running 1 test'; exit 0"; Expected = 'INCOMPLETE_TEST_RUN:' },
        @{ Name = 'partial'; Command = "Write-Output 'running 2 tests'; Write-Output 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;'; exit 0"; Expected = 'INCOMPLETE_TEST_RUN:' }
    )
    foreach ($taskCase in $taskCases) {
        $taskObserved = ''
        try {
            Invoke-RustTest -Executable $taskShell `
                -TestArguments @('-NoProfile', '-Command', $taskCase.Command) `
                -Log (Join-Path $taskTemporary ($taskCase.Name + '.log'))
        } catch { $taskObserved = $_.Exception.Message }
        if (-not $taskObserved.StartsWith($taskCase.Expected)) {
            throw "Unexpected negative-control outcome: $taskObserved"
        }
    }
    $taskLog = Join-Path $taskTemporary 'healthy.log'
    Invoke-RustTest -Executable $taskShell `
        -TestArguments @('-NoProfile', '-Command', "Write-Output 'running 1 test'; Write-Output 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;'; exit 0") `
        -Log $taskLog
    if ((Get-Content $taskLog).Count -ne 2 -or (Get-Content $taskLog)[1] -ne 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;') {
        throw 'The successful backend output was not preserved'
    }
    Write-Host 'RUNNER_CONTROLS_PASSED: failure, zero, listing, early exit, partial, healthy'
} finally {
    Remove-Item -LiteralPath $taskTemporary -Recurse -Force
}
