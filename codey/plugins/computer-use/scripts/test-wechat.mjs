// Use pathToFileURL for ESM compatibility
import { pathToFileURL } from "url";
import { createRequire } from "module";
const require = createRequire(pathToFileURL(process.cwd() + "/dummy.mjs"));

// Load from the plugin's node_modules
const pluginDir = process.cwd();
const skyPath = pluginDir + "/node_modules/@oai/sky/dist/project/cua/sky_js/src/index.js";
const { sky } = await import(pathToFileURL(skyPath).href);

async function main() {
  console.log("=== Listing all apps ===");
  let apps;
  try {
    apps = await sky.list_apps();
    console.log(JSON.stringify(apps, null, 2));
  } catch (e) {
    console.error("Failed to list apps:", e.message);
    return;
  }
  
  // Find WeChat
  const wechat = apps.find(a => 
    a.displayName && (a.displayName.toLowerCase().includes("wechat") || a.displayName.includes("微信"))
  );
  
  if (wechat) {
    console.log("\n=== Found WeChat ===");
    console.log(JSON.stringify(wechat, null, 2));
    
    if (wechat.windows && wechat.windows.length > 0) {
      const win = wechat.windows[0];
      console.log("Window:", JSON.stringify(win, null, 2));
      
      await sky.activate_window({ window: win });
      console.log("Window activated!");
      
      const state = await sky.get_window_state({ 
        window: win, 
        include_screenshot: true,
        include_text: true 
      });
      
      console.log("Screenshots:", state.screenshots?.length);
      const tree = state.accessibility?.tree || "";
      console.log("Accessibility tree (first 3000 chars):");
      console.log(tree.substring(0, 3000));
      
      console.log("\n=== Searching for 小猪猪 ===");
      const contactLines = tree.split("\n").filter(l => l.includes("小猪猪"));
      if (contactLines.length > 0) {
        console.log("Found 小猪猪! Element:", contactLines);
      } else {
        console.log("小猪猪 not found in current view. Looking for search box...");
        const searchBoxLines = tree.split("\n").filter(l => 
          l.toLowerCase().includes("search") || l.includes("搜索")
        );
        console.log("Search UI elements:", searchBoxLines.slice(0, 10));
      }
    }
  } else {
    console.log("\nWeChat not found. Available apps:");
    apps.forEach(a => console.log(`  - ${a.displayName || "unnamed"} windows: ${a.windows?.length || 0}`));
    
    // Try launching WeChat
    const paths = [
      "WeChat",
      "C:\\Program Files (x86)\\Tencent\\WeChat\\WeChat.exe",
      "C:\\Program Files\\Tencent\\WeChat\\WeChat.exe"
    ];
    
    for (const p of paths) {
      try {
        console.log(`\nLaunching: ${p}`);
        await sky.launch_app({ app: p });
        console.log("Launched, waiting 8s...");
        await new Promise(r => setTimeout(r, 8000));
        
        apps = await sky.list_apps();
        const found = apps.find(a => 
          a.displayName && (a.displayName.toLowerCase().includes("wechat") || a.displayName.includes("微信"))
        );
        if (found && found.windows?.length > 0) {
          console.log("Found WeChat!");
          const win = found.windows[0];
          await sky.activate_window({ window: win });
          const state = await sky.get_window_state({ window: win, include_screenshot: true, include_text: true });
          console.log("Screenshots:", state.screenshots?.length);
          console.log("Accessibility tree (first 3000 chars):");
          console.log(state.accessibility?.tree?.substring(0, 3000));
          return;
        }
      } catch (e) {
        console.log(`Failed: ${e.message}`);
      }
    }
    console.log("Could not find or launch WeChat");
  }
}

main().catch(e => {
  console.error("Error:", e.message);
  console.error(e.stack?.substring(0, 1000));
});