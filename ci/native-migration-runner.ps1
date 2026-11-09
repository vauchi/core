# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

function Invoke-RustTest {
    param([string]$Executable, [string[]]$TestArguments, [string]$Log)
    $taskOutput = & $Executable @TestArguments 2>&1 | Tee-Object -FilePath $Log
    $taskStatus = $LASTEXITCODE
    $taskOutput | ForEach-Object { Write-Host $_ }
    if ($taskStatus -ne 0) { throw "TEST_BACKEND_FAILED: exit $taskStatus" }
    $taskText = $taskOutput | Out-String
    if ($taskText -notmatch '(?m)^\s*running ([1-9][0-9]*) tests?\s*$') {
        throw 'NO_EXECUTED_TESTS: a listing or empty filter is not validation'
    }
    $taskAnnounced = [int]$Matches[1]
    $taskSummary = [regex]::Match($taskText, '(?m)^test result: ok\. ([0-9]+) passed; 0 failed; ([0-9]+) ignored; ([0-9]+) measured;')
    if (-not $taskSummary.Success) { throw 'INCOMPLETE_TEST_RUN: no successful completion' }
    $taskPassed = [int]$taskSummary.Groups[1].Value
    $taskCompleted = $taskPassed + [int]$taskSummary.Groups[2].Value + [int]$taskSummary.Groups[3].Value
    if ($taskPassed -eq 0 -or $taskCompleted -ne $taskAnnounced) {
        throw 'INCOMPLETE_TEST_RUN: completed count does not match announced tests'
    }
}
