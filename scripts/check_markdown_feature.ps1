# Repeatable QA entry point; every process uses isolated fixture settings and storage.
param([ValidateSet('Core','Native')][string]$Suite='Core',[switch]$SkipBuild,[switch]$VerifyEvidence)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $taskRoot
$evidenceBase=Join-Path $taskRoot 'target/markdown-acceptance'
$sealPath=Join-Path $evidenceBase ('latest-'+$Suite+'.json')
$mcpBinary=Join-Path $taskRoot 'src-tauri/target/debug/adashi-mcp.exe'
function SourceSnapshot {
    $paths=@('src','src-tauri/src','src-tauri/storage-api/src','src-tauri/storage-api/tests','scripts') | ForEach-Object {
        Get-ChildItem -LiteralPath (Join-Path $taskRoot $_) -Recurse -File | Where-Object {$_.Extension -in '.rs','.ts','.tsx','.js','.mjs','.css','.py','.ps1'} | Select-Object -ExpandProperty FullName
    }
    $paths+=@('agents_template.md','package.json','package-lock.json','src-tauri/Cargo.toml','src-tauri/Cargo.lock','src-tauri/storage-api/Cargo.toml') | ForEach-Object {Join-Path $taskRoot $_}
    @($paths | Sort-Object -Unique | ForEach-Object {[pscustomobject]@{Path=$_.Substring($taskRoot.Length+1);Hash=(Get-FileHash -LiteralPath $_).Hash}})
}
function AssertSameSources($Expected,$Actual,[string]$Message) {
    # Windows PowerShell and PowerShell 7 sort punctuation differently. Compare
    # the complete path/hash sets rather than their serialization order.
    if (@($Expected).Count -ne @($Actual).Count -or
        @(Compare-Object -ReferenceObject @($Expected) -DifferenceObject @($Actual) -Property Path,Hash).Count -ne 0) {
        throw $Message
    }
}
if ($VerifyEvidence) {
    $seal=Get-Content -LiteralPath $sealPath -Raw | ConvertFrom-Json
    if (!$seal.Success -or $seal.Suite -ne $Suite) {throw 'No successful matching acceptance evidence'}
    $current=SourceSnapshot
    AssertSameSources $seal.Sources $current 'Source changed after acceptance; rerun the suite'
    foreach($item in @($seal.Artifacts)+@($seal.Logs)) {
        if((Get-FileHash -LiteralPath $item.Path).Hash -ne $item.Hash){throw ('Evidence or binary changed: '+$item.Path)}
    }
    Write-Output ('Verified '+$Suite+' acceptance completed '+$seal.CompletedAt)
    Write-Output ('Executed by the workspace runner; installed QA host verifies matching source, binaries and retained logs.')
    Write-Output ('Evidence: '+$seal.EvidenceRoot)
    foreach($log in $seal.Logs | Where-Object {$_.Path -notlike '*.stderr.log'}) {Write-Output $log.Path;Get-Content -LiteralPath $log.Path -Tail 8}
    exit 0
}
$evidenceRoot=Join-Path $evidenceBase ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$initialSources=SourceSnapshot
function Step([string]$Name,[string]$Command,[string[]]$Arguments) {
    $log=Join-Path $evidenceRoot ($Name+'.log')
    Write-Output ('RUN '+$Name)
    $resolvedCommand=(Get-Command $Command -ErrorAction Stop).Source
    $errorLog=Join-Path $evidenceRoot ($Name+'.stderr.log')
    # Native tools and their services get ordinary file handles, not PowerShell's pipeline.
    $quotedArguments=$Arguments | ForEach-Object { '"'+[regex]::Replace([regex]::Replace($_,'(\\*)"','$1$1\"'),'(\\+)$','$1$1')+'"' }
    $process=Start-Process -FilePath $resolvedCommand -ArgumentList $quotedArguments -WorkingDirectory $taskRoot -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $errorLog -PassThru -Wait
    $stepExit=$process.ExitCode
    if ($stepExit -ne 0) {
        Get-Content -LiteralPath $log -Tail 50
        Get-Content -LiteralPath $errorLog -Tail 30
        throw "$Name failed; complete log: $log"
    }
    Get-Content -LiteralPath $log -Tail 8
    Write-Output ('PASS '+$Name)
}
try {
    if ($Suite -eq 'Core') {
        Step 'versioned-drafts' 'node' @('scripts/check_versioned_draft.mjs')
        Step 'storage-api' 'cargo' @('test','--manifest-path','src-tauri/storage-api/Cargo.toml')
        Step 'rust-regression' 'cargo' @('test','--manifest-path','src-tauri/Cargo.toml','--lib')
        if (!$SkipBuild) {
            Step 'typecheck' 'node' @('node_modules/typescript/bin/tsc')
            Step 'frontend-build' 'node' @('node_modules/vite/bin/vite.js','build')
            Step 'native-mcp-build' 'cargo' @('build','--manifest-path','src-tauri/Cargo.toml','--bins')
        }
        Get-FileHash $mcpBinary,(Join-Path $taskRoot 'src-tauri/target/debug/adashi.exe') | Format-List | Out-String | Write-Output
        Step 'markdown-stdio' 'python' @('scripts/check_markdown_mcp.py','--binary',$mcpBinary)
        Step 'storage-core' 'python' @('scripts/check_storage_core.py','--binary',$mcpBinary)
        Step 'read-only-storage' 'python' @('scripts/check_read_only_storage.py','--binary',$mcpBinary)
        Step 'sqlite-parity' 'python' @('scripts/check_storage_parity.py','--binary',$mcpBinary,'--backend','sqlite')
        Step 'text-parity' 'python' @('scripts/check_storage_parity.py','--binary',$mcpBinary,'--backend','text')
        Step 'git-collaboration' 'python' @('scripts/check_git_collaboration.py','--binary',$mcpBinary)
    } else {
        if (!$env:ADASHI_PLAYWRIGHT_PACKAGE) {
            $bundledPlaywright=Join-Path $env:USERPROFILE '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright'
            if (Test-Path -LiteralPath $bundledPlaywright) {$env:ADASHI_PLAYWRIGHT_PACKAGE=$bundledPlaywright}
        }
        $env:ADASHI_TEST_MARKDOWN_DOCUMENTS='1'
        $env:ADASHI_TEST_MARKDOWN_EDITOR='1'
        $env:ADASHI_TEST_MARKDOWN_IMPORT='1'
        $env:ADASHI_TEST_MARKDOWN_PROJECTION='1'
        $env:ADASHI_TEST_MARKDOWN_LARGE='1'
        $env:ADASHI_TEST_MIGRATION='0'
        $env:ADASHI_TEST_BACKEND='sqlite'
        Step 'native-sqlite' 'node' @('scripts/check_native_storage.mjs')
        $env:ADASHI_TEST_BACKEND='text'
        Step 'native-text' 'node' @('scripts/check_native_storage.mjs')
    }
    $finalSources=SourceSnapshot
    AssertSameSources $initialSources $finalSources 'Source changed while acceptance was running; rerun before recording success'
    $artifacts=@($mcpBinary,(Join-Path $taskRoot 'src-tauri/target/debug/adashi.exe'),(Join-Path $taskRoot 'dist/index.html')) | ForEach-Object {[pscustomobject]@{Path=$_;Hash=(Get-FileHash -LiteralPath $_).Hash}}
    $logs=Get-ChildItem -LiteralPath $evidenceRoot -File | ForEach-Object {[pscustomobject]@{Path=$_.FullName;Hash=(Get-FileHash -LiteralPath $_.FullName).Hash}}
    [pscustomobject]@{Success=$true;Suite=$Suite;CompletedAt=[DateTime]::UtcNow.ToString('o');EvidenceRoot=$evidenceRoot;Sources=$finalSources;Artifacts=@($artifacts);Logs=@($logs)} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $sealPath -Encoding utf8
    Write-Output ('Evidence: '+$evidenceRoot)
    Write-Output ('SUCCESS '+$Suite)
} catch {
    Write-Output $_
    Write-Output ('Evidence: '+$evidenceRoot)
    exit 1
}
