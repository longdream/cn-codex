/** 按 1-based 行号计算编辑器滚动偏移；用滚动容器可视高度，避免 absolute textarea 全高把目标顶到 0。 */
export function computeDetailLineScrollTop(options: {
  line: number;
  lineHeight: number;
  paddingTop: number;
  viewportHeight: number;
}): number {
  const safeLine = Math.max(1, Math.floor(options.line));
  const lineHeight =
    Number.isFinite(options.lineHeight) && options.lineHeight > 0 ? options.lineHeight : 20;
  const paddingTop = Number.isFinite(options.paddingTop) ? Math.max(0, options.paddingTop) : 0;
  const viewportHeight =
    Number.isFinite(options.viewportHeight) && options.viewportHeight > 0
      ? options.viewportHeight
      : 0;
  return Math.max(0, (safeLine - 1) * lineHeight + paddingTop - viewportHeight / 3);
}
