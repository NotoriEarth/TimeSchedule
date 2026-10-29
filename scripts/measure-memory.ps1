param(
 [Parameter(Mandatory=$true)][int]$AppProcessId,
 [Parameter(Mandatory=$true)][string]$ExpectedExecutablePath,
 [datetime]$ExpectedStartTime,
 [switch]$AsObject
)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'benchmark-support.ps1')
$root=Get-BenchmarkRoot -AppProcessId $AppProcessId -ExpectedExecutablePath $ExpectedExecutablePath -ExpectedStartTime $ExpectedStartTime
$state=New-BenchmarkState -Root $root
$sample=Get-BenchmarkSample -State $state
if ($AsObject) { $sample } else { $sample | ConvertTo-Json -Depth 8 }
