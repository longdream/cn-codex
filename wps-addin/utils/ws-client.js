/**
 * WebSocket client with auto-reconnect for communicating with cn-codex.
 */

const DEFAULT_URL = 'ws://127.0.0.1:23300/wps';
const RECONNECT_INTERVAL_MS = 3000;
const MAX_RECONNECT_INTERVAL_MS = 30000;

let ws = null;
let reconnectTimer = null;
let reconnectInterval = RECONNECT_INTERVAL_MS;
let isConnected = false;
let connId = null;

const listeners = {
  onConnect: null,
  onDisconnect: null,
  onCommand: null,
};

function getWsUrl() {
  if (typeof wps !== 'undefined' && wps.PluginStorage) {
    const custom = wps.PluginStorage.getItem('ws_url');
    if (custom) return custom;
  }
  return DEFAULT_URL;
}

function connect() {
  if (ws && (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN)) {
    return;
  }

  const url = getWsUrl();
  console.log('[cn-codex] Connecting to', url);

  try {
    ws = new WebSocket(url);
  } catch (e) {
    console.error('[cn-codex] WebSocket creation failed:', e);
    scheduleReconnect();
    return;
  }

  ws.onopen = function () {
    console.log('[cn-codex] Connected');
    reconnectInterval = RECONNECT_INTERVAL_MS;
    sendHandshake();
  };

  ws.onmessage = function (event) {
    let msg;
    try {
      msg = JSON.parse(event.data);
    } catch (e) {
      console.warn('[cn-codex] Invalid JSON:', event.data);
      return;
    }

    if (msg.type === 'handshake_ack') {
      connId = msg.connId;
      isConnected = true;
      console.log('[cn-codex] Handshake OK, connId:', connId);
      if (listeners.onConnect) listeners.onConnect(connId);
      return;
    }

    // It's a command request from cn-codex.
    if (msg.id && msg.method) {
      if (listeners.onCommand) {
        listeners.onCommand(msg);
      }
    }
  };

  ws.onerror = function (err) {
    console.error('[cn-codex] WebSocket error:', err);
  };

  ws.onclose = function () {
    console.log('[cn-codex] Disconnected');
    isConnected = false;
    connId = null;
    if (listeners.onDisconnect) listeners.onDisconnect();
    scheduleReconnect();
  };
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  console.log('[cn-codex] Reconnecting in', reconnectInterval, 'ms');
  reconnectTimer = setTimeout(function () {
    reconnectTimer = null;
    connect();
    reconnectInterval = Math.min(reconnectInterval * 1.5, MAX_RECONNECT_INTERVAL_MS);
  }, reconnectInterval);
}

function sendHandshake() {
  let activeDoc = null;
  try {
    if (typeof wps !== 'undefined') {
      const doc = wps.Application.ActiveDocument;
      if (doc) {
        activeDoc = {
          name: doc.Name || '',
          path: doc.FullName || '',
          saved: doc.Saved !== false,
        };
      }
    }
  } catch (_) {
    // No active document.
  }

  const handshake = {
    type: 'handshake',
    addinName: 'cn-codex-wps',
    addinVersion: '0.1.0',
    activeDocument: activeDoc,
  };

  try {
    if (typeof wps !== 'undefined') {
      handshake.wpsVersion = wps.Application.Build || '';
    }
  } catch (_) {}

  send(JSON.stringify(handshake));
}

function send(data) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(data);
  }
}

function sendResponse(id, result) {
  send(JSON.stringify({ id: id, result: result }));
}

function sendError(id, message, code) {
  send(JSON.stringify({ id: id, error: { code: code || -1, message: message } }));
}

function sendNotification(method, params) {
  send(JSON.stringify({ method: method, params: params }));
}

function disconnect() {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  if (ws) {
    ws.onclose = null;
    ws.close();
    ws = null;
  }
  isConnected = false;
  connId = null;
}

// Public API exposed as a global module.
window.WsClient = {
  connect: connect,
  disconnect: disconnect,
  send: send,
  sendResponse: sendResponse,
  sendError: sendError,
  sendNotification: sendNotification,
  isConnected: function () { return isConnected; },
  getConnId: function () { return connId; },
  on: function (event, handler) {
    if (listeners.hasOwnProperty(event)) {
      listeners[event] = handler;
    }
  },
};
