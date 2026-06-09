import fs from "node:fs/promises";
import fsSync from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const LOCAL_GRANOLA_RUNTIME = "local-granola";
const DEFAULT_GRANOLA_REPO = "/Users/esun/code/openai/lib/js/oai_js_granola";

let cachedRuntime;
let walnutExportsPromise;

function scriptDir() {
  return path.dirname(fileURLToPath(import.meta.url));
}

function defaultRuntimeNodeModules() {
  return path.join(
    process.env.HOME || process.cwd(),
    ".cache",
    "codex-runtimes",
    ["codex", "primary", "runtime"].join("-"),
    "dependencies",
    "node",
    "node_modules",
  );
}

function resolveGranolaRepo() {
  const candidates = [
    process.env.GRANOLA_REPO,
    path.resolve(process.cwd(), "lib/js/oai_js_granola"),
    DEFAULT_GRANOLA_REPO,
  ].filter(Boolean);

  for (const candidate of candidates) {
    const resolved = path.resolve(candidate);
    if (fsSync.existsSync(path.join(resolved, "package.json"))) {
      return resolved;
    }
  }

  throw new Error(
    [
      "Could not find a local Granola repo.",
      "Set GRANOLA_REPO=/absolute/path/to/lib/js/oai_js_granola.",
    ].join("\n"),
  );
}

function assertLocalGranolaEnabled() {
  if (process.env.PRESENTATIONS_RUNTIME !== LOCAL_GRANOLA_RUNTIME) {
    throw new Error(`Set PRESENTATIONS_RUNTIME=${LOCAL_GRANOLA_RUNTIME} to use local Granola.`);
  }
  if (process.env.NODE_ENV === "production") {
    throw new Error("PRESENTATIONS_RUNTIME=local-granola is disabled when NODE_ENV=production.");
  }
}

async function ensureModulePackage(workspaceDir) {
  const packagePath = path.join(workspaceDir, "package.json");
  if (!fsSync.existsSync(packagePath)) {
    await fs.writeFile(
      packagePath,
      `${JSON.stringify({ private: true, type: "module" }, null, 2)}\n`,
      "utf8",
    );
    return;
  }

  const packageJson = JSON.parse(await fs.readFile(packagePath, "utf8"));
  if (packageJson.type !== "module") {
    throw new Error(`${packagePath} exists but does not set "type": "module".`);
  }
}

async function ensureSymlink(target, source) {
  await fs.mkdir(path.dirname(target), { recursive: true });
  const existing = await fs.lstat(target).catch(() => undefined);
  if (existing) {
    if (existing.isSymbolicLink()) {
      const existingRealpath = fsSync.realpathSync(target);
      const sourceRealpath = fsSync.realpathSync(source);
      if (existingRealpath === sourceRealpath) return target;
      await fs.rm(target, { recursive: true, force: true });
    } else {
      throw new Error(`${target} exists but is not a symlink.`);
    }
  }

  await fs.symlink(source, target, process.platform === "win32" ? "junction" : "dir");
  return target;
}

function workspacePackagePath(workspaceDir, packageName) {
  return path.join(workspaceDir, "node_modules", ...packageName.split("/"));
}

function optionalPackageFromRuntime(packageName) {
  const packagePath = path.join(defaultRuntimeNodeModules(), ...packageName.split("/"));
  return fsSync.existsSync(path.join(packagePath, "package.json")) ? packagePath : undefined;
}

function optionalPackageFromGranola(granolaRepo, packageName) {
  const requireFromGranola = createRequire(path.join(granolaRepo, "package.json"));
  try {
    const packageJson = requireFromGranola.resolve(`${packageName}/package.json`);
    return path.dirname(packageJson);
  } catch {
    return undefined;
  }
}

function moduleUrl(filePath) {
  return JSON.stringify(pathToFileURL(filePath).href);
}

function namedExportsModule(filePath, exportNames) {
  return [
    `const module = await import(${moduleUrl(filePath)});`,
    ...exportNames.map((name) => `export const ${name} = module.${name};`),
    "export default module;",
    "",
  ].join("\n");
}

function artifactToolRootModule() {
  return [
    'import path from "node:path";',
    'import { fileURLToPath } from "node:url";',
    `import { importGranolaCompat } from ${moduleUrl(path.join(scriptDir(), "granola_artifact_tool_compat.mjs"))};`,
    "",
    "const packageDir = path.dirname(fileURLToPath(import.meta.url));",
    "const workspaceDir = path.resolve(packageDir, '../../..');",
    "const artifact = await importGranolaCompat(workspaceDir);",
    "",
    "export const FileBlob = artifact.FileBlob;",
    "export const Presentation = artifact.Presentation;",
    "export const PresentationFile = artifact.PresentationFile;",
    "export default artifact;",
    "",
  ].join("\n");
}

function granolaRootModule(granolaRepo) {
  return [
    `const presentationModule = await import(${moduleUrl(
      path.join(granolaRepo, "src/models/presentation/presentation.ts"),
    )});`,
    "",
    "export const Presentation = presentationModule.Presentation;",
    "export default { Presentation };",
    "",
  ].join("\n");
}

function packageJsonFor(packageName, exportsMap) {
  return `${JSON.stringify(
    {
      name: packageName,
      private: true,
      type: "module",
      presentationsLocalGranolaCompat: true,
      exports: exportsMap,
    },
    null,
    2,
  )}\n`;
}

async function ensureCompatPackage(workspaceDir, packageName, packageJson, files) {
  const target = workspacePackagePath(workspaceDir, packageName);
  await fs.mkdir(path.dirname(target), { recursive: true });

  const existing = await fs.lstat(target).catch(() => undefined);
  if (existing) {
    if (existing.isSymbolicLink() || existing.isFile()) {
      await fs.rm(target, { recursive: true, force: true });
    } else if (existing.isDirectory()) {
      const existingPackagePath = path.join(target, "package.json");
      const existingPackage = fsSync.existsSync(existingPackagePath)
        ? JSON.parse(fsSync.readFileSync(existingPackagePath, "utf8"))
        : {};
      if (existingPackage.presentationsLocalGranolaCompat === true) {
        await fs.rm(target, { recursive: true, force: true });
      } else {
        throw new Error(`${target} exists but is not a generated local Granola compatibility package.`);
      }
    } else {
      throw new Error(`${target} exists but is not a supported package path.`);
    }
  }

  await fs.mkdir(target, { recursive: true });
  await fs.writeFile(path.join(target, "package.json"), packageJson, "utf8");
  await Promise.all(
    Object.entries(files).map(([fileName, contents]) =>
      fs.writeFile(path.join(target, fileName), contents, "utf8"),
    ),
  );
  return target;
}

async function ensureGranolaCompatPackages(workspaceDir, granolaRepo) {
  const presentationJsx = namedExportsModule(path.join(granolaRepo, "src/presentation-jsx/index.ts"), [
    "Fragment",
    "createRef",
    "paint",
    "stroke",
    "textStyle",
  ]);
  const presentationJsxRuntime = namedExportsModule(
    path.join(granolaRepo, "src/presentation-jsx/jsx-runtime.ts"),
    ["Fragment", "jsx", "jsxs"],
  );
  const presentationJsxDevRuntime = namedExportsModule(
    path.join(granolaRepo, "src/presentation-jsx/jsx-dev-runtime.ts"),
    ["Fragment", "jsxDEV"],
  );

  await ensureCompatPackage(
    workspaceDir,
    "@oai/artifact-tool",
    packageJsonFor("@oai/artifact-tool", {
      ".": "./artifact_tool.mjs",
      "./presentation-jsx": "./presentation-jsx.mjs",
      "./presentation-jsx/jsx-runtime": "./presentation-jsx-jsx-runtime.mjs",
      "./presentation-jsx/jsx-dev-runtime": "./presentation-jsx-jsx-dev-runtime.mjs",
    }),
    {
      "artifact_tool.mjs": artifactToolRootModule(),
      "presentation-jsx.mjs": presentationJsx,
      "presentation-jsx-jsx-runtime.mjs": presentationJsxRuntime,
      "presentation-jsx-jsx-dev-runtime.mjs": presentationJsxDevRuntime,
    },
  );

  await ensureCompatPackage(
    workspaceDir,
    "@oai/granola",
    packageJsonFor("@oai/granola", {
      ".": "./granola.mjs",
      "./models": "./granola.mjs",
      "./presentation-jsx": "./presentation-jsx.mjs",
      "./presentation-jsx/jsx-runtime": "./presentation-jsx-jsx-runtime.mjs",
      "./presentation-jsx/jsx-dev-runtime": "./presentation-jsx-jsx-dev-runtime.mjs",
      "./plugins/google-sheets": "./plugins-google-sheets.mjs",
      "./plugins/presentation-help": "./plugins-presentation-help.mjs",
      "./plugins/preset-shape-definitions": "./plugins-preset-shape-definitions.mjs",
      "./plugins/workbook-export": "./plugins-workbook-export.mjs",
      "./plugins/workbook-help": "./plugins-workbook-help.mjs",
    }),
    {
      "granola.mjs": granolaRootModule(granolaRepo),
      "presentation-jsx.mjs": presentationJsx,
      "presentation-jsx-jsx-runtime.mjs": presentationJsxRuntime,
      "presentation-jsx-jsx-dev-runtime.mjs": presentationJsxDevRuntime,
      "plugins-google-sheets.mjs": namedExportsModule(path.join(granolaRepo, "src/plugins/google-sheets.ts"), [
        "installGoogleSheetsPlugin",
      ]),
      "plugins-presentation-help.mjs": namedExportsModule(
        path.join(granolaRepo, "src/plugins/presentation-help.ts"),
        ["installPresentationHelp"],
      ),
      "plugins-preset-shape-definitions.mjs": namedExportsModule(
        path.join(granolaRepo, "src/plugins/preset-shape-definitions.ts"),
        ["installPresetShapeDefinitions"],
      ),
      "plugins-workbook-export.mjs": namedExportsModule(
        path.join(granolaRepo, "src/plugins/workbook-export.ts"),
        ["installWorkbookExport"],
      ),
      "plugins-workbook-help.mjs": namedExportsModule(path.join(granolaRepo, "src/plugins/workbook-help.ts"), [
        "installWorkbookHelp",
      ]),
    },
  );
}

export async function ensureGranolaWorkspace(workspaceDir) {
  assertLocalGranolaEnabled();
  const resolvedWorkspace = path.resolve(workspaceDir);
  const granolaRepo = resolveGranolaRepo();

  await fs.mkdir(resolvedWorkspace, { recursive: true });
  await ensureModulePackage(resolvedWorkspace);

  await ensureGranolaCompatPackages(resolvedWorkspace, granolaRepo);

  const walnutPackage = optionalPackageFromGranola(granolaRepo, "@oai/walnut");
  if (walnutPackage) {
    await ensureSymlink(workspacePackagePath(resolvedWorkspace, "@oai/walnut"), walnutPackage);
  }

  const lucidePackage = optionalPackageFromRuntime("lucide");
  if (lucidePackage) {
    await ensureSymlink(workspacePackagePath(resolvedWorkspace, "lucide"), lucidePackage);
  }

  return { workspaceDir: resolvedWorkspace, packageDir: granolaRepo };
}

async function importWithUsefulError(modulePath, label) {
  try {
    return await import(pathToFileURL(modulePath).href);
  } catch (error) {
    throw new Error(
      [
        `Failed to import ${label} from local Granola.`,
        `Path: ${modulePath}`,
        "Run through scripts/run_with_local_granola.mjs, or run the target script with `pnpm exec tsx` from the Granola repo.",
        error.stack || error.message || String(error),
      ].join("\n"),
    );
  }
}

async function loadWalnutExports(granolaRepo) {
  if (!walnutExportsPromise) {
    walnutExportsPromise = (async () => {
      const requireFromGranola = createRequire(path.join(granolaRepo, "package.json"));
      const wasmEntry = requireFromGranola.resolve("@oai/walnut/wasm/dotnet.js");
      const dotnetModule = await importWithUsefulError(wasmEntry, "@oai/walnut/wasm/dotnet.js");
      const wasmDir = path.dirname(wasmEntry);
      const bootJsonPath = path.join(wasmDir, "blazor.boot.json");
      const bootJsonRaw = await fs.readFile(bootJsonPath, "utf8");
      const config = JSON.parse(bootJsonRaw);

      const runtime = await dotnetModule.dotnet
        .withConfig(config)
        .withResourceLoader((type, _name, defaultUri) => {
          const resolvedUri =
            defaultUri.startsWith("file:") ||
            defaultUri.startsWith("http:") ||
            defaultUri.startsWith("https:") ||
            path.isAbsolute(defaultUri)
              ? defaultUri
              : path.join(wasmDir, defaultUri);

          if (type === "dotnetwasm") {
            if (
              resolvedUri.startsWith("http:") ||
              resolvedUri.startsWith("https:") ||
              typeof Response === "undefined"
            ) {
              return resolvedUri;
            }
            const wasmPath = resolvedUri.startsWith("file:") ? new URL(resolvedUri) : resolvedUri;
            return fs.readFile(wasmPath).then(
              (buffer) =>
                new Response(buffer, {
                  headers: { "Content-Type": "application/wasm" },
                }),
            );
          }

          return resolvedUri;
        })
        .create();

      const mainAssembly = runtime.getConfig().mainAssemblyName;
      if (!mainAssembly) {
        throw new Error("Walnut runtime did not provide a main assembly name.");
      }
      return runtime.getAssemblyExports(mainAssembly);
    })();
  }

  return walnutExportsPromise;
}

async function loadGranolaRuntime(workspaceDir) {
  if (cachedRuntime) return cachedRuntime;

  assertLocalGranolaEnabled();
  await ensureGranolaWorkspace(workspaceDir);
  const granolaRepo = resolveGranolaRepo();
  const requireFromGranola = createRequire(path.join(granolaRepo, "package.json"));

  const presentationModule = await importWithUsefulError(
    path.join(granolaRepo, "src/models/presentation/presentation.ts"),
    "Granola Presentation",
  );
  const Presentation = presentationModule.Presentation ?? presentationModule.default?.Presentation;
  if (!Presentation) {
    throw new Error("Local Granola Presentation export was not found.");
  }

  const setupModule = await importWithUsefulError(
    path.join(granolaRepo, "scripts/setup-skia-canvas-runtime.ts"),
    "Granola Skia runtime setup",
  );
  const setupSkiaCanvasRuntime =
    setupModule.setupSkiaCanvasRuntime ?? setupModule.default?.setupSkiaCanvasRuntime;
  if (setupSkiaCanvasRuntime) await setupSkiaCanvasRuntime();

  const presentationHelpModule = await importWithUsefulError(
    path.join(granolaRepo, "src/plugins/presentation-help.ts"),
    "Granola presentation help plugin",
  );
  const installPresentationHelp =
    presentationHelpModule.installPresentationHelp ?? presentationHelpModule.default?.installPresentationHelp;
  if (installPresentationHelp) installPresentationHelp();

  const workbookExportModule = await importWithUsefulError(
    path.join(granolaRepo, "src/plugins/workbook-export.ts"),
    "Granola workbook export plugin",
  );
  const installWorkbookExport =
    workbookExportModule.installWorkbookExport ?? workbookExportModule.default?.installWorkbookExport;
  if (installWorkbookExport) installWorkbookExport();

  const walnutPresentationPath = requireFromGranola.resolve("@oai/walnut/pptx/presentation");
  const walnutPresentationModule = await importWithUsefulError(
    walnutPresentationPath,
    "@oai/walnut/pptx/presentation",
  );
  const WalnutPresentationMessage = walnutPresentationModule.Presentation;
  if (!WalnutPresentationMessage) {
    throw new Error("Walnut Presentation message export was not found.");
  }

  cachedRuntime = {
    granolaRepo,
    Presentation,
    WalnutPresentationMessage,
  };
  console.error(`Using local Granola runtime from ${granolaRepo}`);
  return cachedRuntime;
}

function normalizeBytes(bytes) {
  return bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
}

function normalizeWalnutReaderInput(bytes) {
  return Array.from(normalizeBytes(bytes));
}

class GranolaFileBlob {
  constructor(filePath, bytes) {
    this.path = filePath;
    this.bytes = normalizeBytes(bytes);
  }

  static async load(filePath) {
    const resolved = path.resolve(filePath);
    return new GranolaFileBlob(resolved, await fs.readFile(resolved));
  }

  async arrayBuffer() {
    return this.bytes.buffer.slice(
      this.bytes.byteOffset,
      this.bytes.byteOffset + this.bytes.byteLength,
    );
  }

  async text() {
    return Buffer.from(this.bytes).toString("utf8");
  }

  async save(outputPath) {
    await fs.mkdir(path.dirname(path.resolve(outputPath)), { recursive: true });
    await fs.writeFile(path.resolve(outputPath), Buffer.from(this.bytes));
  }
}

async function importPresentationFromPptx(runtime, fileBlob) {
  const walnut = await loadWalnutExports(runtime.granolaRepo);
  const reader = walnut.PptxReader?.ExtractSlidesProto;
  if (!reader) {
    throw new Error("Walnut PPTX reader is unavailable.");
  }

  const protoBytes = reader(normalizeWalnutReaderInput(fileBlob.bytes), true);
  const proto = runtime.WalnutPresentationMessage.decode(normalizeBytes(protoBytes));
  return runtime.Presentation.load(proto);
}

async function exportPresentationToPptx(runtime, presentation) {
  const walnut = await loadWalnutExports(runtime.granolaRepo);
  const exporter = walnut.PptxExport?.ExportProtoToPptx;
  if (!exporter) {
    throw new Error("Walnut PPTX exporter is unavailable.");
  }

  const protoBytes = runtime.WalnutPresentationMessage.encode(presentation.toProto()).finish();
  return new GranolaFileBlob(undefined, exporter(protoBytes));
}

export async function importGranolaCompat(workspaceDir) {
  const runtime = await loadGranolaRuntime(workspaceDir || scriptDir());

  return {
    FileBlob: GranolaFileBlob,
    Presentation: runtime.Presentation,
    PresentationFile: {
      async importPptx(fileBlob) {
        return importPresentationFromPptx(runtime, fileBlob);
      },
      async exportPptx(presentation) {
        return exportPresentationToPptx(runtime, presentation);
      },
    },
  };
}
