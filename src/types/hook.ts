export interface HookListItem {
  id: string;
  event: string;
  sourceType: string;
  sourceName: string;
  sourcePath: string;
  command: string;
  enabled: boolean;
  matcher?: string | null;
}
