import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

import {
  SSH_SOURCES_KEY,
  createEmptySmartbrainSshSource,
  formatSshTarget,
  loadSmartbrainSshSources,
  normalizeSmartbrainSshSource,
  saveSmartbrainSshSources,
  validateSmartbrainSshSource,
} from "../components/settings/smartbrainSshState";

describe("smartbrainSshState", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("creates an empty source with safe defaults", () => {
    const source = createEmptySmartbrainSshSource();
    expect(source.enabled).toBe(true);
    expect(source.port).toBe(22);
    expect(source.authMethod).toBe("password");
    expect(source.allowExec).toBe(false);
    expect(source.password).toBe("");
    expect(source.privateKey).toBe("");
    expect(source.privateKeyPath).toBe("");
    expect(source.passphrase).toBe("");
  });

  it("normalizes missing fields and empty names", () => {
    const normalized = normalizeSmartbrainSshSource({
      host: " 10.0.0.8 ",
      username: " deploy ",
      authMethod: "privateKey",
      privateKeyPath: " C:/keys/id_rsa ",
    });

    expect(normalized.name).toBe("deploy@10.0.0.8");
    expect(normalized.host).toBe("10.0.0.8");
    expect(normalized.port).toBe(22);
    expect(normalized.username).toBe("deploy");
    expect(normalized.authMethod).toBe("privateKey");
    expect(normalized.privateKeyPath).toBe("C:/keys/id_rsa");
    expect(normalized.enabled).toBe(true);
    expect(normalized.allowExec).toBe(false);
  });

  it("rejects incomplete password and private key drafts", () => {
    expect(validateSmartbrainSshSource(createEmptySmartbrainSshSource())).toBe(
      "hostRequired",
    );
    expect(
      validateSmartbrainSshSource({
        ...createEmptySmartbrainSshSource(),
        host: "10.0.0.8",
      }),
    ).toBe("usernameRequired");
    expect(
      validateSmartbrainSshSource({
        ...createEmptySmartbrainSshSource(),
        host: "10.0.0.8",
        username: "root",
      }),
    ).toBe("passwordRequired");
    expect(
      validateSmartbrainSshSource({
        ...createEmptySmartbrainSshSource(),
        host: "10.0.0.8",
        username: "root",
        authMethod: "privateKey",
      }),
    ).toBe("privateKeyRequired");
    expect(
      validateSmartbrainSshSource({
        ...createEmptySmartbrainSshSource(),
        host: "10.0.0.8",
        username: "root",
        port: 70000,
        password: "secret",
      }),
    ).toBe("portInvalid");
  });

  it("accepts a complete password or private-key source", () => {
    expect(
      validateSmartbrainSshSource({
        ...createEmptySmartbrainSshSource(),
        host: "10.0.0.8",
        username: "root",
        password: "secret",
      }),
    ).toBeNull();
    expect(
      validateSmartbrainSshSource({
        ...createEmptySmartbrainSshSource(),
        host: "10.0.0.8",
        username: "root",
        authMethod: "privateKey",
        privateKey: "-----BEGIN OPENSSH PRIVATE KEY-----",
      }),
    ).toBeNull();
  });

  it("formats the server target for list display", () => {
    expect(
      formatSshTarget({
        ...createEmptySmartbrainSshSource(),
        username: "deploy",
        host: "192.168.1.10",
        port: null,
      }),
    ).toBe("deploy@192.168.1.10:22");
  });

  it("loads an empty list when sqlite has no ssh sources", async () => {
    invoke.mockResolvedValueOnce(null);
    await expect(loadSmartbrainSshSources()).resolves.toEqual([]);
    expect(invoke).toHaveBeenCalledWith("app_state_get", { key: SSH_SOURCES_KEY });
  });

  it("fills defaults when loading incomplete persisted sources", async () => {
    invoke.mockResolvedValueOnce(
      JSON.stringify([
        {
          id: "ssh-1",
          name: "生产跳板机",
          host: "192.168.1.10",
          username: "deploy",
          authMethod: "privateKey",
          privateKeyPath: "/home/ops/.ssh/id_rsa",
        },
      ]),
    );

    const loaded = await loadSmartbrainSshSources();
    expect(loaded).toHaveLength(1);
    expect(loaded[0].id).toBe("ssh-1");
    expect(loaded[0].enabled).toBe(true);
    expect(loaded[0].port).toBe(22);
    expect(loaded[0].allowExec).toBe(false);
    expect(loaded[0].password).toBe("");
    expect(loaded[0].privateKey).toBe("");
    expect(loaded[0].passphrase).toBe("");
  });

  it("saves sources back to sqlite app_state", async () => {
    invoke.mockResolvedValueOnce(undefined);
    const source = normalizeSmartbrainSshSource({
      ...createEmptySmartbrainSshSource(),
      name: "测试机",
      host: "10.0.0.8",
      username: "root",
      password: "secret",
    });

    await saveSmartbrainSshSources([source]);
    expect(invoke).toHaveBeenCalledWith("app_state_set", {
      key: SSH_SOURCES_KEY,
      value: JSON.stringify([source]),
    });
  });
});
