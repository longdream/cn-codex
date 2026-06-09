# API Troubleshooting
## General Guidance
IMPORTANT: do NOT attempt to dig through source code or control the browser through unrelated mechanisms before attempting the workflow for the selected backend. If you run into issues, follow the steps below FIRST.

- Do not fall back to Computer Use just because its tool calls are already visible. Read and attempt this workflow first.
- If `js_reset` is visible but `js` is not, do not conclude that `node_repl` is unusable. Use tool discovery for `node_repl js`, then `mcp__node_repl__js`, then `js`, then `node_repl js JavaScript execution`; run the bootstrap cell with the Node REPL `js` tool once it is exposed.
- If the Node REPL `js` execution tool is still unavailable after those searches, say that explicitly before choosing any fallback browser-control path.
- If `node_repl` is not available, say that explicitly before choosing any fallback browser-control path.
