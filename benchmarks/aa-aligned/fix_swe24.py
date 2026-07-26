content = open('benchmarks/aa-aligned/run_benchmark.py', encoding='utf-8').read()
content = content.replace('"swe-24-add-hash"', '"swe-24-add-md5-hash"')
open('benchmarks/aa-aligned/run_benchmark.py', 'w', encoding='utf-8').write(content)
print('Done')