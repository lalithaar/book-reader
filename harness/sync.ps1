# Regenerate harness/index.html from ui/index.html so the browser harness never
# drifts from the real markup. Adds the IPC mock and points at ../ui assets.
$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root 'ui\index.html'
$target = Join-Path $PSScriptRoot 'index.html'

(Get-Content $source -Raw) `
  -replace 'href="app\.css"', 'href="../ui/app.css"' `
  -replace '<script src="app\.js"></script>', '<script src="_mock.js"></script><script src="../ui/app.js"></script>' |
  Set-Content -Path $target -NoNewline -Encoding utf8

Write-Host "synced $target from $source"
