from pathlib import Path

path = Path(r"src/components/settings/SmartbrainDatabasePanel.tsx")
text = path.read_text(encoding="utf-8")

old = '''  const handleRefreshDatabases = useCallback(async () => {
    if (draft.dbType === "sqlite") {
      const path = draft.filePath.trim() || draft.databaseName.trim() || draft.connectionUri.trim();
      if (!path) {
        setNotice({
          kind: "error",
          text: intl.formatMessage({ id: "settings.smartbrain.database.refreshNeedConnection" }),
        });
        return;
      }
      const fileName = path.split(/[\\\\/]/).filter(Boolean).pop() || path;
      setDatabaseOptions([fileName]);
      setDraft((prev) => ({
        ...prev,
        databaseName: prev.databaseName.trim() || fileName,
        filePath: prev.filePath.trim() || path,
      }));
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.database.refreshSuccess" }, { count: 1 }),
      });
      return;
    }

    if (!draft.host.trim() && !draft.connectionUri.trim()) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.refreshNeedConnection" }),
      });
      return;
    }

    setRefreshingDatabases(true);
    try {
      const databases = await listSmartbrainDatabases({
        dbType: draft.dbType,
        host: draft.host,
        port: draft.port,
        username: draft.username,
        password: draft.password,
        connectionUri: draft.connectionUri,
        databaseName: draft.databaseName,
        filePath: draft.filePath,
      });
      setDatabaseOptions(databases);

      if (databases.length === 0) {
        setNotice({
          kind: "warning",
          text: intl.formatMessage({ id: "settings.smartbrain.database.refreshEmpty" }),
        });
        return;
      }

      setNotice({
        kind: "success",
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.refreshSuccess" },
          { count: databases.length },
        ),
      });

      const currentName = draft.databaseName.trim();
      if (!currentName || !databases.includes(currentName)) {
        const nextName = databases[0];
        setDraft((prev) => ({
          ...prev,
          databaseName: nextName,
          connectionUri: applyDatabaseNameToConnectionUri(prev.dbType, prev.connectionUri, nextName),
        }));
      }
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.refreshFailed" },
          { error: typeof error === "string" ? error : (error as Error).message },
        ),
      });
    } finally {
      setRefreshingDatabases(false);
    }
  }, [draft, intl]);'''

new = '''  const handleRefreshDatabases = useCallback(async () => {
    let working = { ...draft };

    if (working.connectionUri.trim()) {
      try {
        const parsed = parseSmartbrainConnectionUriLocally(working.dbType, working.connectionUri);
        working = {
          ...working,
          ...mergeSmartbrainDbParsedFields(working, parsed),
        };
        setDraft((prev) => ({
          ...prev,
          ...mergeSmartbrainDbParsedFields(prev, parsed),
        }));
      } catch {
        // Keep manual fields when the connection string cannot be parsed locally.
      }
    }

    if (working.dbType === "sqlite") {
      const path = working.filePath.trim() || working.databaseName.trim() || working.connectionUri.trim();
      if (!path) {
        setNotice({
          kind: "error",
          text: intl.formatMessage({ id: "settings.smartbrain.database.refreshNeedConnection" }),
        });
        return;
      }
      const fileName = path.split(/[\\\\/]/).filter(Boolean).pop() || path;
      setDatabaseOptions([fileName]);
      setDraft((prev) => ({
        ...prev,
        databaseName: prev.databaseName.trim() || fileName,
        filePath: prev.filePath.trim() || path,
      }));
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.database.refreshSuccess" }, { count: 1 }),
      });
      return;
    }

    if (!working.host.trim() && !working.connectionUri.trim()) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.refreshNeedConnection" }),
      });
      return;
    }

    setRefreshingDatabases(true);
    try {
      const databases = await listSmartbrainDatabases({
        dbType: working.dbType,
        host: working.host,
        port: working.port,
        username: working.username,
        password: working.password,
        connectionUri: working.connectionUri,
        databaseName: working.databaseName,
        filePath: working.filePath,
      });
      setDatabaseOptions(databases);

      if (databases.length === 0) {
        setNotice({
          kind: "warning",
          text: intl.formatMessage({ id: "settings.smartbrain.database.refreshEmpty" }),
        });
        return;
      }

      setNotice({
        kind: "success",
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.refreshSuccess" },
          { count: databases.length },
        ),
      });

      const currentName = working.databaseName.trim();
      if (!currentName || !databases.includes(currentName)) {
        const nextName = databases[0];
        setDraft((prev) => ({
          ...prev,
          databaseName: nextName,
          connectionUri: applyDatabaseNameToConnectionUri(prev.dbType, prev.connectionUri, nextName),
        }));
      }
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.refreshFailed" },
          { error: typeof error === "string" ? error : (error as Error).message },
        ),
      });
    } finally {
      setRefreshingDatabases(false);
    }
  }, [draft, intl]);'''

if old not in text:
    raise SystemExit("panel refresh handler not found")
text = text.replace(old, new, 1)

imp_old = '''  listSmartbrainDatabases,
  loadSmartbrainDbSettings,
  loadSmartbrainDbSources,
  mergeSmartbrainDbParsedFields,
  parseSmartbrainConnectionUri,
  saveSmartbrainDbSources,'''

imp_new = '''  listSmartbrainDatabases,
  loadSmartbrainDbSettings,
  loadSmartbrainDbSources,
  mergeSmartbrainDbParsedFields,
  parseSmartbrainConnectionUri,
  parseSmartbrainConnectionUriLocally,
  saveSmartbrainDbSources,'''

if imp_old not in text:
    raise SystemExit("import block not found")
text = text.replace(imp_old, imp_new, 1)

path.write_text(text, encoding="utf-8")
print("updated panel")
