import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import {
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
});
