#!/usr/bin/env python3
"""Verify all 90 tasks have required files."""
import json, os

with open('benchmarks/aa-aligned/suite.json', encoding='utf-8') as f:
    s = json.load(f)

tasks = s['tasks']
print(f'Total tasks: {len(tasks)}')

from collections import Counter
for comp, cnt in sorted(Counter(t['component'] for t in tasks).items()):
    print(f'  {comp}: {cnt}')

base = 'benchmarks/aa-aligned/tasks'
ok = True
for t in tasks:
    d = os.path.join(base, t['id'])
    pm = os.path.join(d, 'PROMPT.md')
    gp = os.path.join(d, 'grade.ps1')
    if not os.path.exists(pm):
        print(f'MISSING: {t["id"]}/PROMPT.md')
        ok = False
    if not os.path.exists(gp):
        print(f'MISSING: {t["id"]}/grade.ps1')
        ok = False

if ok:
    print('All task files present - ready to run!')