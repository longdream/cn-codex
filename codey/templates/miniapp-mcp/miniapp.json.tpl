{
  "id": "miniapp-{{slug}}",
  "name": "{{name}}",
  "slug": "{{slug}}",
  "description": "{{description}}",
  "databaseId": "{{databaseId}}",
  "databaseName": "{{databaseName}}",
  "status": "generated",
  "port": null,
  "rootPath": "",
  "mcp": {
    "command": "{{nodePath}}",
    "args": ["server/index.mjs"],
    "cwd": ""
  },
  "pages": [
    {
      "id": "home",
      "title": "首页",
      "path": "/",
      "description": "默认示例页"
    }
  ],
  "tools": [
    { "name": "list_pages", "description": "列出页面" },
    { "name": "get_status", "description": "健康检查" },
    { "name": "open_page", "description": "打开页面" }
  ],
  "lastError": "",
  "createdAt": 0,
  "updatedAt": 0
}
