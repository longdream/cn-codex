---
name: wps-document-control
description: "Control WPS Office Word documents via the wps_execute_command tool. Open, edit, format, and save documents through the connected WPS add-in."
tags:
  - wps
  - word
  - document
  - office
---

# WPS Office Document Control

Use the `wps_execute_command` tool to manipulate Word documents in WPS Office.

## Prerequisites

- WPS Office must be running with the cn-codex add-in loaded
- The WPS WebSocket server must be started (check via settings or `wps_status` command)
- The add-in must show "已连接" status in the CN-Codex ribbon tab

## Tool Usage

All commands use the same tool:

```
wps_execute_command(method: string, params?: object)
```

## Available Commands

### Document Management

**Open a document:**
```json
{"method": "document.open", "params": {"path": "C:\\Documents\\report.docx"}}
```

**Save the active document:**
```json
{"method": "document.save"}
```

**Save as a new file:**
```json
{"method": "document.saveAs", "params": {"path": "C:\\Documents\\report_v2.docx"}}
```

**Close the active document:**
```json
{"method": "document.close", "params": {"save": true}}
```

**Get document info (page count, word count, etc.):**
```json
{"method": "document.getInfo"}
```
Returns: `{name, path, saved, pageCount, wordCount, charCount}`

**Read document content:**
```json
{"method": "document.getContent", "params": {"maxLength": 50000}}
```
Returns: `{text, truncated, totalLength}`

### Text Operations

**Insert text at cursor position:**
```json
{"method": "text.insert", "params": {"text": "Hello World"}}
```

**Insert text at a specific position:**
- `"position": "start"` — beginning of document
- `"position": "end"` — end of document
- `"position": "cursor"` — current cursor (default)
- `"position": 100` — character offset

**Find and replace:**
```json
{"method": "text.replace", "params": {"find": "old text", "replace": "new text", "all": true, "matchCase": false}}
```

**Delete text by range:**
```json
{"method": "text.delete", "params": {"start": 0, "end": 50}}
```

### Formatting

**Set font properties:**
```json
{"method": "format.setFont", "params": {"name": "微软雅黑", "size": 14, "bold": true, "color": "#FF0000"}}
```
Supported properties: `name`, `size`, `bold`, `italic`, `underline`, `color` (hex), `strikethrough`.
Optional `start`/`end` to target a specific range; otherwise applies to current selection.

**Set paragraph format:**
```json
{"method": "format.setParagraph", "params": {"alignment": "center", "lineSpacing": 24, "spaceBefore": 6, "spaceAfter": 6}}
```
Alignment values: `"left"`, `"center"`, `"right"`, `"justify"`.
Other: `firstLineIndent`, `leftIndent`, `rightIndent`.

**Apply a named style:**
```json
{"method": "format.setStyle", "params": {"style": "Heading 1"}}
```

### Bookmarks

**Add a bookmark at current selection:**
```json
{"method": "bookmark.add", "params": {"name": "introduction"}}
```

**Navigate to a bookmark:**
```json
{"method": "bookmark.goto", "params": {"name": "introduction"}}
```

**List all bookmarks:**
```json
{"method": "bookmark.list"}
```

**Delete a bookmark:**
```json
{"method": "bookmark.delete", "params": {"name": "introduction"}}
```

### Tables

**Insert a table:**
```json
{"method": "table.insert", "params": {"rows": 3, "cols": 4, "data": [["Name","Age","City","Score"],["Alice","30","Beijing","95"],["Bob","25","Shanghai","88"]]}}
```

**Set a cell value:**
```json
{"method": "table.setCell", "params": {"tableIndex": 1, "row": 2, "col": 1, "text": "Charlie"}}
```

**Read a cell value:**
```json
{"method": "table.getCell", "params": {"tableIndex": 1, "row": 1, "col": 1}}
```

### Comments and Revisions

**Add a comment:**
```json
{"method": "comment.add", "params": {"text": "Please review this section"}}
```
With range: add `"start"` and `"end"` to target specific text.

**List all comments:**
```json
{"method": "comment.list"}
```

**Delete a comment:**
```json
{"method": "comment.delete", "params": {"index": 1}}
```

**Accept revisions:**
```json
{"method": "revision.accept", "params": {"all": true}}
```
Or accept a single revision: `{"index": 1}`

**Reject revisions:**
```json
{"method": "revision.reject", "params": {"all": true}}
```

### Headers and Footers

**Set header text:**
```json
{"method": "header.set", "params": {"text": "Confidential Report", "font": {"name": "Arial", "size": 10}}}
```

**Set footer text:**
```json
{"method": "footer.set", "params": {"text": "Page footer", "section": 1}}
```

### Images

**Insert an image:**
```json
{"method": "image.insert", "params": {"path": "C:\\Images\\logo.png", "width": 200, "height": 100}}
```

### Template Filling

**Fill template placeholders:**
```json
{"method": "template.fill", "params": {"fields": {"name": "张三", "date": "2024-01-01", "department": "技术部"}}}
```
Default placeholder format: `{{key}}`. Custom format via `prefix`/`suffix` params.

## Workflow Example

1. Open the document:
   `wps_execute_command(method="document.open", params={"path": "C:\\template.docx"})`

2. Fill template fields:
   `wps_execute_command(method="template.fill", params={"fields": {"title": "Annual Report", "author": "AI Assistant"}})`

3. Insert additional content:
   `wps_execute_command(method="text.insert", params={"text": "\n\nGenerated content here...", "position": "end"})`

4. Format the title:
   `wps_execute_command(method="format.setFont", params={"start": 0, "end": 20, "size": 24, "bold": true})`

5. Save the document:
   `wps_execute_command(method="document.save")`

## Error Handling

- If no WPS is connected: the tool returns an error message asking to connect
- If no active document: document commands return "No active document"
- If a command times out (30s): the tool returns a timeout error
- All errors are returned as text; check the response for "error" or "failed"
