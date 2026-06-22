import { describe, expect, it } from "vitest";

import {
  parseFortuneDetailResponseForTest,
  parseFortuneSummaryResponseForTest,
} from "../utils/fortune";

const BASE_FORTUNE_SUMMARY = {
  date: "2026-06-22",
  overall: "阻滞",
  direction: "北方",
  bestAction: "等待",
  environment: "不利",
  summary: "空亡当头，宜静待时机",
};

const BASE_FORTUNE_DETAIL = {
  qimenDetail: "## 奇门遁甲\n- 值符落坎宫\n- 值使临死门",
  advice: "宜静不宜动，先稳住节奏。",
};

describe("parseFortuneSummaryResponseForTest", () => {
  it("parses valid summary json payload", () => {
    const parsed = parseFortuneSummaryResponseForTest(JSON.stringify(BASE_FORTUNE_SUMMARY));
    expect(parsed).not.toBeNull();
    expect(parsed?.overall).toBe("阻滞");
    expect(parsed?.summary).toContain("空亡当头");
  });

  it("parses payload wrapped with markdown fence and noise text", () => {
    const payload = `以下是结果：\n\`\`\`json\n${JSON.stringify(BASE_FORTUNE_SUMMARY, null, 2)}\n\`\`\`\n请查收`;
    const parsed = parseFortuneSummaryResponseForTest(payload);
    expect(parsed).not.toBeNull();
    expect(parsed?.direction).toBe("北方");
  });

  it("repairs non-strict summary json using jsonrepair", () => {
    const nonStrict = `{
      date: '2026-06-22',
      overall: '阻滞',
      direction: '北方',
      bestAction: '等待',
      environment: '不利',
      summary: '空亡当头，宜静待时机',
    }`;
    const parsed = parseFortuneSummaryResponseForTest(nonStrict);
    expect(parsed).not.toBeNull();
    expect(parsed?.overall).toBe("阻滞");
    expect(parsed?.direction).toBe("北方");
  });

  it("returns null when summary required fields are missing", () => {
    const noAdvice = {
      ...BASE_FORTUNE_SUMMARY,
      summary: "",
    };
    expect(parseFortuneSummaryResponseForTest(JSON.stringify(noAdvice))).toBeNull();
  });

  it("returns null for pure narrative response", () => {
    const narrative = "我们根据当前时间进行奇门遁甲排盘，先判断节气，再确定用局。";
    expect(parseFortuneSummaryResponseForTest(narrative)).toBeNull();
  });
});

describe("parseFortuneDetailResponseForTest", () => {
  it("parses detail json payload", () => {
    const parsed = parseFortuneDetailResponseForTest(JSON.stringify(BASE_FORTUNE_DETAIL));
    expect(parsed).not.toBeNull();
    expect(parsed?.qimenDetail).toContain("值符落坎宫");
    expect(parsed?.advice).toContain("稳住节奏");
  });

  it("returns null when detail required fields are missing", () => {
    expect(parseFortuneDetailResponseForTest("{\"qimenDetail\":\"仅有奇门\"}")).toBeNull();
  });
});
