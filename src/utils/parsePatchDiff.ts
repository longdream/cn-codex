export interface PatchDiffEntry {
  // 一个补丁项可能同时命中旧路径/新路径（例如 rename）。
  paths: string[];
  // 作为 Diff 左侧输入的文本。
  beforeContent: string;
  // 作为 Diff 右侧输入的文本。
  afterContent: string;
}

/**
 * 解析 apply_patch 文本得到每个文件的 before/after 内容块。
 * 说明：这里是“补丁级还原”，用于 Diff 可视化，不做完整文件重建。
 */
export function parsePatchDiffEntries(patch: string): PatchDiffEntry[] {
  const lines = patch.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
  const result: PatchDiffEntry[] = [];
  let idx = 0;

  const isBoundary = (line: string): boolean => {
    return (
      line.startsWith("*** Add File: ") ||
      line.startsWith("*** Update File: ") ||
      line.startsWith("*** Delete File: ") ||
      line.startsWith("*** End Patch")
    );
  };

  while (idx < lines.length) {
    const line = lines[idx];

    if (line.startsWith("*** Add File: ")) {
      const path = line.slice("*** Add File: ".length).trim();
      idx += 1;
      const afterLines: string[] = [];
      while (idx < lines.length && !isBoundary(lines[idx])) {
        const body = lines[idx];
        if (body.startsWith("+")) {
          afterLines.push(body.slice(1));
        }
        idx += 1;
      }
      result.push({
        paths: [path],
        beforeContent: "",
        afterContent: afterLines.join("\n"),
      });
      continue;
    }

    if (line.startsWith("*** Delete File: ")) {
      const path = line.slice("*** Delete File: ".length).trim();
      idx += 1;
      result.push({
        paths: [path],
        // 删除文件在补丁里通常不含完整正文，这里用占位确保弹窗有明确反馈。
        beforeContent: "[deleted file]",
        afterContent: "",
      });
      continue;
    }

    if (line.startsWith("*** Update File: ")) {
      const path = line.slice("*** Update File: ".length).trim();
      let moveTo: string | null = null;
      const beforeLines: string[] = [];
      const afterLines: string[] = [];
      idx += 1;
      while (idx < lines.length && !isBoundary(lines[idx])) {
        const body = lines[idx];
        if (body.startsWith("*** Move to: ")) {
          moveTo = body.slice("*** Move to: ".length).trim();
          idx += 1;
          continue;
        }
        if (body.startsWith("@@")) {
          idx += 1;
          continue;
        }
        if (body.startsWith("-")) {
          beforeLines.push(body.slice(1));
        } else if (body.startsWith("+")) {
          afterLines.push(body.slice(1));
        } else if (body.startsWith(" ")) {
          const context = body.slice(1);
          beforeLines.push(context);
          afterLines.push(context);
        }
        idx += 1;
      }
      result.push({
        paths: moveTo ? [path, moveTo] : [path],
        beforeContent: beforeLines.join("\n"),
        afterContent: afterLines.join("\n"),
      });
      continue;
    }

    idx += 1;
  }

  return result;
}
