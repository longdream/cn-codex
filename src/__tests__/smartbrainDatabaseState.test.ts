import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { parseSmartbrainConnectionUriLocally } from "../components/settings/smartbrainDatabaseState";

describe("smartbrainDatabaseState", () => {
  it("parses MySQL key-value connection strings", () => {
    const parsed = parseSmartbrainConnectionUriLocally(
      "mysql",
      "Server=10.136.0.134;Port=3306;Database=psa_crm_pact_test;Uid=root;Pwd=secret;CharSet=utf8;SslMode=none;",
    );

    expect(parsed.host).toBe("10.136.0.134");
    expect(parsed.port).toBe(3306);
    expect(parsed.databaseName).toBe("psa_crm_pact_test");
    expect(parsed.username).toBe("root");
    expect(parsed.password).toBe("secret");
    expect(parsed.queryParams).toEqual({
      charset: "utf8",
      sslmode: "none",
    });
  });

  it("parses standard PostgreSQL URLs", () => {
    const parsed = parseSmartbrainConnectionUriLocally(
      "postgresql",
      "postgresql://readonly:test@localhost:5432/app_db?schema=public&sslmode=disable",
    );

    expect(parsed.host).toBe("localhost");
    expect(parsed.port).toBe(5432);
    expect(parsed.databaseName).toBe("app_db");
    expect(parsed.username).toBe("readonly");
    expect(parsed.password).toBe("test");
    expect(parsed.schema).toBe("public");
    expect(parsed.queryParams).toEqual({
      schema: "public",
      sslmode: "disable",
    });
  });
});
