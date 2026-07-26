<#
.SYNOPSIS
  CN-Codex AA-aligned local coding-agent benchmark runner.

.EXAMPLE
  pwsh -File benchmarks/aa-aligned/runner.ps1 -Action init
  pwsh -File benchmarks/aa-aligned/runner.ps1 -Action grade -TaskId term-01-json-transform -Attempt 1
  pwsh -File benchmarks/aa-aligned/runner.ps1 -Action summarize
  pwsh -File benchmarks/aa-aligned/runner.ps1 -Action selftest
#>
param(
  [ValidateSet("init", "grade", "summarize", "selftest", "status")]
  [string]$Action = "status",

  [string]$TaskId,
  [int]$Attempt = 1,
  [string]$Model = "grok-4.5-high",
  [string]$Harness = "CN-Codex",
  [string]$Notes = ""
)

$ErrorActionPreference = "Stop"
$SuiteRoot = $PSScriptRoot
$RepoRoot = (Resolve-Path (Join-Path $SuiteRoot "../..")).Path
$SuiteJsonPath = Join-Path $SuiteRoot "suite.json"
$WorkRoot = Join-Path $SuiteRoot "workspaces"
$ResultRoot = Join-Path $SuiteRoot "results"
$AttemptsPath = Join-Path $ResultRoot "attempts.jsonl"

function Read-Suite {
  return Get-Content -Raw -Encoding UTF8 $SuiteJsonPath | ConvertFrom-Json
}

function Ensure-Dirs {
  New-Item -ItemType Directory -Force -Path $WorkRoot | Out-Null
  New-Item -ItemType Directory -Force -Path $ResultRoot | Out-Null
}

function Get-Task([string]$id) {
  $suite = Read-Suite
  $task = $suite.tasks | Where-Object { $_.id -eq $id } | Select-Object -First 1
  if (-not $task) { throw "Unknown task id: $id" }
  return $task
}

function Init-TaskWorkspace([string]$id) {
  $taskDir = Join-Path $WorkRoot $id
  if (Test-Path $taskDir) {
    Remove-Item -Recurse -Force $taskDir
  }
  New-Item -ItemType Directory -Force -Path $taskDir | Out-Null

  $fixture = Join-Path $SuiteRoot "fixtures/$id"
  if (Test-Path $fixture) {
    Copy-Item -Recurse -Force (Join-Path $fixture '*') $taskDir
  }

  $promptSrc = Join-Path $SuiteRoot "tasks/$id/PROMPT.md"
  if (Test-Path $promptSrc) {
    Copy-Item -Force $promptSrc (Join-Path $taskDir "PROMPT.md")
  }
  return $taskDir
}

function Invoke-Grade([string]$id, [string]$workspace) {
  $gradeScript = Join-Path $SuiteRoot "tasks/$id/grade.ps1"
  if (-not (Test-Path $gradeScript)) {
    throw "Missing grade script: $gradeScript"
  }

  # Pass only parameters declared by the grade script.
  $meta = Get-Command $gradeScript
  $params = @{ Workspace = $workspace }
  if ($meta.Parameters.ContainsKey("RepoRoot")) {
    $params.RepoRoot = $RepoRoot
  }
  $result = & $gradeScript @params
  if ($null -eq $result) {
    throw "Grade script returned null for $id"
  }
  if ($result -is [hashtable]) {
    return $result
  }
  # PSCustomObject fallback
  return @{
    pass = [int]$result.pass
    reason = [string]$result.reason
  }
}

function Write-Attempt($record) {
  Ensure-Dirs
  $line = ($record | ConvertTo-Json -Compress)
  Add-Content -Path $AttemptsPath -Value $line -Encoding UTF8
}

function Action-Init {
  Ensure-Dirs
  $suite = Read-Suite
  foreach ($t in $suite.tasks) {
    $path = Init-TaskWorkspace $t.id
    Write-Host "initialized $($t.id) -> $path"
  }
  Write-Host "Done. Open workspaces/<task_id>/PROMPT.md in CN-Codex with model $Model"
}

function Action-Grade {
  if ([string]::IsNullOrWhiteSpace($TaskId)) { throw "-TaskId is required for grade" }
  if ($Attempt -lt 1 -or $Attempt -gt 3) { throw "-Attempt must be 1..3 for AA-style pass@1 tracking" }

  Ensure-Dirs
  $task = Get-Task $TaskId
  $workspace = Join-Path $WorkRoot $TaskId
  if (-not (Test-Path $workspace)) {
    Write-Host "workspace missing, initializing..."
    Init-TaskWorkspace $TaskId | Out-Null
  }

  $started = Get-Date
  $grade = Invoke-Grade -id $TaskId -workspace $workspace
  $ended = Get-Date

  $record = [ordered]@{
    ts = (Get-Date).ToString("o")
    model = $Model
    harness = $Harness
    task_id = $TaskId
    component = $task.component
    attempt = $Attempt
    pass = [int]$grade.pass
    reason = [string]$grade.reason
    wall_time_sec = [math]::Round(($ended - $started).TotalSeconds, 3)
    notes = $Notes
  }
  Write-Attempt $record
  Write-Host ("[{0}] attempt={1} pass={2} reason={3}" -f $TaskId, $Attempt, $record.pass, $record.reason)
  $out = Join-Path $ResultRoot ("{0}-attempt{1}.json" -f $TaskId, $Attempt)
  ($record | ConvertTo-Json -Depth 5) | Set-Content -Path $out -Encoding UTF8
}

function Read-Attempts {
  if (-not (Test-Path $AttemptsPath)) { return @() }
  $rows = @()
  foreach ($line in Get-Content -Encoding UTF8 $AttemptsPath) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    $rows += ($line | ConvertFrom-Json)
  }
  return $rows
}

function Action-Summarize {
  Ensure-Dirs
  $suite = Read-Suite
  $rows = @(Read-Attempts)
  if ($rows.Count -eq 0) {
    Write-Host "No attempts yet. Run grade first."
    return
  }

  # Keep latest record per task/attempt
  $latest = @{}
  foreach ($r in $rows) {
    $key = "{0}#{1}" -f $r.task_id, $r.attempt
    $latest[$key] = $r
  }
  $dedup = @($latest.Values)

  $components = @("repo_qa", "terminal", "swe_edit")
  $componentScores = @{}
  $taskScores = @()

  foreach ($t in $suite.tasks) {
    $attempts = @($dedup | Where-Object { $_.task_id -eq $t.id })
    if ($attempts.Count -eq 0) {
      $taskScores += [pscustomobject]@{
        task_id = $t.id
        component = $t.component
        attempts = 0
        pass1 = $null
        status = "missing"
      }
      continue
    }
    # AA style: average over available attempts for the task (ideally 3)
    $pass1 = ($attempts | Measure-Object -Property pass -Average).Average
    $taskScores += [pscustomobject]@{
      task_id = $t.id
      component = $t.component
      attempts = $attempts.Count
      pass1 = [math]::Round($pass1, 4)
      status = "scored"
    }
  }

  foreach ($c in $components) {
    $scored = @($taskScores | Where-Object { $_.component -eq $c -and $null -ne $_.pass1 })
    if ($scored.Count -eq 0) {
      $componentScores[$c] = $null
    } else {
      $componentScores[$c] = [math]::Round((($scored | Measure-Object -Property pass1 -Average).Average), 4)
    }
  }

  $present = @($componentScores.GetEnumerator() | Where-Object { $null -ne $_.Value })
  $proxyIndex = $null
  if ($present.Count -eq 3) {
    $proxyIndex = [math]::Round((($present | ForEach-Object { $_.Value } | Measure-Object -Average).Average), 4)
  }

  $ref = $suite.official_reference_scores.grok_4_5_high
  $summary = [ordered]@{
    generated_at = (Get-Date).ToString("o")
    model = $Model
    harness = $Harness
    local_proxy_index = $proxyIndex
    components = $componentScores
    task_scores = $taskScores
    official_reference = @{
      model = "Grok 4.5 (high)"
      intelligence_index = $ref.intelligence_index
      coding_agent_index_grok_build = $ref.coding_agent_index_grok_build
      note = "Official AA score is model+harness specific. Compare only as reference, not absolute equality."
    }
    interpretation = $(
      if ($null -eq $proxyIndex) {
        "Incomplete: need at least one scored task in each component (repo_qa/terminal/swe_edit)."
      } elseif ($proxyIndex -ge 0.8) {
        "Very strong local agent performance (proxy). AA Grok4.5@GrokBuild Coding Agent Index ref=76."
      } elseif ($proxyIndex -ge 0.6) {
        "Strong local agent performance. Investigate remaining failures vs harness limits."
      } elseif ($proxyIndex -ge 0.4) {
        "Medium. Good on some axes, weak on others; check component breakdown."
      } else {
        "Weak on this proxy suite. Likely tool-use / multi-step reliability issues in current harness settings."
      }
    )
  }

  $jsonPath = Join-Path $ResultRoot "latest-summary.json"
  ($summary | ConvertTo-Json -Depth 8) | Set-Content -Path $jsonPath -Encoding UTF8

  $md = @()
  $md += "# CN-Codex AA-Aligned Scorecard"
  $md += ""
  $md += "- Generated: $($summary.generated_at)"
  $md += "- Model: **$Model**"
  $md += "- Harness: **$Harness**"
  $md += "- Local Coding Agent Proxy Index: **$proxyIndex**"
  $md += "- AA reference Grok 4.5 @ Grok Build Coding Agent Index: **$($ref.coding_agent_index_grok_build)**"
  $md += "- AA reference Grok 4.5 Intelligence Index: **$($ref.intelligence_index)**"
  $md += ""
  $md += "## Component pass@1"
  $md += ""
  $md += "| Component | Aligns to | pass@1 |"
  $md += "|-----------|-----------|--------|"
  $md += "| repo_qa | SWE-Atlas-QnA | $($componentScores.repo_qa) |"
  $md += "| terminal | Terminal-Bench v2 | $($componentScores.terminal) |"
  $md += "| swe_edit | DeepSWE | $($componentScores.swe_edit) |"
  $md += ""
  $md += "## Tasks"
  $md += ""
  $md += "| Task | Component | Attempts | pass@1 | Status |"
  $md += "|------|-----------|----------|--------|--------|"
  foreach ($ts in $taskScores) {
    $md += "| $($ts.task_id) | $($ts.component) | $($ts.attempts) | $($ts.pass1) | $($ts.status) |"
  }
  $md += ""
  $md += "## Interpretation"
  $md += ""
  $md += $summary.interpretation
  $md += ""
  $md += "## How to read against AA leaderboard"
  $md += ""
  $md += "1. AA scores **model + harness**. Your number is **$Model @ $Harness**."
  $md += "2. This local suite is a **proxy** (12 tasks), not the official 321-task Coding Agent Index."
  $md += "3. Use component breakdown to see if gaps are Q&A, terminal, or multi-file SWE."
  $md += "4. If local proxy << 0.76 while AA Grok Build is 76, gap is likely harness/tooling/settings, not just model IQ."
  $md += ""
  $md += "Official links:"
  $md += "- https://artificialanalysis.ai/agents/coding-agents"
  $md += "- https://artificialanalysis.ai/models/grok-4-5"
  $md += "- https://artificialanalysis.ai/methodology/coding-agents-benchmarking"

  $mdPath = Join-Path $ResultRoot "latest-scorecard.md"
  Set-Content -Path $mdPath -Value ($md -join "`n") -Encoding UTF8

  Write-Host "Summary written:"
  Write-Host "  $jsonPath"
  Write-Host "  $mdPath"
  Write-Host "Proxy Index = $proxyIndex"
}

function Install-GoldSolution([string]$id, [string]$workspace) {
  switch ($id) {
    "qa-01-find-entrypoint" {
      @{
        entrypoint = "src/main.tsx"
        ui_framework = "react"
        desktop_framework = "tauri"
        test_script = "test"
      } | ConvertTo-Json | Set-Content (Join-Path $workspace "answer.json") -Encoding UTF8
    }
    "qa-02-config-provider" {
      $cfg = Get-Content -Raw -Encoding UTF8 (Join-Path $RepoRoot "codey/config.toml")
      function Get-TomlScalar([string]$text, [string]$key) {
        $m = [regex]::Match($text, '(?m)^\s*' + [regex]::Escape($key) + '\s*=\s*"([^"]*)"')
        if ($m.Success) { return $m.Groups[1].Value }
        return $null
      }
      @{
        model = (Get-TomlScalar $cfg "model")
        model_provider = (Get-TomlScalar $cfg "model_provider")
        web_search = (Get-TomlScalar $cfg "web_search")
        approval_policy = (Get-TomlScalar $cfg "approval_policy")
      } | ConvertTo-Json | Set-Content (Join-Path $workspace "answer.json") -Encoding UTF8
    }
    "qa-03-tool-pipeline" {
      @{
        has_apply_patch = $true
        has_browser_run = $true
        has_tool_search = $true
        source_file = "src/hooks/useTauriEvents.ts"
      } | ConvertTo-Json | Set-Content (Join-Path $workspace "answer.json") -Encoding UTF8
    }
    "qa-04-test-command" {
      @{
        test_script = "test"
        runner = "vitest"
        has_watch_script = $true
        rust_manifest = "src-tauri/Cargo.toml"
      } | ConvertTo-Json | Set-Content (Join-Path $workspace "answer.json") -Encoding UTF8
    }
    "term-01-json-transform" {
      New-Item -ItemType Directory -Force -Path (Join-Path $workspace "output") | Out-Null
      @(
        @{ id = 3; name = "Grace"; score = 99 },
        @{ id = 1; name = "Ada"; score = 90 },
        @{ id = 5; name = "Eve"; score = 70 }
      ) | ConvertTo-Json | Set-Content (Join-Path $workspace "output/top_active.json") -Encoding UTF8
      Set-Content (Join-Path $workspace "output/summary.txt") -Value "count=3;max=99" -Encoding UTF8
    }
    "term-02-log-etl" {
      New-Item -ItemType Directory -Force -Path (Join-Path $workspace "output") | Out-Null
      @(
        "level,count",
        "DEBUG,1",
        "ERROR,2",
        "INFO,3",
        "WARN,1"
      ) | Set-Content (Join-Path $workspace "output/levels.csv") -Encoding UTF8
      @("disk full", "auth failed") | Set-Content (Join-Path $workspace "output/errors.txt") -Encoding UTF8
    }
    "term-03-batch-rename" {
      $outDir = Join-Path $workspace "output/renamed"
      New-Item -ItemType Directory -Force -Path $outDir | Out-Null
      Set-Content (Join-Path $outDir "note_a.txt") -Value "alpha" -Encoding UTF8
      Set-Content (Join-Path $outDir "hello_world.txt") -Value "beta" -Encoding UTF8
      Set-Content (Join-Path $outDir "report_final.txt") -Value "gamma" -Encoding UTF8
      @{
        files = @(
          @{ from = "Hello World.txt"; to = "hello_world.txt" },
          @{ from = "note a.txt"; to = "note_a.txt" },
          @{ from = "REPORT Final.txt"; to = "report_final.txt" }
        )
      } | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $workspace "output/manifest.json") -Encoding UTF8
    }
    "term-04-mini-pipeline" {
      New-Item -ItemType Directory -Force -Path (Join-Path $workspace "output") | Out-Null
      @(
        "region,orders,gross,net",
        "east,2,30.00,27.00",
        "west,1,25.00,22.75"
      ) | Set-Content (Join-Path $workspace "output/by_region.csv") -Encoding UTF8
      @{ orders = 3; gross = 55.00; net = 49.75 } | ConvertTo-Json | Set-Content (Join-Path $workspace "output/total.json") -Encoding UTF8
    }
    "swe-01-fix-off-by-one" {
      @'
export function sumRange(start, end) {
  if (end < start) return 0;
  let total = 0;
  for (let i = start; i <= end; i += 1) {
    total += i;
  }
  return total;
}
'@ | Set-Content (Join-Path $workspace "src/sumRange.js") -Encoding UTF8
    }
    "swe-02-add-feature" {
      @'
export function slugify(input) {
  return String(input)
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function truncate(str, maxLen) {
  const s = String(str);
  if (maxLen < 1) return "";
  if (s.length <= maxLen) return s;
  const ellipsis = "\u2026";
  if (maxLen === 1) return ellipsis;
  return s.slice(0, maxLen - 1) + ellipsis;
}

export function countWords(str) {
  const s = String(str).trim();
  if (!s) return 0;
  return s.split(/\s+/).length;
}
'@ | Set-Content (Join-Path $workspace "src/textkit.js") -Encoding UTF8
    }
    "swe-03-refactor-api" {
      @'
function round2(n) {
  return Math.round((n + Number.EPSILON) * 100) / 100;
}

export function lineTotal(item) {
  return item.price * (item.qty ?? 1);
}

export function calcTotal(items) {
  return items.reduce((sum, item) => sum + lineTotal(item), 0);
}

export function calcTotalV2(items, options = {}) {
  const taxRate = options.taxRate ?? 0;
  const subtotal = round2(items.reduce((sum, item) => sum + lineTotal(item), 0));
  const tax = round2(subtotal * taxRate);
  const total = round2(subtotal + tax);
  return { subtotal, tax, total };
}
'@ | Set-Content (Join-Path $workspace "src/price.js") -Encoding UTF8
    }
    "swe-04-bug-and-regression" {
      @'
export class Cache {
  constructor(options = {}) {
    this.ttlMs = options.ttlMs ?? 1000;
    this.now = options.now ?? (() => Date.now());
    this.store = new Map();
  }

  set(key, value) {
    this.store.set(key, { value, expiresAt: this.now() + this.ttlMs });
  }

  get(key) {
    const hit = this.store.get(key);
    if (!hit) return undefined;
    if (this.now() >= hit.expiresAt) {
      this.store.delete(key);
      return undefined;
    }
    return hit.value;
  }
}
'@ | Set-Content (Join-Path $workspace "src/cache.js") -Encoding UTF8
      @'
import test from "node:test";
import assert from "node:assert/strict";
import { Cache } from "../src/cache.js";

test("basic set/get", () => {
  let t = 1000;
  const c = new Cache({ ttlMs: 100, now: () => t });
  c.set("a", 1);
  assert.equal(c.get("a"), 1);
});

test("expires and returns undefined (regression)", () => {
  let t = 1000;
  const c = new Cache({ ttlMs: 100, now: () => t });
  c.set("a", 1);
  t = 1100;
  assert.equal(c.get("a"), undefined);
  // second get still undefined
  assert.equal(c.get("a"), undefined);
});
'@ | Set-Content (Join-Path $workspace "test/cache.test.mjs") -Encoding UTF8
    }
   default { throw "No gold solution for $id" }
 }
}

function Action-SelfTest {
Ensure-Dirs
 if (Test-Path $AttemptsPath) { Remove-Item -Force $AttemptsPath }
 $suite = Read-Suite
 $failed = @()
 foreach ($t in $suite.tasks) {
   $ws = Init-TaskWorkspace $t.id
   Install-GoldSolution -id $t.id -workspace $ws
   $grade = Invoke-Grade -id $t.id -workspace $ws
   $pass = [int]$grade.pass
   Write-Host ("selftest {0}: pass={1} ({2})" -f $t.id, $pass, $grade.reason)
   if ($pass -ne 1) { $failed += $t.id }

   # also write as attempt 1 for summarize demo
   $record = [ordered]@{
     ts = (Get-Date).ToString("o")
     model = "gold-solution"
     harness = $Harness
     task_id = $t.id
     component = $t.component
     attempt = 1
     pass = $pass
     reason = [string]$grade.reason
     wall_time_sec = 0
     notes = "selftest gold"
   }
   Write-Attempt $record
 }

 Action-Summarize
 if ($failed.Count -gt 0) {
   throw "Selftest failed for: $($failed -join ', ')"
 }
 Write-Host "Selftest passed for all $($suite.tasks.Count) tasks."
}

function Action-Status {
  $suite = Read-Suite
  Ensure-Dirs
  Write-Host "Suite: $($suite.name) v$($suite.version)"
  Write-Host "RepoRoot: $RepoRoot"
  Write-Host "Tasks: $($suite.tasks.Count)"
  Write-Host "Workspaces: $WorkRoot"
  Write-Host "Results: $ResultRoot"
  if (Test-Path $AttemptsPath) {
    $n = @(Get-Content -Encoding UTF8 $AttemptsPath | Where-Object { $_.Trim() -ne "" }).Count
    Write-Host "Recorded attempts: $n"
  } else {
    Write-Host "Recorded attempts: 0"
  }
  Write-Host ""
  Write-Host "Suggested flow for IDE Grok 4.5 scoring:"
  Write-Host "  1) powershell -File benchmarks/aa-aligned/runner.ps1 -Action init"
  Write-Host "  2) In CN-Codex, set model=Grok 4.5 (high), open workspaces/<task>/PROMPT.md"
  Write-Host "  3) After agent finishes: -Action grade -TaskId <id> -Attempt 1"
  Write-Host "  4) Repeat attempts 2/3, then -Action summarize"
}

switch ($Action) {
  "init" { Action-Init }
  "grade" { Action-Grade }
  "summarize" { Action-Summarize }
  "selftest" { Action-SelfTest }
  "status" { Action-Status }
}
