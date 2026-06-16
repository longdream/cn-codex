import { Buffer } from "node:buffer";
import { endianness, platform } from "node:os";

import { WindowsComputerUseClientBase } from "../node_modules/@oai/sky/dist/project/cua/sky_js/src/targets/windows/internal/computer_use_client_base.js";

const FRAME_HEADER_BYTES = 4;
const COMPUTER_USE_APPROVAL_REQUEST_METHOD = "requestComputerUseApproval";
let activeSkyClient = null;

export async function setupComputerUseRuntime({ globals = globalThis } = {}) {
  if (platform() !== "win32") {
    const { sky } = await import("@oai/sky");
    globals.sky = sky;
    return sky;
  }

  await activeSkyClient?.close().catch(() => {});
  activeSkyClient = new NativePipeComputerUseClient({
    transport: await NativePipeComputerUseTransport.create(),
  });
  globals.sky = activeSkyClient;
  return activeSkyClient;
}

class NativePipeComputerUseClient extends WindowsComputerUseClientBase {
  async close() {
    try {
      await super.close();
    } finally {
      if (activeSkyClient === this) {
        activeSkyClient = null;
      }
    }
  }
}

class NativePipeComputerUseTransport {
  static async create() {
    if (platform() !== "win32") {
      throw new Error("Computer Use native pipe is only available on Windows");
    }
    const nativePipe = globalThis.nodeRepl?.nativePipe;
    if (
      nativePipe == null ||
      typeof nativePipe.createConnection !== "function"
    ) {
      throw new Error("Computer Use native pipe is unavailable");
    }

    const pipePath = getComputerUsePipePath();
    let transport = null;
    try {
      const socket = await nativePipe.createConnection(pipePath);
      transport = new NativePipeComputerUseTransport(socket);
      await transport.request("list_windows", {});
      return transport;
    } catch (error) {
      await transport?.close().catch(() => {});
      throw new Error(
        `Computer Use native pipe is unavailable: ${formatErrorMessage(error)}`,
      );
    }
  }

  constructor(socket) {
    this.nextRequestId = 1;
    this.pendingData = Buffer.alloc(0);
    this.pendingRequests = new Map();
    this.socket = socket;
    socket.on("data", (chunk) => this.handleData(chunk));
    socket.on("error", (error) => this.rejectPendingRequests(error));
    socket.on("close", () =>
      this.rejectPendingRequests(
        new Error("Computer Use native pipe closed before response"),
      ),
    );
  }

  async close() {
    if (this.socket == null) {
      return;
    }
    await this.sendMessage("close", {});
    this.socket.end();
    this.socket = null;
  }

  request(method, params, options = {}) {
    return this.sendMessage("request", {
      codexTurnMetadata: options.codexTurnMetadata,
      method,
      params,
    });
  }

  sendMessage(method, params) {
    if (this.socket == null) {
      throw new Error("Computer Use native pipe is closed");
    }

    const id = this.nextRequestId;
    this.nextRequestId += 1;
    const payload = JSON.stringify({
      id,
      jsonrpc: "2.0",
      method,
      params,
    });
    this.socket.write(encodeMessageFrame(payload));
    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { reject, resolve });
    });
  }

  handleData(chunk) {
    this.pendingData = Buffer.concat([this.pendingData, Buffer.from(chunk)]);
    const decoded = decodeMessageFrames(this.pendingData);
    this.pendingData = decoded.remainingData;

    for (const message of decoded.messages) {
      this.handleMessage(JSON.parse(message));
    }
  }

  handleMessage(message) {
    if (
      message.method === COMPUTER_USE_APPROVAL_REQUEST_METHOD &&
      message.id != null
    ) {
      void this.handleApprovalRequest(message);
      return;
    }

    const request =
      typeof message.id === "number"
        ? this.pendingRequests.get(message.id)
        : undefined;
    if (request == null) {
      return;
    }
    this.pendingRequests.delete(message.id);
    if (message.error != null) {
      request.reject(new Error(message.error.message));
      return;
    }
    request.resolve(message.result);
  }

  async handleApprovalRequest(message) {
    try {
      const createElicitation = globalThis.nodeRepl?.createElicitation;
      if (typeof createElicitation !== "function") {
        throw new Error("Computer Use app approval UI is unavailable");
      }
      this.writeMessage({
        id: message.id,
        jsonrpc: "2.0",
        result: await createElicitation(message.params),
      });
    } catch (error) {
      this.writeMessage({
        error: {
          code: -32000,
          message: error instanceof Error ? error.message : String(error),
        },
        id: message.id,
        jsonrpc: "2.0",
      });
    }
  }

  rejectPendingRequests(error) {
    for (const request of this.pendingRequests.values()) {
      request.reject(error);
    }
    this.pendingRequests.clear();
  }

  writeMessage(message) {
    if (this.socket == null) {
      return;
    }
    this.socket.write(encodeMessageFrame(JSON.stringify(message)));
  }
}

function getComputerUsePipePath() {
  const nativePipeDirectory =
    globalThis.nodeRepl?.env?.SKY_CUA_NATIVE_PIPE_DIRECTORY;
  if (typeof nativePipeDirectory === "string") {
    const trimmed = nativePipeDirectory.trim();
    if (trimmed) {
      return trimmed;
    }
  }

  throw new Error("Computer Use native pipe path is unavailable");
}

function formatErrorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function encodeMessageFrame(message) {
  const payload = Buffer.from(message, "utf8");
  const frame = Buffer.alloc(FRAME_HEADER_BYTES + payload.length);
  if (endianness() === "LE") {
    frame.writeUInt32LE(payload.length, 0);
  } else {
    frame.writeUInt32BE(payload.length, 0);
  }
  payload.copy(frame, FRAME_HEADER_BYTES);
  return frame;
}

function decodeMessageFrames(buffer) {
  const messages = [];
  let offset = 0;

  while (buffer.length - offset >= FRAME_HEADER_BYTES) {
    const payloadLength =
      endianness() === "LE"
        ? buffer.readUInt32LE(offset)
        : buffer.readUInt32BE(offset);
    const frameLength = FRAME_HEADER_BYTES + payloadLength;
    if (buffer.length - offset < frameLength) {
      break;
    }
    messages.push(
      buffer
        .subarray(offset + FRAME_HEADER_BYTES, offset + frameLength)
        .toString("utf8"),
    );
    offset += frameLength;
  }

  return {
    messages,
    remainingData: buffer.subarray(offset),
  };
}
