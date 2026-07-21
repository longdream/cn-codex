{
  "mcpServers": {
    "miniapp": {
      "command": "{{nodePath}}",
      "args": ["server/index.mjs"],
      "cwd": "{{cwd}}",
      "env": {
        "MINIAPP_DATABASE_ID": "{{databaseId}}"
      }
    }
  }
}
