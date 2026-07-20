import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useState } from "react";

/**
 * 读取 Tauri 应用版本号（与 Cargo.toml / 更新检查同源）。
 * 非 Tauri 环境下保留 fallback，避免开发预览时报错。
 */
export function useAppVersion(fallback = "—"): string {
  const [version, setVersion] = useState(fallback);

  useEffect(() => {
    let cancelled = false;
    getVersion()
      .then((value) => {
        const next = value.trim();
        if (!cancelled && next) {
          setVersion(next);
        }
      })
      .catch(() => {
        // Keep the fallback when running outside Tauri runtime.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return version;
}
