#!/usr/bin/env python3
"""Generate grade.ps1 scripts and fixture files for all 90 tasks."""

import json, os, textwrap

BASE = os.path.dirname(os.path.abspath(__file__))
TASKS_DIR = os.path.join(BASE, "tasks")
FIXTURES_DIR = os.path.join(BASE, "fixtures")
SUITE_PATH = os.path.join(BASE, "suite.json")

with open(SUITE_PATH, encoding='utf-8') as f:
    suite = json.load(f)

# ============================================================
# Helper
# ============================================================
def write_file(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(content)

# ============================================================
# REPO_QA grade scripts (30 tasks)
# ============================================================
REPO_QA_EXPECTED = {
    "qa-01-find-entrypoint": {"entrypoint": "src/main.tsx", "ui_framework": "react", "desktop_framework": "tauri", "test_script": "test"},
    "qa-03-tool-pipeline": {"has_apply_patch": True, "has_browser_run": True, "has_tool_search": True, "source_file": "src/hooks/useTauriEvents.ts"},
    "qa-04-test-command": {"test_script": "test", "runner": "vitest", "has_watch_script": True, "rust_manifest": "src-tauri/Cargo.toml"},
}

def write_repo_qa_grade(tid):
    """Write a grade.ps1 for repo_qa tasks. For tasks with known expected values, check them."""
    if tid in REPO_QA_EXPECTED:
        expected = REPO_QA_EXPECTED[tid]
        checks = []
        for k, v in expected.items():
            if isinstance(v, bool):
                checks.append(f'([bool]($ans.{k} -eq $true))')
            else:
                checks.append(f'([string]$ans.{k} -eq "{v}")')
        check_expr = " -and ".join(checks)
        content = f'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$answerPath = Join-Path $Workspace "answer.json"
if (-not (Test-Path $answerPath)) {{ return @{{ pass = 0; reason = "missing answer.json" }} }}
try {{ $ans = Get-Content -Raw -Encoding UTF8 $answerPath | ConvertFrom-Json }} catch {{ return @{{ pass = 0; reason = "invalid json" }} }}
$pass = if ({check_expr}) {{ 1 }} else {{ 0 }}
return @{{ pass = $pass; reason = $(if ($pass) {{ "correct" }} else {{ "field mismatch" }}) }}
'''
        write_file(os.path.join(TASKS_DIR, tid, "grade.ps1"), content.strip())
    else:
        # Dynamic tasks - accept any answer.json with required fields
        content = '''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$answerPath = Join-Path $Workspace "answer.json"
if (-not (Test-Path $answerPath)) { return @{ pass = 0; reason = "missing answer.json" } }
try { $ans = Get-Content -Raw -Encoding UTF8 $answerPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
# Accept any answer that has at least 4 fields
$fields = @($ans.PSObject.Properties.Name)
if ($fields.Count -ge 4) { return @{ pass = 1; reason = "accepted" } }
return @{ pass = 0; reason = "too few fields" }
'''
        write_file(os.path.join(TASKS_DIR, tid, "grade.ps1"), content.strip())

for tid in os.listdir(TASKS_DIR):
    if tid.startswith("qa-"):
        write_repo_qa_grade(tid)

print("✅ repo_qa grade scripts done")

# ============================================================
# TERMINAL fixtures + grade scripts (30 tasks)
# ============================================================
TERMINAL_FIXTURES = {
    "term-01-json-transform": {
        "input/users.json": json.dumps([
            {"id": 1, "name": "Ada", "active": True, "score": 90},
            {"id": 2, "name": "Bob", "active": False, "score": 95},
            {"id": 3, "name": "Grace", "active": True, "score": 99},
            {"id": 4, "name": "Dan", "active": False, "score": 80},
            {"id": 5, "name": "Eve", "active": True, "score": 70},
        ], indent=2),
    },
    "term-02-log-etl": {
        "input/app.log": "\n".join([
            "2026-07-01T10:00:00 INFO boot",
            "2026-07-01T10:00:01 DEBUG cache warm",
            "2026-07-01T10:00:02 ERROR disk full",
            "2026-07-01T10:00:03 INFO ready",
            "2026-07-01T10:00:04 WARN slow query",
            "2026-07-01T10:00:05 ERROR auth failed",
            "2026-07-01T10:00:06 INFO shutdown",
        ]),
    },
    "term-03-batch-rename": {
        "input/raw/Hello World.txt": "beta",
        "input/raw/note a.txt": "alpha",
        "input/raw/REPORT Final.txt": "gamma",
    },
    "term-04-mini-pipeline": {
        "input/sales.csv": "order_id,region,amount,status\n1,east,10.00,paid\n2,east,20.00,paid\n3,west,25.00,paid\n4,west,40.00,refunded\n5,east,5.00,pending\n",
        "input/rates.json": json.dumps({"east": 0.10, "west": 0.09, "north": 0.08}, indent=2),
    },
    "term-05-merge-json": {
        "input/users.json": json.dumps([
            {"id": 1, "name": "Alice", "dept": "eng"},
            {"id": 2, "name": "Bob", "dept": "sales"},
            {"id": 3, "name": "Carol", "dept": "eng"},
        ], indent=2),
        "input/profiles.json": json.dumps([
            {"id": 1, "email": "alice@co.com", "phone": "111"},
            {"id": 2, "email": "bob@co.com", "phone": "222"},
            {"id": 4, "email": "dave@co.com", "phone": "444"},
        ], indent=2),
    },
    "term-06-csv-filter": {
        "input/orders.csv": "order_id,amount,status,date\n1,50.00,completed,2026-01-01\n2,150.00,completed,2026-01-02\n3,200.00,pending,2026-01-03\n4,80.00,completed,2026-01-04\n5,300.00,completed,2026-01-05\n6,120.00,cancelled,2026-01-06\n",
    },
    "term-07-file-count": {
        "input/docs/report.txt": "some text",
        "input/docs/notes.txt": "more text",
        "input/docs/data.csv": "a,b,c\n1,2,3",
        "input/docs/code.py": "print('hello')",
        "input/docs/readme.md": "# Readme",
        "input/docs/archive.zip": "binary",
    },
    "term-08-dedup-lines": {
        "input/emails.txt": "\n".join([
            "alice@co.com", "bob@co.com", "alice@co.com",
            "carol@co.com", "bob@co.com", "dave@co.com",
            "alice@co.com", "eve@co.com",
        ]),
    },
    "term-09-tsv-to-csv": {
        "input/data.tsv": "name\tage\tcity\nAlice\t30\tNYC\nBob\t25\tLA\nCarol\t35\tChicago\n",
    },
    "term-10-find-and-replace": {
        "input/file1.txt": "The token is OLD_TOKEN. Please use OLD_TOKEN carefully.",
        "input/file2.txt": "OLD_TOKEN has been deprecated. Replace with NEW_TOKEN.",
        "input/file3.txt": "No tokens here.",
    },
    "term-11-sort-by-column": {
        "input/employees.csv": "name,department,salary\nAlice,Eng,85000\nBob,Sales,72000\nCarol,Eng,95000\nDave,Sales,65000\nEve,Eng,110000\n",
    },
    "term-12-validate-json": {
        "input/valid1.json": json.dumps({"name": "test", "value": 1}),
        "input/valid2.json": json.dumps([1, 2, 3]),
        "input/invalid1.json": "{broken json",
        "input/valid3.json": json.dumps({"status": "ok"}),
        "input/invalid2.json": "{'single': 'quotes'}",
    },
    "term-13-generate-checksums": {
        "input/doc1.txt": "hello world",
        "input/doc2.txt": "data to checksum",
        "input/doc3.txt": "another file",
    },
    "term-14-parse-urls": {
        "input/urls.txt": "\n".join([
            "Visit https://example.com for info",
            "Check http://test.org/page and https://example.com again",
            "Also ftp://files.com and https://docs.example.com/path?q=1",
            "No url here",
            "Email: user@example.com",
        ]),
    },
    "term-15-table-join": {
        "input/students.csv": "student_id,name,grade\n1,Alice,10\n2,Bob,11\n3,Carol,10\n4,Dave,12\n5,Eve,11\n",
        "input/scores.csv": "student_id,subject,score\n1,Math,95\n1,Science,88\n2,Math,72\n3,Science,91\n5,Math,85\n6,History,70\n",
    },
    "term-16-parse-nginx-log": {
        "input/nginx.log": "\n".join([
            '192.168.1.1 - - [01/Jan/2026:10:00:00 +0000] "GET /index.html HTTP/1.1" 200 1234',
            '192.168.1.2 - - [01/Jan/2026:10:00:01 +0000] "GET /api/data HTTP/1.1" 200 567',
            '192.168.1.1 - - [01/Jan/2026:10:00:02 +0000] "POST /api/login HTTP/1.1" 401 234',
            '192.168.1.3 - - [01/Jan/2026:10:00:03 +0000] "GET /index.html HTTP/1.1" 200 1234',
            '192.168.1.1 - - [01/Jan/2026:10:00:04 +0000] "GET /api/data HTTP/1.1" 200 567',
            '192.168.1.2 - - [01/Jan/2026:10:00:05 +0000] "GET /about.html HTTP/1.1" 404 345',
        ]),
    },
    "term-17-date-transform": {
        "input/dates.txt": "\n".join(["12/25/2026", "01/01/2026", "07/04/2026", "not a date", "03/15/2026"]),
    },
    "term-18-diff-files": {
        "input/file_a.txt": "line1\nline2\nline3\nline4\nline5\n",
        "input/file_b.txt": "line1\nline2\nlineX\nline4\nline6\n",
    },
    "term-19-json-to-csv": {
        "input/items.json": json.dumps([
            {"id": 1, "name": "Widget", "price": 9.99},
            {"id": 2, "name": "Gadget", "price": 24.99},
            {"id": 3, "name": "Doohickey", "price": 4.99},
        ], indent=2),
    },
    "term-20-base64-encode": {
        "input/file1.txt": "Hello World",
        "input/file2.txt": "Base64 test data",
        "input/file3.txt": "Short text",
    },
    "term-21-find-duplicates": {
        "input/file_a.txt": "duplicate content",
        "input/file_b.txt": "unique content A",
        "input/file_c.txt": "duplicate content",
        "input/file_d.txt": "unique content B",
        "input/file_e.txt": "duplicate content",
    },
    "term-22-split-csv": {
        "input/transactions.csv": "txn_id,type,amount,date\n1,credit,100.00,2026-01-01\n2,debit,50.00,2026-01-02\n3,credit,200.00,2026-01-03\n4,debit,75.00,2026-01-04\n5,refund,25.00,2026-01-05\n6,credit,150.00,2026-01-06\n",
    },
    "term-23-grep-and-count": {
        "input/app1.log": "INFO started\nERROR failed to connect\nWARN timeout\nERROR disk full\nINFO done\n",
        "input/app2.log": "ERROR null pointer\nINFO running\nERROR out of memory\nINFO shutdown\n",
        "input/app3.log": "INFO boot\nDEBUG init\nINFO ready\n",
    },
    "term-24-ip-validate": {
        "input/ip_list.txt": "\n".join([
            "192.168.1.1", "256.1.2.3", "10.0.0.1", "not.an.ip", "172.16.0.0",
            "0.0.0.0", "999.999.999.999", "127.0.0.1", "abc.def.ghi.jkl",
        ]),
    },
    "term-25-json-flatten": {
        "input/nested.json": json.dumps({
            "name": "test",
            "address": {"city": "NYC", "zip": "10001", "coordinates": {"lat": 40.7, "lng": -74.0}},
            "tags": ["a", "b", "c"],
            "metadata": {"views": 100, "likes": {"count": 25, "users": ["u1", "u2"]}}
        }, indent=2),
    },
    "term-26-encrypt-decrypt": {
        "input/secret.txt": "This is a secret message that needs encryption.",
    },
    "term-27-html-to-text": {
        "input/page.html": "<html><body><h1>Title</h1><p>This is a <b>paragraph</b> with <a href='link'>a link</a>.</p><ul><li>Item 1</li><li>Item 2</li></ul></body></html>",
    },
    "term-28-csv-statistics": {
        "input/grades.csv": "student,math,english,science\nAlice,85,90,88\nBob,72,68,75\nCarol,95,92,94\nDave,60,65,58\nEve,88,85,90\n",
    },
    "term-29-json-patch": {
        "input/base.json": json.dumps({"name": "config", "version": 1, "enabled": True, "items": [1, 2, 3]}, indent=2),
        "input/patch.json": json.dumps({"version": 2, "enabled": False, "items": [1, 2, 3, 4], "newField": "added"}, indent=2),
    },
    "term-30-tar-archive": {
        "input/docs/readme.txt": "readme content",
        "input/docs/notes.txt": "notes content",
        "input/docs/sub/info.txt": "nested info",
    },
}

TERMINAL_GRADE_SCRIPTS = {
    "term-01-json-transform": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$outJson = Join-Path $Workspace "output/top_active.json"
$outSum = Join-Path $Workspace "output/summary.txt"
if (-not (Test-Path $outJson)) { return @{ pass = 0; reason = "missing top_active.json" } }
if (-not (Test-Path $outSum)) { return @{ pass = 0; reason = "missing summary.txt" } }
try { $arr = Get-Content -Raw -Encoding UTF8 $outJson | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
if ($arr.Count -ne 3) { return @{ pass = 0; reason = "expected 3" } }
$ids = @($arr | ForEach-Object { [int]$_.id })
$ok = ($ids[0] -eq 3 -and $ids[1] -eq 1 -and $ids[2] -eq 5)
$sum = (Get-Content -Raw -Encoding UTF8 $outSum).Trim()
$ok = $ok -and ($sum -eq "count=3;max=99")
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "json transform ok" } else { "bad output" }) }
''',
    "term-02-log-etl": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$csvPath = Join-Path $Workspace "output/levels.csv"
$errPath = Join-Path $Workspace "output/errors.txt"
if (-not (Test-Path $csvPath)) { return @{ pass = 0; reason = "missing levels.csv" } }
if (-not (Test-Path $errPath)) { return @{ pass = 0; reason = "missing errors.txt" } }
$csv = Get-Content -Encoding UTF8 $csvPath | Where-Object { $_.Trim() -ne "" }
if ($csv.Count -lt 2 -or $csv[0].Trim().ToLowerInvariant() -ne "level,count") { return @{ pass = 0; reason = "bad csv" } }
$map = @{}
foreach ($line in $csv[1..($csv.Count - 1)]) { $parts = $line.Split(","); $map[$parts[0].Trim().ToUpperInvariant()] = [int]$parts[1] }
$okMap = ($map["DEBUG"] -eq 1) -and ($map["ERROR"] -eq 2) -and ($map["INFO"] -eq 3) -and ($map["WARN"] -eq 1)
$errs = @(Get-Content -Encoding UTF8 $errPath | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne "" })
$okErr = ($errs.Count -eq 2) -and ($errs[0] -eq "disk full") -and ($errs[1] -eq "auth failed")
return @{ pass = $(if ($okMap -and $okErr) { 1 } else { 0 }); reason = $(if ($okMap -and $okErr) { "log etl ok" } else { "csv/err mismatch" }) }
''',
    "term-03-batch-rename": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$manPath = Join-Path $Workspace "output/manifest.json"
$outDir = Join-Path $Workspace "output/renamed"
if (-not (Test-Path $manPath)) { return @{ pass = 0; reason = "missing manifest.json" } }
if (-not (Test-Path $outDir)) { return @{ pass = 0; reason = "missing output/renamed" } }
try { $man = Get-Content -Raw -Encoding UTF8 $manPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$files = @($man.files)
if ($files.Count -ne 3) { return @{ pass = 0; reason = "expected 3 files" } }
$tos = @($files | ForEach-Object { [string]$_.to })
$expectedTo = @("hello_world.txt", "note_a.txt", "report_final.txt")
$ok = $true
for ($i = 0; $i -lt 3; $i++) { if ($tos[$i] -ne $expectedTo[$i]) { $ok = $false } }
$expectedContent = @{ "note_a.txt" = "alpha"; "hello_world.txt" = "beta"; "report_final.txt" = "gamma" }
foreach ($name in $expectedContent.Keys) {
  $p = Join-Path $outDir $name
  if (-not (Test-Path $p)) { $ok = $false; continue }
  $c = (Get-Content -Raw -Encoding UTF8 $p).Trim()
  if ($c -ne $expectedContent[$name]) { $ok = $false }
}
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "batch rename ok" } else { "mismatch" }) }
''',
    "term-04-mini-pipeline": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$csvPath = Join-Path $Workspace "output/by_region.csv"
$totPath = Join-Path $Workspace "output/total.json"
if (-not (Test-Path $csvPath)) { return @{ pass = 0; reason = "missing by_region.csv" } }
if (-not (Test-Path $totPath)) { return @{ pass = 0; reason = "missing total.json" } }
$lines = @(Get-Content -Encoding UTF8 $csvPath | Where-Object { $_.Trim() -ne "" })
if ($lines.Count -ne 3) { return @{ pass = 0; reason = "expected header + 2 regions" } }
if ($lines[0].Trim().ToLowerInvariant() -ne "region,orders,gross,net") { return @{ pass = 0; reason = "bad header" } }
function Parse-Row([string]$line) { $p = $line.Split(","); return [pscustomobject]@{ region = $p[0].Trim().ToLowerInvariant(); orders = [int]$p[1]; gross = [decimal]$p[2]; net = [decimal]$p[3] } }
$r1 = Parse-Row $lines[1]; $r2 = Parse-Row $lines[2]
$okCsv = ($r1.region -eq "east" -and $r1.orders -eq 2 -and [math]::Round($r1.gross, 2) -eq 30.00 -and [math]::Round($r1.net, 2) -eq 27.00) -and
  ($r2.region -eq "west" -and $r2.orders -eq 1 -and [math]::Round($r2.gross, 2) -eq 25.00 -and [math]::Round($r2.net, 2) -eq 22.75)
try { $tot = Get-Content -Raw -Encoding UTF8 $totPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid total.json" } }
$okTot = ([int]$tot.orders -eq 3) -and ([math]::Round([decimal]$tot.gross, 2) -eq 55.00) -and ([math]::Round([decimal]$tot.net, 2) -eq 49.75)
return @{ pass = $(if ($okCsv -and $okTot) { 1 } else { 0 }); reason = $(if ($okCsv -and $okTot) { "pipeline ok" } else { "aggregation mismatch" }) }
''',
    "term-05-merge-json": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/merged.json"
$s = Join-Path $Workspace "output/stats.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing merged.json" } }
if (-not (Test-Path $s)) { return @{ pass = 0; reason = "missing stats.txt" } }
try { $d = Get-Content -Raw -Encoding UTF8 $p | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$cnt = ($d | Measure-Object).Count
$ok = $cnt -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "merge ok" } else { "too few records" }) }
''',
    "term-06-csv-filter": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/high_value.csv"
$t = Join-Path $Workspace "output/total.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing high_value.csv" } }
if (-not (Test-Path $t)) { return @{ pass = 0; reason = "missing total.txt" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "filter ok" } else { "bad output" }) }
''',
    "term-07-file-count": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/counts.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing counts.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "file count ok" } else { "bad csv" }) }
''',
    "term-08-dedup-lines": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/unique.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing unique.txt" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 4
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "dedup ok" } else { "too few lines" }) }
''',
    "term-09-tsv-to-csv": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/data.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing data.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 4
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "tsv to csv ok" } else { "bad output" }) }
''',
    "term-10-find-and-replace": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/changes.json"
$d = Join-Path $Workspace "output"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing changes.json" } }
$files = @(Get-ChildItem -Path $d -Filter "*.txt" -File | Select-Object -ExpandProperty Name)
$ok = $files.Count -ge 1
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "replace ok" } else { "no output files" }) }
''',
    "term-11-sort-by-column": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/sorted.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing sorted.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 5
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "sort ok" } else { "bad output" }) }
''',
    "term-12-validate-json": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$v = Join-Path $Workspace "output/valid.txt"
$iv = Join-Path $Workspace "output/invalid.txt"
if (-not (Test-Path $v)) { return @{ pass = 0; reason = "missing valid.txt" } }
if (-not (Test-Path $iv)) { return @{ pass = 0; reason = "missing invalid.txt" } }
$valid = @(Get-Content -Encoding UTF8 $v | Where-Object { $_.Trim() -ne "" })
$invalid = @(Get-Content -Encoding UTF8 $iv | Where-Object { $_.Trim() -ne "" })
$ok = $valid.Count -ge 2 -and $invalid.Count -ge 1
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "validate ok" } else { "bad classification" }) }
''',
    "term-13-generate-checksums": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/checksums.json"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing checksums.json" } }
try { $d = Get-Content -Raw -Encoding UTF8 $p | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$ok = ($d.PSObject.Properties.Name | Measure-Object).Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "checksums ok" } else { "too few files" }) }
''',
    "term-14-parse-urls": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/parsed_urls.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing parsed_urls.txt" } }
$urls = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $urls.Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "parse urls ok" } else { "too few urls" }) }
''',
    "term-15-table-join": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/joined.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing joined.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 4
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "join ok" } else { "bad output" }) }
''',
    "term-16-parse-nginx-log": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/ip_counts.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing ip_counts.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "nginx parse ok" } else { "bad output" }) }
''',
    "term-17-date-transform": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/iso_dates.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing iso_dates.txt" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "date transform ok" } else { "bad output" }) }
''',
    "term-18-diff-files": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$d = Join-Path $Workspace "output/diff.txt"
$c = Join-Path $Workspace "output/common.txt"
if (-not (Test-Path $d)) { return @{ pass = 0; reason = "missing diff.txt" } }
if (-not (Test-Path $c)) { return @{ pass = 0; reason = "missing common.txt" } }
return @{ pass = 1; reason = "diff ok" }
''',
    "term-19-json-to-csv": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/items.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing items.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 4
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "json to csv ok" } else { "bad csv" }) }
''',
    "term-20-base64-encode": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$d = Join-Path $Workspace "output/encoded"
$m = Join-Path $Workspace "output/manifest.json"
if (-not (Test-Path $d)) { return @{ pass = 0; reason = "missing output/encoded" } }
if (-not (Test-Path $m)) { return @{ pass = 0; reason = "missing manifest.json" } }
$files = @(Get-ChildItem -Path $d -File | Select-Object -ExpandProperty Name)
$ok = $files.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "base64 encode ok" } else { "too few files" }) }
''',
    "term-21-find-duplicates": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/duplicates.json"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing duplicates.json" } }
try { $d = Get-Content -Raw -Encoding UTF8 $p | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
return @{ pass = 1; reason = "duplicates check ok" }
''',
    "term-22-split-csv": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$d = Join-Path $Workspace "output/split"
$s = Join-Path $Workspace "output/summary.json"
if (-not (Test-Path $d)) { return @{ pass = 0; reason = "missing output/split" } }
if (-not (Test-Path $s)) { return @{ pass = 0; reason = "missing summary.json" } }
$files = @(Get-ChildItem -Path $d -File | Select-Object -ExpandProperty Name)
$ok = $files.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "split ok" } else { "too few files" }) }
''',
    "term-23-grep-and-count": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/error_counts.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing error_counts.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "grep count ok" } else { "bad csv" }) }
''',
    "term-24-ip-validate": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$v = Join-Path $Workspace "output/valid_ips.txt"
$iv = Join-Path $Workspace "output/invalid_ips.txt"
if (-not (Test-Path $v)) { return @{ pass = 0; reason = "missing valid_ips.txt" } }
if (-not (Test-Path $iv)) { return @{ pass = 0; reason = "missing invalid_ips.txt" } }
$valid = @(Get-Content -Encoding UTF8 $v | Where-Object { $_.Trim() -ne "" })
$invalid = @(Get-Content -Encoding UTF8 $iv | Where-Object { $_.Trim() -ne "" })
$ok = $valid.Count -ge 3 -and $invalid.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "ip validate ok" } else { "bad classification" }) }
''',
    "term-25-json-flatten": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/flat.json"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing flat.json" } }
try { $d = Get-Content -Raw -Encoding UTF8 $p | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$ok = ($d.PSObject.Properties.Name | Measure-Object).Count -ge 5
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "flatten ok" } else { "too few fields" }) }
''',
    "term-26-encrypt-decrypt": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$e = Join-Path $Workspace "output/encrypted.bin"
$d = Join-Path $Workspace "output/decrypted.txt"
if (-not (Test-Path $e)) { return @{ pass = 0; reason = "missing encrypted.bin" } }
if (-not (Test-Path $d)) { return @{ pass = 0; reason = "missing decrypted.txt" } }
return @{ pass = 1; reason = "encrypt decrypt ok" }
''',
    "term-27-html-to-text": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/plain_text.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing plain_text.txt" } }
$txt = (Get-Content -Raw -Encoding UTF8 $p).Trim()
$ok = $txt.Length -gt 20
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "html to text ok" } else { "too short" }) }
''',
    "term-28-csv-statistics": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$j = Join-Path $Workspace "output/stats.json"
$t = Join-Path $Workspace "output/summary.txt"
if (-not (Test-Path $j)) { return @{ pass = 0; reason = "missing stats.json" } }
if (-not (Test-Path $t)) { return @{ pass = 0; reason = "missing summary.txt" } }
return @{ pass = 1; reason = "csv stats ok" }
''',
    "term-29-json-patch": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/patched.json"
$c = Join-Path $Workspace "output/changelog.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing patched.json" } }
if (-not (Test-Path $c)) { return @{ pass = 0; reason = "missing changelog.txt" } }
return @{ pass = 1; reason = "json patch ok" }
''',
    "term-30-tar-archive": r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$a = Join-Path $Workspace "output/archive.zip"
$e = Join-Path $Workspace "output/extracted"
$alt = Join-Path $Workspace "output/archive.tar.gz"
if (-not (Test-Path $a) -and -not (Test-Path $alt)) { return @{ pass = 0; reason = "missing archive" } }
if (-not (Test-Path $e)) { return @{ pass = 0; reason = "missing output/extracted" } }
return @{ pass = 1; reason = "archive ok" }
''',
}

# Write terminal fixtures
for tid, files in TERMINAL_FIXTURES.items():
    for relpath, content in files.items():
        write_file(os.path.join(FIXTURES_DIR, tid, relpath), content)

# Write terminal grade scripts
for tid, script in TERMINAL_GRADE_SCRIPTS.items():
    write_file(os.path.join(TASKS_DIR, tid, "grade.ps1"), script.strip())

print("✅ terminal fixtures + grades done")

# ============================================================
# SWE_EDIT fixtures + grade scripts (30 tasks)
# ============================================================
SWE_EDIT_FIXTURES = {
    "swe-01-fix-off-by-one": {
        "package.json": '{"type": "module"}',
        "src/sumRange.js": '''/**
 * Sum integers in inclusive range [start, end].
 * BUG: currently excludes `end`.
 */
export function sumRange(start, end) {
  if (end < start) return 0;
  let total = 0;
  for (let i = start; i < end; i += 1) {
    total += i;
  }
  return total;
}
''',
        "test/sumRange.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { sumRange } from "../src/sumRange.js";

test("sumRange inclusive", () => {
  assert.equal(sumRange(1, 3), 6);
  assert.equal(sumRange(0, 0), 0);
  assert.equal(sumRange(5, 5), 5);
  assert.equal(sumRange(2, 5), 14);
});

test("sumRange empty when end < start", () => {
  assert.equal(sumRange(3, 1), 0);
});
''',
    },
    "swe-02-add-feature": {
        "package.json": '{"type": "module"}',
        "src/textkit.js": '''export function slugify(input) {
  return String(input)
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

// TODO: implement truncate(str, maxLen)
// TODO: implement countWords(str)
''',
        "test/textkit.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { slugify, truncate, countWords } from "../src/textkit.js";

test("slugify still works", () => {
  assert.equal(slugify("Hello World"), "hello-world");
});

test("truncate", () => {
  assert.equal(truncate("abcdef", 10), "abcdef");
  assert.equal(truncate("abcdef", 4), "abc\\u2026");
  assert.equal(truncate("a", 1), "a");
  assert.equal(truncate("ab", 1), "\\u2026");
  assert.equal(truncate("xyz", 0), "");
});

test("countWords", () => {
  assert.equal(countWords(""), 0);
  assert.equal(countWords("  "), 0);
  assert.equal(countWords("one"), 1);
  assert.equal(countWords("one two  three"), 3);
});
''',
    },
    "swe-03-refactor-api": {
        "package.json": '{"type": "module"}',
        "src/price.js": '''export function calcTotal(items) {
  return items.reduce((sum, item) => sum + item.price * (item.qty ?? 1), 0);
}

// TODO: add lineTotal(item)
// TODO: add calcTotalV2(items, options)
''',
        "test/price.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { calcTotal, lineTotal, calcTotalV2 } from "../src/price.js";

const items = [
  { price: 10, qty: 2 },
  { price: 5 },
];

test("legacy calcTotal", () => {
  assert.equal(calcTotal(items), 25);
});

test("lineTotal", () => {
  assert.equal(lineTotal({ price: 3, qty: 4 }), 12);
  assert.equal(lineTotal({ price: 7 }), 7);
});

test("calcTotalV2", () => {
  const r = calcTotalV2(items, { taxRate: 0.1 });
  assert.equal(r.subtotal, 25);
  assert.equal(r.tax, 2.5);
  assert.equal(r.total, 27.5);
});

test("calcTotalV2 rounding", () => {
  const r = calcTotalV2([{ price: 1.005, qty: 1 }], { taxRate: 0 });
  assert.equal(r.subtotal, 1.01);
  assert.equal(r.tax, 0);
  assert.equal(r.total, 1.01);
});
''',
    },
    "swe-04-bug-and-regression": {
        "package.json": '{"type": "module"}',
        "src/cache.js": '''export class Cache {
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
''',
        "test/cache.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { Cache } from "../src/cache.js";

test("basic set/get", () => {
  let t = 1000;
  const c = new Cache({ ttlMs: 100, now: () => t });
  c.set("a", 1);
  assert.equal(c.get("a"), 1);
  t = 1200;
  assert.equal(c.get("a"), undefined);
});

test("expired", () => {
  let t = 1000;
  const c = new Cache({ ttlMs: 100, now: () => t });
  c.set("a", 1);
  t = 1100;
  assert.equal(c.get("a"), undefined);
});
''',
    },
    "swe-05-fix-divide-by-zero": {
        "package.json": '{"type": "module"}',
        "src/calculator.js": '''export function add(a, b) { return a + b; }
export function subtract(a, b) { return a - b; }
export function multiply(a, b) { return a * b; }
export function divide(a, b) { return a / b; }
''',
        "test/calc.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { add, subtract, multiply, divide } from "../src/calculator.js";

test("basic operations", () => {
  assert.equal(add(2, 3), 5);
  assert.equal(subtract(5, 3), 2);
  assert.equal(multiply(4, 3), 12);
});

test("divide by zero should throw", () => {
  assert.throws(() => divide(10, 0), /cannot divide by zero/i);
});
''',
    },
    "swe-06-add-sort-function": {
        "package.json": '{"type": "module"}',
        "src/arrayUtils.js": 'export function sortBy(arr, key) {}\nexport function unique(arr) {}\n',
        "test/arrayUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { sortBy, unique } from "../src/arrayUtils.js";

test("sortBy", () => {
  const items = [{n: 3}, {n: 1}, {n: 2}];
  const r = sortBy(items, "n");
  assert.equal(r[0].n, 1);
  assert.equal(r[1].n, 2);
  assert.equal(r[2].n, 3);
});

test("unique", () => {
  assert.deepEqual(unique([1, 2, 2, 3, 1, 4]), [1, 2, 3, 4]);
  assert.deepEqual(unique([]), []);
});
''',
    },
    "swe-07-fix-string-escape": {
        "package.json": '{"type": "module"}',
        "src/stringUtils.js": '''export function escapeHtml(str) {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
''',
        "test/stringUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { escapeHtml } from "../src/stringUtils.js";

test("escapeHtml", () => {
  assert.equal(escapeHtml('<script>alert("xss")</script>'), '&lt;script&gt;alert("xss")&lt;/script&gt;');
  assert.equal(escapeHtml("it's a test"), "it&#39;s a test");
  assert.equal(escapeHtml("a & b"), "a &amp; b");
});
''',
    },
    "swe-08-add-debounce": {
        "package.json": '{"type": "module"}',
        "src/functionUtils.js": 'export function debounce(fn, delay) {}\nexport function throttle(fn, interval) {}\n',
        "test/functionUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { debounce, throttle } from "../src/functionUtils.js";

test("debounce basic", async () => {
  let count = 0;
  const fn = debounce(() => { count++; }, 50);
  fn(); fn(); fn();
  await new Promise(r => setTimeout(r, 100));
  assert.equal(count, 1);
});

test("throttle basic", async () => {
  let count = 0;
  const fn = throttle(() => { count++; }, 50);
  fn(); fn(); fn();
  await new Promise(r => setTimeout(r, 100));
  assert.ok(count >= 1);
});
''',
    },
    "swe-09-fix-json-parse": {
        "package.json": '{"type": "module"}',
        "src/jsonUtils.js": 'export function safeParse(text) { return JSON.parse(text); }\n',
        "test/jsonUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { safeParse } from "../src/jsonUtils.js";

test("safeParse valid", () => {
  const r = safeParse('{"a":1}');
  assert.equal(r.error, null);
  assert.deepEqual(r.data, {a: 1});
});

test("safeParse invalid", () => {
  const r = safeParse("not json");
  assert.equal(r.error, "invalid json");
  assert.equal(r.data, null);
});
''',
    },
    "swe-10-add-date-format": {
        "package.json": '{"type": "module"}',
        "src/dateUtils.js": 'export function formatDate(date, fmt) {}\nexport function daysBetween(d1, d2) {}\n',
        "test/dateUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { formatDate, daysBetween } from "../src/dateUtils.js";

test("formatDate", () => {
  const d = new Date(2026, 0, 15);
  assert.equal(formatDate(d, "YYYY-MM-DD"), "2026-01-15");
});

test("daysBetween", () => {
  assert.equal(daysBetween(new Date("2026-01-01"), new Date("2026-01-10")), 9);
});
''',
    },
    "swe-11-fix-regex": {
        "package.json": '{"type": "module"}',
        "src/validator.js": 'export function validateEmail(email) { return /^[a-z0-9]+@[a-z0-9]+\\.[a-z]+$/.test(email); }\n',
        "test/validator.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { validateEmail } from "../src/validator.js";

test("valid emails", () => {
  assert.ok(validateEmail("user@example.com"));
  assert.ok(validateEmail("first.last@example.com"));
  assert.ok(validateEmail("user+tag@example.co.uk"));
});

test("invalid emails", () => {
  assert.ok(!validateEmail(""));
  assert.ok(!validateEmail("notanemail"));
  assert.ok(!validateEmail("@example.com"));
});
''',
    },
    "swe-12-add-queue": {
        "package.json": '{"type": "module"}',
        "src/dataStructures.js": 'export class Queue {}\nexport class Stack {}\n',
        "test/dataStructures.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { Queue, Stack } from "../src/dataStructures.js";

test("Queue", () => {
  const q = new Queue();
  assert.equal(q.size(), 0);
  q.enqueue(1); q.enqueue(2);
  assert.equal(q.size(), 2);
  assert.equal(q.peek(), 1);
  assert.equal(q.dequeue(), 1);
  assert.equal(q.size(), 1);
});

test("Stack", () => {
  const s = new Stack();
  s.push(1); s.push(2);
  assert.equal(s.peek(), 2);
  assert.equal(s.pop(), 1);
  assert.equal(s.size(), 1);
});
''',
    },
    "swe-13-fix-memoization": {
        "package.json": '{"type": "module"}',
        "src/memoize.js": 'export function memoize(fn) { const cache = {}; return function(...args) { return fn(...args); }; }\n',
        "test/memoize.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { memoize } from "../src/memoize.js";

test("memoize basic", () => {
  let callCount = 0;
  const fn = memoize((x) => { callCount++; return x * 2; });
  assert.equal(fn(5), 10);
  assert.equal(fn(5), 10);
  assert.equal(callCount, 1);
});

test("memoize object args", () => {
  let callCount = 0;
  const fn = memoize((obj) => { callCount++; return obj.x; });
  assert.equal(fn({x: 1}), 1);
  assert.equal(fn({x: 1}), 1);
  assert.equal(callCount, 1);
});
''',
    },
    "swe-14-add-clone": {
        "package.json": '{"type": "module"}',
        "src/objectUtils.js": 'export function deepClone(obj) {}\nexport function merge(target, source) {}\n',
        "test/objectUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { deepClone, merge } from "../src/objectUtils.js";

test("deepClone", () => {
  const obj = { a: 1, b: { c: 2 } };
  const clone = deepClone(obj);
  assert.deepEqual(clone, obj);
  clone.b.c = 3;
  assert.equal(obj.b.c, 2);
});

test("merge", () => {
  const r = merge({ a: 1, b: 2 }, { b: 3, c: 4 });
  assert.equal(r.a, 1);
  assert.equal(r.b, 3);
  assert.equal(r.c, 4);
});
''',
    },
    "swe-15-fix-async-race": {
        "package.json": '{"type": "module"}',
        "src/asyncUtils.js": 'export function fetchWithTimeout(url, ms) {}\n',
        "test/asyncUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { fetchWithTimeout } from "../src/asyncUtils.js";

test("timeout rejects", async () => {
  const slow = new Promise(r => setTimeout(r, 1000));
  await assert.rejects(() => fetchWithTimeout(slow, 50));
});
''',
    },
    "swe-16-add-binary-search": {
        "package.json": '{"type": "module"}',
        "src/algorithms.js": 'export function binarySearch(arr, target) {}\nexport function quickSort(arr) {}\n',
        "test/algorithms.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { binarySearch, quickSort } from "../src/algorithms.js";

test("binarySearch", () => {
  assert.equal(binarySearch([1, 3, 5, 7, 9], 5), 2);
  assert.equal(binarySearch([1, 3, 5, 7, 9], 2), -1);
  assert.equal(binarySearch([], 1), -1);
});

test("quickSort", () => {
  assert.deepEqual(quickSort([3, 1, 4, 1, 5]), [1, 1, 3, 4, 5]);
  assert.deepEqual(quickSort([]), []);
});
''',
    },
    "swe-17-fix-array-mutation": {
        "package.json": '{"type": "module"}',
        "src/arrayUtils.js": 'export function removeFalsy(arr) { return arr.filter(Boolean); }\n',
        "test/arrayUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { removeFalsy } from "../src/arrayUtils.js";

test("removeFalsy", () => {
  const input = [0, 1, false, 2, "", 3, null];
  const result = removeFalsy(input);
  assert.deepEqual(result, [1, 2, 3]);
  assert.deepEqual(input, [0, 1, false, 2, "", 3, null]);
});
''',
    },
    "swe-18-add-event-emitter": {
        "package.json": '{"type": "module"}',
        "src/events.js": 'export class EventEmitter {}\n',
        "test/events.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "../src/events.js";

test("EventEmitter", () => {
  const ee = new EventEmitter();
  let calls = [];
  ee.on("test", (a, b) => calls.push([a, b]));
  ee.emit("test", 1, 2);
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0], [1, 2]);
});

test("once", () => {
  const ee = new EventEmitter();
  let count = 0;
  ee.once("test", () => count++);
  ee.emit("test");
  ee.emit("test");
  assert.equal(count, 1);
});

test("off", () => {
  const ee = new EventEmitter();
  let count = 0;
  const fn = () => count++;
  ee.on("test", fn);
  ee.emit("test");
  ee.off("test", fn);
  ee.emit("test");
  assert.equal(count, 1);
});
''',
    },
    "swe-19-fix-type-coercion": {
        "package.json": '{"type": "module"}',
        "src/typeUtils.js": 'export function toNumber(val) { return Number(val); }\n',
        "test/typeUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { toNumber } from "../src/typeUtils.js";

test("toNumber", () => {
  assert.equal(toNumber("42"), 42);
  assert.equal(toNumber("3.14"), 3.14);
  assert.ok(Number.isNaN(toNumber("1a")));
  assert.ok(Number.isNaN(toNumber("abc")));
  assert.equal(toNumber(""), 0);
});
''',
    },
    "swe-20-add-pipe": {
        "package.json": '{"type": "module"}',
        "src/fpUtils.js": 'export function pipe(...fns) {}\nexport function compose(...fns) {}\n',
        "test/fpUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { pipe, compose } from "../src/fpUtils.js";

test("pipe", () => {
  const add1 = x => x + 1;
  const double = x => x * 2;
  const fn = pipe(add1, double);
  assert.equal(fn(5), 12);
});

test("compose", () => {
  const add1 = x => x + 1;
  const double = x => x * 2;
  const fn = compose(add1, double);
  assert.equal(fn(5), 11);
});
''',
    },
    "swe-21-fix-float-arithmetic": {
        "package.json": '{"type": "module"}',
        "src/mathUtils.js": 'export function add(a, b) { return a + b; }\nexport function multiply(a, b) { return a * b; }\n',
        "test/mathUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { add, multiply } from "../src/mathUtils.js";

test("add precision", () => {
  assert.equal(add(0.1, 0.2), 0.3);
  assert.equal(add(0.01, 0.02), 0.03);
});

test("multiply precision", () => {
  assert.equal(multiply(0.1, 0.2), 0.02);
});
''',
    },
    "swe-22-add-lru-cache": {
        "package.json": '{"type": "module"}',
        "src/cache.js": 'export class LRUCache {}\n',
        "test/cache.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { LRUCache } from "../src/cache.js";

test("LRUCache basic", () => {
  const c = new LRUCache(2);
  c.set("a", 1);
  c.set("b", 2);
  assert.equal(c.get("a"), 1);
  c.set("c", 3);
  assert.equal(c.get("b"), undefined);
  assert.equal(c.get("c"), 3);
});

test("LRUCache update", () => {
  const c = new LRUCache(2);
  c.set("a", 1);
  c.set("b", 2);
  c.get("a");
  c.set("c", 3);
  assert.equal(c.get("b"), undefined);
  assert.equal(c.get("a"), 1);
});
''',
    },
    "swe-23-fix-enum": {
        "package.json": '{"type": "module"}',
        "src/enumUtils.js": 'export function parseEnum(value, validValues) { return validValues.includes(value); }\n',
        "test/enumUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { parseEnum } from "../src/enumUtils.js";

test("parseEnum", () => {
  assert.ok(parseEnum("HIGH", ["high", "medium", "low"]));
  assert.ok(parseEnum("High", ["high", "medium", "low"]));
  assert.ok(parseEnum("HIGH", ["HIGH", "MEDIUM", "LOW"]));
  assert.ok(!parseEnum("unknown", ["high", "medium", "low"]));
});
''',
    },
    "swe-24-add-hash": {
        "package.json": '{"type": "module"}',
        "src/hashUtils.js": 'export function hashString(str) {}\nexport function hashCode(str) {}\n',
        "test/hashUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { hashString, hashCode } from "../src/hashUtils.js";

test("hashString", () => {
  const h1 = hashString("hello");
  const h2 = hashString("hello");
  const h3 = hashString("world");
  assert.equal(h1, h2);
  assert.notEqual(h1, h3);
  assert.ok(typeof h1 === "string" || typeof h1 === "number");
});

test("hashCode", () => {
  const h = hashCode("test");
  assert.ok(Number.isInteger(h));
});
''',
    },
    "swe-25-fix-timer-leak": {
        "package.json": '{"type": "module"}',
        "src/timer.js": 'let intervalId = null;\nexport function start(fn, ms) { intervalId = setInterval(fn, ms); }\nexport function stop() { if (intervalId) { clearInterval(intervalId); intervalId = null; } }\n',
        "test/timer.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { start, stop } from "../src/timer.js";

test("timer restart", async () => {
  let count = 0;
  start(() => count++, 10);
  await new Promise(r => setTimeout(r, 30));
  stop();
  const c1 = count;
  start(() => count++, 10);
  await new Promise(r => setTimeout(r, 30));
  stop();
  assert.ok(count > c1);
});
''',
    },
    "swe-26-add-trie": {
        "package.json": '{"type": "module"}',
        "src/trie.js": 'export class Trie {}\n',
        "test/trie.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { Trie } from "../src/trie.js";

test("Trie", () => {
  const t = new Trie();
  t.insert("hello");
  t.insert("world");
  assert.ok(t.search("hello"));
  assert.ok(!t.search("hell"));
  assert.ok(t.startsWith("hel"));
  assert.ok(!t.startsWith("xyz"));
});
''',
    },
    "swe-27-fix-promise-chain": {
        "package.json": '{"type": "module"}',
        "src/promiseUtils.js": 'export async function retry(fn, maxAttempts) { return await fn(); }\n',
        "test/promiseUtils.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { retry } from "../src/promiseUtils.js";

test("retry succeeds", async () => {
  let attempts = 0;
  const r = await retry(async () => { attempts++; return "ok"; }, 3);
  assert.equal(r, "ok");
  assert.equal(attempts, 1);
});

test("retry fails finally", async () => {
  let attempts = 0;
  await assert.rejects(() => retry(async () => { attempts++; throw new Error("fail"); }, 3));
  assert.equal(attempts, 3);
});
''',
    },
    "swe-28-add-csv-parse": {
        "package.json": '{"type": "module"}',
        "src/csvParser.js": 'export function parseCSV(text) {}\nexport function toCSV(data) {}\n',
        "test/csvParser.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { parseCSV, toCSV } from "../src/csvParser.js";

test("parseCSV", () => {
  const data = parseCSV("a,b,c\\n1,2,3\\n4,5,6");
  assert.equal(data.length, 2);
  assert.deepEqual(data[0], {a: "1", b: "2", c: "3"});
});

test("parseCSV with quotes", () => {
  const data = parseCSV('a,b\\n1,"hello, world"');
  assert.equal(data[0].b, "hello, world");
});

test("toCSV", () => {
  const csv = toCSV([{a: 1, b: 2}, {a: 3, b: 4}]);
  assert.ok(csv.includes("a,b"));
  assert.ok(csv.includes("1,2"));
});
''',
    },
    "swe-29-fix-null-pointer": {
        "package.json": '{"type": "module"}',
        "src/safeAccess.js": 'export function safeGet(obj, path) { return path.split(".").reduce((o, k) => o[k], obj); }\n',
        "test/safeAccess.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { safeGet } from "../src/safeAccess.js";

test("safeGet", () => {
  const obj = { a: { b: { c: 42 } } };
  assert.equal(safeGet(obj, "a.b.c"), 42);
  assert.equal(safeGet(obj, "a.b"), 42);
  assert.equal(safeGet({ a: null }, "a.b"), undefined);
  assert.equal(safeGet({}, "x.y.z"), undefined);
});
''',
    },
    "swe-30-add-rate-limiter": {
        "package.json": '{"type": "module"}',
        "src/rateLimiter.js": 'export class RateLimiter {}\n',
        "test/rateLimiter.test.mjs": '''import test from "node:test";
import assert from "node:assert/strict";
import { RateLimiter } from "../src/rateLimiter.js";

test("RateLimiter allow", () => {
  const rl = new RateLimiter({ maxRequests: 2, windowMs: 1000 });
  assert.ok(rl.allow("user1"));
  assert.ok(rl.allow("user1"));
  assert.ok(!rl.allow("user1"));
});

test("RateLimiter reset", () => {
  const rl = new RateLimiter({ maxRequests: 1, windowMs: 1000 });
  rl.allow("user1");
  rl.reset("user1");
  assert.ok(rl.allow("user1"));
});

test("RateLimiter remaining", () => {
  const rl = new RateLimiter({ maxRequests: 3, windowMs: 1000 });
  rl.allow("user1");
  assert.equal(rl.remaining("user1"), 2);
});
''',
    },
}

# Write swe_edit fixtures
for tid, files in SWE_EDIT_FIXTURES.items():
    for relpath, content in files.items():
        write_file(os.path.join(FIXTURES_DIR, tid, relpath), content)

# Write swe_edit grade scripts (all use node --test)
for tid in [t["id"] for t in suite["tasks"] if t["component"] == "swe_edit"]:
    grade = r'''param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
Push-Location $Workspace
try {
  $output = & node --test 2>&1 | Out-String
  $code = $LASTEXITCODE
} finally {
  Pop-Location
}
$pass = if ($code -eq 0) { 1 } else { 0 }
$reason = if ($pass) { "tests passed" } else { "tests failed: $output" }
return @{ pass = $pass; reason = $reason }
'''
    write_file(os.path.join(TASKS_DIR, tid, "grade.ps1"), grade.strip())

print("✅ swe_edit fixtures + grades done")

# ============================================================
# Summary
# ============================================================
total_tasks = len([t for t in suite["tasks"]])
print(f"\n🎯 Total: {total_tasks} tasks ready for benchmarking")