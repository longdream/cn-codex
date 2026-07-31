import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync("e:/work/RustWorks/cn-codex/codey/usage.db", { readOnly: true });
const row = db.prepare("SELECT value FROM app_state WHERE key='providers'").get();
const providers = JSON.parse(row.value);
for (const p of providers) {
  console.log("===");
  console.log("id:", p.id);
  console.log("type:", p.type, "| name:", p.name, "| category:", p.category);
  console.log("baseUrl:", p.baseUrl);
  console.log("apiKey:", String(p.apiKey || "").slice(0, 12) + "...");
  console.log("wireApi:", p.wireApi, "| requiresOpenAIAuth:", p.requiresOpenAIAuth);
  console.log("models:", JSON.stringify((p.models || []).map((m) => m.id)));
}
db.close();
