/**
 * CN-Codex WPS Add-in entry point.
 *
 * This add-in connects to the cn-codex desktop app via WebSocket and
 * executes document manipulation commands received from the AI agent.
 */

// ---- Ribbon callback ----

function OnShowStatus() {
  var connected = WsClient.isConnected();
  var connId = WsClient.getConnId();
  var msg = connected
    ? '已连接到 CN-Codex\n连接 ID: ' + connId
    : '未连接到 CN-Codex\n正在尝试自动重连...';
  alert(msg);
}

// ---- Document event handlers ----

function onDocumentOpen(doc) {
  if (!WsClient.isConnected()) return;
  WsClient.sendNotification('event.documentOpened', {
    name: doc.Name,
    path: doc.FullName,
    saved: doc.Saved,
  });
}

function onDocumentBeforeSave(doc) {
  if (!WsClient.isConnected()) return;
  WsClient.sendNotification('event.documentBeforeSave', {
    name: doc.Name,
    path: doc.FullName,
  });
}

function onDocumentBeforeClose(doc) {
  if (!WsClient.isConnected()) return;
  WsClient.sendNotification('event.documentBeforeClose', {
    name: doc.Name,
    path: doc.FullName,
  });
}

function onDocumentChange() {
  if (!WsClient.isConnected()) return;
  try {
    var doc = wps.Application.ActiveDocument;
    if (doc) {
      WsClient.sendNotification('event.documentChanged', {
        name: doc.Name,
        path: doc.FullName,
        saved: doc.Saved,
      });
    }
  } catch (_) {}
}

// ---- Initialization ----

function onLoad() {
  // Register command dispatch handler.
  WsClient.on('onCommand', function (msg) {
    Protocol.dispatch(msg);
  });

  WsClient.on('onConnect', function (connId) {
    console.log('[cn-codex] Add-in connected with id:', connId);
  });

  WsClient.on('onDisconnect', function () {
    console.log('[cn-codex] Add-in disconnected');
  });

  // Register WPS document events.
  try {
    wps.ApiEvent.AddApiEventListener('DocumentOpen', onDocumentOpen);
    wps.ApiEvent.AddApiEventListener('DocumentBeforeSave', onDocumentBeforeSave);
    wps.ApiEvent.AddApiEventListener('DocumentBeforeClose', onDocumentBeforeClose);
    wps.ApiEvent.AddApiEventListener('DocumentChange', onDocumentChange);
  } catch (e) {
    console.warn('[cn-codex] Failed to register WPS events:', e);
  }

  // Start WebSocket connection.
  WsClient.connect();
}

// Entry point called by WPS on add-in load.
onLoad();
