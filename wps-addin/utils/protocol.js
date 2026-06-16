/**
 * Protocol utilities for parsing and dispatching cn-codex commands.
 */

// Registry of command handlers keyed by method name.
const handlers = {};

/**
 * Register a handler for a method.
 * @param {string} method
 * @param {function(params: object): object|Promise<object>} handler
 */
function register(method, handler) {
  handlers[method] = handler;
}

/**
 * Dispatch an incoming command message to the registered handler.
 * Sends the response (or error) back via WsClient.
 * @param {object} msg - { id, method, params }
 */
async function dispatch(msg) {
  const handler = handlers[msg.method];
  if (!handler) {
    WsClient.sendError(msg.id, 'Unknown method: ' + msg.method, -32601);
    return;
  }

  try {
    const result = await handler(msg.params || {});
    WsClient.sendResponse(msg.id, result || { success: true });
  } catch (e) {
    const message = (e && e.message) ? e.message : String(e);
    WsClient.sendError(msg.id, message);
  }
}

window.Protocol = {
  register: register,
  dispatch: dispatch,
  handlers: handlers,
};
