with open('utils/llm_client.py','rb') as f:
    data = f.read()

# Find the split pattern
idx = data.find(b'.split(')
print(f"Found .split( at byte {idx}")
if idx >= 0:
    # Show 30 bytes before and 40 after
    ctx = data[idx-10:idx+50]
    print(f"Context: {ctx}")
    print(f"Hex: {ctx.hex()}")
    
# Also search for the if "" pattern
idx2 = data.find(b'if ')
for i in range(len(data)-10):
    if data[i:i+2] == b'if':
        if data[i+3:i+5] == b'\"\"':
            ctx = data[i:i+30]
            print(f"if '' found at {i}: {ctx}")
