import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import {
  browserGetNavigationState,
  browserGoBack,
  browserGoForward,
  browserNavigateHome,
  copyPathEntry,
  createPathEntry,
  deletePath,
  renamePathEntry,
  searchWorkspaceFiles,
  windowClose,
  windowMinimize,
  windowOpenBrowser,
  windowStartDragging,
  windowToggleMaximize,
} from "../api/window";

const mockInvoke = vi.mocked(invoke);

describe("window API", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("opens built-in browser with a trimmed URL", async () => {
    mockInvoke.mockResolvedValueOnce({
      label: "cn-browser",
      url: "http://localhost:1420/",
      created: true,
      debugPort: 9242,
      cdpEndpoint: "http://127.0.0.1:9242",
    });

    await expect(windowOpenBrowser(" localhost:1420 ")).resolves.toMatchObject({
      label: "cn-browser",
      created: true,
    });
    expect(mockInvoke).toHaveBeenCalledWith("window_open_browser", {
      url: "localhost:1420",
      x: null,
      y: null,
      width: null,
      height: null,
    });
  });

  it("passes null when opening the blank browser window", async () => {
    mockInvoke.mockResolvedValueOnce({
      label: "cn-browser",
      url: "about:blank",
      created: false,
      debugPort: 9242,
      cdpEndpoint: "http://127.0.0.1:9242",
    });

    await windowOpenBrowser(" ");
    expect(mockInvoke).toHaveBeenCalledWith("window_open_browser", {
      url: null,
      x: null,
      y: null,
      width: null,
      height: null,
    });
  });

  it("routes titlebar window actions through tauri commands", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await windowStartDragging();
    await windowMinimize();
    await windowToggleMaximize();
    await windowClose();

    expect(mockInvoke).toHaveBeenNthCalledWith(1, "window_start_dragging");
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "window_minimize");
    expect(mockInvoke).toHaveBeenNthCalledWith(3, "window_toggle_maximize");
    expect(mockInvoke).toHaveBeenNthCalledWith(4, "window_close");
  });

  it("routes browser navigation commands through tauri invoke", async () => {
    const payload = {
      url: "https://example.com",
      title: "Example Domain",
      canGoBack: true,
      canGoForward: false,
    };
    mockInvoke
      .mockResolvedValueOnce(payload)
      .mockResolvedValueOnce(payload)
      .mockResolvedValueOnce(payload)
      .mockResolvedValueOnce(payload);

    await expect(browserGoBack()).resolves.toMatchObject(payload);
    await expect(browserGoForward()).resolves.toMatchObject(payload);
    await expect(browserNavigateHome()).resolves.toMatchObject(payload);
    await expect(browserGetNavigationState()).resolves.toMatchObject(payload);

    expect(mockInvoke).toHaveBeenNthCalledWith(1, "browser_go_back");
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "browser_go_forward");
    expect(mockInvoke).toHaveBeenNthCalledWith(3, "browser_navigate_home");
    expect(mockInvoke).toHaveBeenNthCalledWith(4, "browser_get_navigation_state");
  });

  it("routes workspace file content search through tauri invoke", async () => {
    mockInvoke.mockResolvedValueOnce({
      query: "hello",
      matches: [
        {
          path: "E:/work/demo/src/a.ts",
          name: "a.ts",
          relativePath: "src/a.ts",
          kind: "content",
          line: 12,
          preview: "console.log('hello')",
          isDir: false,
        },
      ],
      truncated: false,
      searchedFiles: 3,
    });

    await expect(searchWorkspaceFiles("E:/work/demo", "hello", 50)).resolves.toMatchObject({
      query: "hello",
      searchedFiles: 3,
    });
    expect(mockInvoke).toHaveBeenCalledWith("search_workspace_files", {
      root: "E:/work/demo",
      query: "hello",
      maxResults: 50,
      caseSensitive: false,
      include: null,
    });
  });

  it("forwards content search options to tauri invoke", async () => {
    mockInvoke.mockResolvedValueOnce({
      query: "Hello",
      matches: [],
      truncated: false,
      searchedFiles: 0,
    });

    await searchWorkspaceFiles("E:/work/demo", "Hello", 80, {
      caseSensitive: true,
      include: "src/**/*.ts",
    });

    expect(mockInvoke).toHaveBeenCalledWith("search_workspace_files", {
      root: "E:/work/demo",
      query: "Hello",
      maxResults: 80,
      caseSensitive: true,
      include: "src/**/*.ts",
    });
  });

  it("routes file tree mutation commands through tauri invoke", async () => {
    mockInvoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({
        name: "new.ts",
        path: "E:/work/demo/src/new.ts",
        isDir: false,
        size: 0,
      })
      .mockResolvedValueOnce({
        name: "renamed.ts",
        path: "E:/work/demo/src/renamed.ts",
        isDir: false,
        size: 0,
      })
      .mockResolvedValueOnce({
        name: "copy.ts",
        path: "E:/work/demo/src/copy.ts",
        isDir: false,
        size: 0,
      });

    await deletePath("E:/work/demo/src/a.ts", false);
    await createPathEntry("E:/work/demo/src", "new.ts", false);
    await renamePathEntry("E:/work/demo/src/a.ts", "E:/work/demo/src/renamed.ts");
    await copyPathEntry("E:/work/demo/src/a.ts", "E:/work/demo/src/copy.ts");

    expect(mockInvoke).toHaveBeenNthCalledWith(1, "delete_path", {
      path: "E:/work/demo/src/a.ts",
      recursive: false,
    });
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "create_path_entry", {
      parentDir: "E:/work/demo/src",
      name: "new.ts",
      isDir: false,
    });
    expect(mockInvoke).toHaveBeenNthCalledWith(3, "rename_path_entry", {
      from: "E:/work/demo/src/a.ts",
      to: "E:/work/demo/src/renamed.ts",
    });
    expect(mockInvoke).toHaveBeenNthCalledWith(4, "copy_path_entry", {
      from: "E:/work/demo/src/a.ts",
      to: "E:/work/demo/src/copy.ts",
    });
  });
});
