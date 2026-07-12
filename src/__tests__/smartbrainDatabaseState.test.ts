import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import {
  applyDatabaseNameToConnectionUri,
  mergeSmartbrainDbParsedFields,
  parseSmartbrainConnectionUriLocally,
} from "../components/settings/smartbrainDatabaseState";

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

  it("keeps existing database fields when merging parse results", () => {
    const merged = mergeSmartbrainDbParsedFields(
      {
        host: "10.0.0.8",
        port: 3307,
        databaseName: "manual_db",
        username: "manual_user",
        password: "manual_pass",
      },
      {
        host: "parsed-host",
        port: 3306,
        databaseName: "parsed_db",
        username: "parsed_user",
        password: "parsed_pass",
      },
    );

    expect(merged.host).toBe("10.0.0.8");
    expect(merged.port).toBe(3307);
    expect(merged.databaseName).toBe("manual_db");
    expect(merged.username).toBe("manual_user");
    expect(merged.password).toBe("manual_pass");
  });

  it("updates database name inside connection strings", () => {
    const url = applyDatabaseNameToConnectionUri(
      "mysql",
      "mysql://root:secret@10.0.0.1:3306/old_db",
      "new_db",
    );
    expect(url).toContain("/new_db");

    const kv = applyDatabaseNameToConnectionUri(
      "mysql",
      "Server=10.0.0.1;Port=3306;Database=old_db;Uid=root;Pwd=secret;",
      "new_db",
    );
    expect(kv.toLowerCase()).toContain("database=new_db");
  });
});
