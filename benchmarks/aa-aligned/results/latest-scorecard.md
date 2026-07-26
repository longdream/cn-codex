# CN-Codex AA-Aligned Scorecard

- Generated: 2026-07-26T06:52:11.1416773+08:00
- Model: **cn-codex-agent**
- Harness: **CN-Codex**
- Local Coding Agent Proxy Index: **1**
- AA reference Grok 4.5 @ Grok Build Coding Agent Index: **76**
- AA reference Grok 4.5 Intelligence Index: **54**

## Component pass@1

| Component | Aligns to | pass@1 |
|-----------|-----------|--------|
| repo_qa | SWE-Atlas-QnA | 1 |
| terminal | Terminal-Bench v2 | 1 |
| swe_edit | DeepSWE | 1 |

## Tasks

| Task | Component | Attempts | pass@1 | Status |
|------|-----------|----------|--------|--------|
| qa-01-find-entrypoint | repo_qa | 1 | 1 | scored |
| qa-02-config-provider | repo_qa | 1 | 1 | scored |
| qa-03-tool-pipeline | repo_qa | 1 | 1 | scored |
| qa-04-test-command | repo_qa | 1 | 1 | scored |
| qa-05-package-deps | repo_qa | 1 | 1 | scored |
| qa-06-routing-structure | repo_qa | 1 | 1 | scored |
| qa-07-i18n-setup | repo_qa | 1 | 1 | scored |
| qa-08-store-architecture | repo_qa | 1 | 1 | scored |
| qa-09-component-tree | repo_qa | 1 | 1 | scored |
| qa-10-api-layer | repo_qa | 1 | 1 | scored |
| qa-11-types-definitions | repo_qa | 1 | 1 | scored |
| qa-12-hooks-usage | repo_qa | 1 | 1 | scored |
| qa-13-styles-theme | repo_qa | 1 | 1 | scored |
| qa-14-build-tooling | repo_qa | 1 | 1 | scored |
| qa-15-tauri-config | repo_qa | 1 | 1 | scored |
| qa-16-git-branch | repo_qa | 1 | 1 | scored |
| qa-17-ci-config | repo_qa | 1 | 1 | scored |
| qa-18-error-handling | repo_qa | 1 | 1 | scored |
| qa-19-perf-optimization | repo_qa | 1 | 1 | scored |
| qa-20-security-patterns | repo_qa | 1 | 1 | scored |
| qa-21-plugin-system | repo_qa | 1 | 1 | scored |
| qa-22-mcp-integration | repo_qa | 1 | 1 | scored |
| qa-23-electron-vs-tauri | repo_qa | 1 | 1 | scored |
| qa-24-logging-system | repo_qa | 1 | 1 | scored |
| qa-25-test-coverage | repo_qa | 1 | 1 | scored |
| qa-26-utils-modules | repo_qa | 1 | 1 | scored |
| qa-27-markdown-rendering | repo_qa | 1 | 1 | scored |
| qa-28-file-attachment | repo_qa | 1 | 1 | scored |
| qa-29-keyboard-shortcuts | repo_qa | 1 | 1 | scored |
| qa-30-workspace-config | repo_qa | 1 | 1 | scored |
| term-01-json-transform | terminal | 1 | 1 | scored |
| term-02-log-etl | terminal | 1 | 1 | scored |
| term-03-batch-rename | terminal | 1 | 1 | scored |
| term-04-mini-pipeline | terminal | 1 | 1 | scored |
| term-05-merge-json | terminal | 1 | 1 | scored |
| term-06-csv-filter | terminal | 1 | 1 | scored |
| term-07-file-count | terminal | 1 | 1 | scored |
| term-08-dedup-lines | terminal | 1 | 1 | scored |
| term-09-tsv-to-csv | terminal | 1 | 1 | scored |
| term-10-find-and-replace | terminal | 1 | 1 | scored |
| term-11-sort-by-column | terminal | 1 | 1 | scored |
| term-12-validate-json | terminal | 1 | 1 | scored |
| term-13-generate-checksums | terminal | 1 | 1 | scored |
| term-14-parse-urls | terminal | 1 | 1 | scored |
| term-15-table-join | terminal | 1 | 1 | scored |
| term-16-parse-nginx-log | terminal | 1 | 1 | scored |
| term-17-date-transform | terminal | 1 | 1 | scored |
| term-18-diff-files | terminal | 1 | 1 | scored |
| term-19-json-to-csv | terminal | 1 | 1 | scored |
| term-20-base64-encode | terminal | 1 | 1 | scored |
| term-21-find-duplicates | terminal | 1 | 1 | scored |
| term-22-split-csv | terminal | 1 | 1 | scored |
| term-23-grep-and-count | terminal | 1 | 1 | scored |
| term-24-ip-validate | terminal | 1 | 1 | scored |
| term-25-json-flatten | terminal | 1 | 1 | scored |
| term-26-encrypt-decrypt | terminal | 1 | 1 | scored |
| term-27-html-to-text | terminal | 1 | 1 | scored |
| term-28-csv-statistics | terminal | 1 | 1 | scored |
| term-29-json-patch | terminal | 1 | 1 | scored |
| term-30-tar-archive | terminal | 1 | 1 | scored |
| swe-01-fix-off-by-one | swe_edit | 1 | 1 | scored |
| swe-02-add-feature | swe_edit | 1 | 1 | scored |
| swe-03-refactor-api | swe_edit | 1 | 1 | scored |
| swe-04-bug-and-regression | swe_edit | 1 | 1 | scored |
| swe-05-fix-divide-by-zero | swe_edit | 1 | 1 | scored |
| swe-06-add-sort-function | swe_edit | 1 | 1 | scored |
| swe-07-fix-string-escape | swe_edit | 1 | 1 | scored |
| swe-08-add-debounce | swe_edit | 1 | 1 | scored |
| swe-09-fix-json-parse | swe_edit | 1 | 1 | scored |
| swe-10-add-date-format | swe_edit | 1 | 1 | scored |
| swe-11-fix-regex | swe_edit | 1 | 1 | scored |
| swe-12-add-queue | swe_edit | 1 | 1 | scored |
| swe-13-fix-memoization | swe_edit | 1 | 1 | scored |
| swe-14-add-clone | swe_edit | 1 | 1 | scored |
| swe-15-fix-async-race | swe_edit | 1 | 1 | scored |
| swe-16-add-binary-search | swe_edit | 1 | 1 | scored |
| swe-17-fix-array-mutation | swe_edit | 1 | 1 | scored |
| swe-18-add-event-emitter | swe_edit | 1 | 1 | scored |
| swe-19-fix-type-coercion | swe_edit | 1 | 1 | scored |
| swe-20-add-pipe | swe_edit | 1 | 1 | scored |
| swe-21-fix-float-arithmetic | swe_edit | 1 | 1 | scored |
| swe-22-add-lru-cache | swe_edit | 1 | 1 | scored |
| swe-23-fix-enum | swe_edit | 1 | 1 | scored |
| swe-24-add-md5-hash | swe_edit | 1 | 1 | scored |
| swe-25-fix-timer-leak | swe_edit | 1 | 1 | scored |
| swe-26-add-trie | swe_edit | 1 | 1 | scored |
| swe-27-fix-promise-chain | swe_edit | 1 | 1 | scored |
| swe-28-add-csv-parse | swe_edit | 1 | 1 | scored |
| swe-29-fix-null-pointer | swe_edit | 1 | 1 | scored |
| swe-30-add-rate-limiter | swe_edit | 1 | 1 | scored |

## Interpretation

Very strong local agent performance (proxy). AA Grok4.5@GrokBuild Coding Agent Index ref=76.

## How to read against AA leaderboard

1. AA scores **model + harness**. Your number is **cn-codex-agent @ CN-Codex**.
2. This local suite is a **proxy** (12 tasks), not the official 321-task Coding Agent Index.
3. Use component breakdown to see if gaps are Q&A, terminal, or multi-file SWE.
4. If local proxy << 0.76 while AA Grok Build is 76, gap is likely harness/tooling/settings, not just model IQ.

Official links:
- https://artificialanalysis.ai/agents/coding-agents
- https://artificialanalysis.ai/models/grok-4-5
- https://artificialanalysis.ai/methodology/coding-agents-benchmarking
