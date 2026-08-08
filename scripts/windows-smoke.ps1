$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot '..\src-tauri\target\debug\cypherlinks.exe'
$exe = [System.IO.Path]::GetFullPath($exe)
if (-not (Test-Path $exe)) { throw "CypherLinks executable not found at $exe" }
$process = Start-Process -FilePath $exe -PassThru
try {
  Start-Sleep -Seconds 8
  if ($process.HasExited) { throw "CypherLinks exited during startup with code $($process.ExitCode)." }
  Write-Host "CypherLinks Windows smoke test passed. PID=$($process.Id)"
} finally {
  if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
}
