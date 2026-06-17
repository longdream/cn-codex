import { describe, it, expect } from "vitest";
import { getDefaultSlashCommands } from "../components/chat/SlashCommandPanel";

describe("getDefaultSlashCommands", () => {
  const commands = getDefaultSlashCommands();

  it("returns 8 commands", () => {
    expect(commands).toHaveLength(8);
  });

  it("each command has name, description, and action", () => {
    for (const cmd of commands) {
      expect(cmd).toHaveProperty("name");
      expect(cmd).toHaveProperty("description");
      expect(cmd).toHaveProperty("action");
      expect(typeof cmd.name).toBe("string");
      expect(typeof cmd.description).toBe("string");
      expect(typeof cmd.action).toBe("function");
    }
  });

  it("includes plan, goal, skill, model, clear, compact, help, modifyrobot", () => {
    const names = commands.map((c) => c.name);
    expect(names).toContain("plan");
    expect(names).toContain("goal");
    expect(names).toContain("skill");
    expect(names).toContain("model");
    expect(names).toContain("clear");
    expect(names).toContain("compact");
    expect(names).toContain("help");
    expect(names).toContain("modifyrobot");
  });
});
