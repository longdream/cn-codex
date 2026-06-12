# Tab Capability: cdp

Raw Chrome DevTools Protocol access through a Browser Use tab. This capability is only advertised when full CDP access is enabled. It exposes raw CDP method invocation through the debugger session attached to the selected tab. The browser backend may reject methods or targets it cannot route. Individual CDP methods may act on broader browser targets or browser state beyond that page.

```ts
const capability = await tab.capabilities.get("cdp");

type CdpEventsOptions = {
  afterSequence?: number;
  limit?: number;
  methods?: Array<string>;
  target?: CdpTarget;
  timeoutMs?: number;
};

type CdpCommandParams = Record<string, unknown>;

type CdpSendOptions = {
  target?: CdpTarget;
  timeoutMs?: number;
};

type CdpTarget = { sessionId: string; targetId?: never } | { sessionId?: never; targetId: string };

interface CdpTabCapability {
  readEvents(options?: CdpEventsOptions): Promise<{ cursor: number; events: Array<{ method: string; params?: Record<string, unknown>; sequence: number; source: { extensionId?: string; sessionId?: string; tabId?: number; targetId?: string } }>; hasMore: boolean; truncated: boolean }>; // Read DevTools Protocol events for this tab. Call this with no options to capture a cursor before triggering an action, then pass that cursor back through `afterSequence` to retrieve events that arrived afterward. When `timeoutMs` is provided and no matching buffered event is ready, Browser Use waits up to that duration for the first match. `truncated` means older buffered events were evicted; `hasMore` means this read hit `limit` and another read from `cursor` can continue. Cursors are positions in this tab's event stream, not filter-specific bookmarks, so continue polling with the same `methods` and `target` filters when you need exhaustive reads. When provided, `methods` must contain at least one CDP event name and `limit` must be between 1 and 1000. Unlike `send()`, `target` here filters buffered events and can still match a child target after that target detaches. Use `Target.attachedToTarget` events to discover child selectors when the backend exposes them.
  send(method: string, params?: CdpCommandParams, options?: CdpSendOptions): Promise<unknown>; // Send a DevTools Protocol command through this tab's debugger session. The backend decides which methods it can route and the scope of each method. Some protocol domains and methods can affect targets or browser state beyond the attached page. Nested target selectors may address only child debugger targets Browser Use already tracks for this tab. Use `Target.attachedToTarget` events to discover child selectors when the backend exposes them. Raw `Fetch.enable` and `Fetch.disable` interception is rejected by this capability.
}
```
