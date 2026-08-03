import { describe, expect, it } from "vitest";
import { computeDetailLineScrollTop } from "../utils/detailLineScroll";

describe("computeDetailLineScrollTop", () => {
  it("keeps early lines near the top of the viewport", () => {
    expect(
      computeDetailLineScrollTop({
        line: 1,
        lineHeight: 20,
        paddingTop: 12,
        viewportHeight: 600,
      }),
    ).toBe(0);
  });

  it("uses viewport height instead of full document height", () => {
    // 旧实现误用 textarea 全高（≈文档高度）会把目标行算成 scrollTop=0。
    const broken = Math.max(0, (120 - 1) * 20 - 20000 / 3);
    expect(broken).toBe(0);

    expect(
      computeDetailLineScrollTop({
        line: 120,
        lineHeight: 20,
        paddingTop: 12,
        viewportHeight: 600,
      }),
    ).toBe(119 * 20 + 12 - 200);
  });
});
