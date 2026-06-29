import * as echarts from "echarts";
import type { EChartsOption } from "echarts";
import { jsonrepair } from "jsonrepair";
import { useEffect, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { CodeBlock } from "./CodeBlock";

interface ChartBlockProps {
  source: unknown;
  title?: string;
}

interface ParsedOptionResult {
  option: EChartsOption | null;
  fallbackCode: string;
}

function parseChartOption(source: unknown): ParsedOptionResult {
  const fallbackCode = fallbackCodeForSource(source);

  if (typeof source === "string") {
    const raw = source.trim();
    if (!raw) {
      return { option: null, fallbackCode: "" };
    }

    const parsed = parseJsonObject(raw);
    return parsed
      ? { option: parsed, fallbackCode: raw }
      : { option: null, fallbackCode: raw };
  }

  if (source && typeof source === "object" && !Array.isArray(source)) {
    return { option: source as EChartsOption, fallbackCode };
  }

  return { option: null, fallbackCode };
}

function parseJsonObject(raw: string): EChartsOption | null {
  try {
    return normalizeParsedOption(JSON.parse(raw));
  } catch {
    try {
      return normalizeParsedOption(JSON.parse(jsonrepair(raw)));
    } catch {
      return null;
    }
  }
}

function normalizeParsedOption(parsed: unknown): EChartsOption | null {
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return null;
  }
  return parsed as EChartsOption;
}

function fallbackCodeForSource(source: unknown): string {
  if (typeof source === "string") {
    return source.trim();
  }
  if (source && typeof source === "object") {
    try {
      return JSON.stringify(source, null, 2);
    } catch {
      return "";
    }
  }
  return "";
}

export function ChartBlock({ source, title }: ChartBlockProps) {
  const intl = useIntl();
  const hostRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<ReturnType<typeof echarts.init> | null>(null);
  const [renderError, setRenderError] = useState<string | null>(null);
  const parsed = useMemo(() => parseChartOption(source), [source]);

  useEffect(() => {
    if (!parsed.option) {
      setRenderError(null);
      chartRef.current?.dispose();
      chartRef.current = null;
      return;
    }

    const host = hostRef.current;
    if (!host) {
      return;
    }

    let chart = chartRef.current;
    if (!chart) {
      chart = echarts.init(host);
      chartRef.current = chart;
    }

    try {
      chart.setOption(parsed.option, { notMerge: true, lazyUpdate: false });
      setRenderError(null);
    } catch (error) {
      setRenderError(error instanceof Error ? error.message : String(error));
    }
  }, [parsed.option]);

  useEffect(() => {
    return () => {
      chartRef.current?.dispose();
      chartRef.current = null;
    };
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) {
      return;
    }

    const resize = () => chartRef.current?.resize();
    if (typeof ResizeObserver !== "undefined") {
      const observer = new ResizeObserver(() => resize());
      observer.observe(host);
      return () => observer.disconnect();
    }

    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);

  if (!parsed.option) {
    return (
      <div className="my-3 space-y-2">
        <p className="rounded-[var(--radius-sm)] border border-[var(--danger-soft)] bg-[var(--danger-soft)] px-2.5 py-2 text-[11px] text-[var(--danger)]">
          {intl.formatMessage({ id: "tool.echartsInvalidOption" })}
        </p>
        <CodeBlock code={parsed.fallbackCode || "{}"} language="json" />
      </div>
    );
  }

  return (
    <div className="my-3 overflow-hidden rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-main)]">
      {title && (
        <div className="border-b border-[var(--chat-line)] px-3 py-2 text-[12px] font-medium text-[var(--chat-prose)]">
          {title}
        </div>
      )}
      {renderError && (
        <p className="border-b border-[var(--danger-soft)] bg-[var(--danger-soft)] px-3 py-2 text-[11px] text-[var(--danger)]">
          {intl.formatMessage({ id: "tool.echartsRenderFailed" })}
        </p>
      )}
      <div ref={hostRef} className="h-[320px] w-full min-w-0" />
    </div>
  );
}
