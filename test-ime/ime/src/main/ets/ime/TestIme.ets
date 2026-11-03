import { inputMethodEngine, InputMethodExtensionContext } from '@kit.IMEKit';
import { display } from '@kit.ArkUI';
import { socket } from '@kit.NetworkKit';
import { BusinessError, commonEventManager, systemDateTime } from '@kit.BasicServicesKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { util } from '@kit.ArkTS';

const DOMAIN: number = 0xE0C5;
const TAG: string = 'TestIme';
export const DEFAULT_PORT: number = 8971;
/** A common event whose data is the control port to listen on instead. */
export const PORT_EVENT: string = 'org.openharmonyrs.testime.PORT';
const MAX_LINE_LENGTH: number = 64 * 1024;
const SESSION_LOG_INTERVAL_MS: number = 1000;
const PANEL_HEIGHT_RATIO: number = 0.3;

/** One control request, sent as a JSON line. */
export interface Command {
  id?: number;
  op: string;
  token?: string;
  text?: string;
  n?: number;
  start?: number;
  end?: number;
  action?: number;
  direction?: number;
}

export interface Reply {
  id: number;
  op: string;
  ok: boolean;
  value?: string;
  error?: string;
}

interface Hello {
  bundle: string;
  port: number;
}

interface EditorInfo {
  inputPattern: number;
  enterKeyType: number;
  isTextPreviewSupported: boolean;
}

interface State {
  attached: boolean;
  shown: boolean;
  securityMode: number;
  starts: number;
  lastTextLength: number;
}

/** Logs `message` publicly; never pass it text from the editor. */
export function log(message: string): void {
  hilog.info(DOMAIN, TAG, 'TESTIME %{public}s', message);
}

/** A random token for the control connection: two version 4 UUIDs from a secure generator, 244 random bits. hilogd
 * shows other processes' logs only to the shell and a few system services, so the host learns it from hilog and other
 * apps can't. */
function newToken(): string {
  let hex = '';
  for (let i = 0; i < 2; i++) {
    for (const byte of util.generateRandomBinaryUUID(false)) {
      hex += (byte < 16 ? '0' : '') + byte.toString(16);
    }
  }
  return hex;
}

function tokensEqual(a: string | undefined, b: string): boolean {
  if (a === undefined || a.length !== b.length || b === '') {
    return false;
  }
  let difference = 0;
  for (let i = 0; i < a.length; i++) {
    difference |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return difference === 0;
}

/** Returns the command, or an error message. */
function parseCommand(line: string): Command | string {
  let value: Object | null = null;
  try {
    value = JSON.parse(line) as Object | null;
  } catch (e) {
    return 'bad json';
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return 'a command must be a JSON object';
  }
  const command = value as Command;
  if (typeof command.op !== 'string') {
    return 'a command needs an op';
  }
  if (command.id !== undefined && typeof command.id !== 'number') {
    return 'id must be a number';
  }
  return command;
}

function requireSuccess(result: boolean, call: string): string {
  if (!result) {
    throw new Error(`${call} failed`);
  }
  return '';
}

/** A control connection. Its commands run one after the other. */
class Connection {
  socket: socket.TCPSocketConnection;
  authenticated: boolean = false;
  closed: boolean = false;
  pending: string = '';
  queue: Promise<void> = Promise.resolve();
  decoder: util.TextDecoder = util.TextDecoder.create('utf-8');

  constructor(conn: socket.TCPSocketConnection) {
    this.socket = conn;
  }
}

class TestIme {
  private ability: inputMethodEngine.InputMethodAbility = inputMethodEngine.getInputMethodAbility();
  private started: boolean = false;
  private bundle: string = '';
  private token: string = '';
  private sessionLoggedAt: number = 0;
  private port: number = DEFAULT_PORT;
  private listening: boolean = false;
  private subscriber?: commonEventManager.CommonEventSubscriber;
  private server?: socket.TCPSocketServer;
  private connections: Connection[] = [];
  private panel?: inputMethodEngine.Panel;
  private client?: inputMethodEngine.InputClient;
  private keyboard?: inputMethodEngine.KeyboardController;
  private shown: boolean = false;
  private starts: number = 0;
  private lastTextLength: number = 0;
  private privateCommands: Record<string, inputMethodEngine.CommandDataType>[] = [];
  private privateCommandError?: string;

  private onInputStart = (kb: inputMethodEngine.KeyboardController, client: inputMethodEngine.InputClient): void => {
    this.keyboard = kb;
    this.client = client;
    this.starts++;
    log(`inputStart #${this.starts}`);
    this.status(`attached (#${this.starts})`);
  };
  private onInputStop = (): void => {
    log('inputStop');
    this.client = undefined;
    this.keyboard = undefined;
    this.status('detached');
  };
  private onKeyboardShow = (): void => {
    this.shown = true;
    log('keyboardShow');
  };
  private onKeyboardHide = (): void => {
    this.shown = false;
    log('keyboardHide');
  };
  private onPrivateCommand = (command: Record<string, inputMethodEngine.CommandDataType>): void => {
    this.privateCommands.push(command);
    log(`privateCommand with ${Object.keys(command).length} entries`);
  };
  private onTextChange = (text: string): void => {
    this.lastTextLength = text.length;
    log(`textChange, ${text.length} chars`);
  };
  private onSelectionChange = (oldBegin: number, oldEnd: number, newBegin: number, newEnd: number): void => {
    log(`selectionChange ${oldBegin},${oldEnd} -> ${newBegin},${newEnd}`);
  };

  start(context: InputMethodExtensionContext): void {
    if (this.started) {
      this.stop();
    }
    this.started = true;
    log('onCreate');
    this.bundle = context.applicationInfo.name;
    this.token = newToken();
    this.ability.on('inputStart', this.onInputStart);
    this.ability.on('inputStop', this.onInputStop);
    this.ability.on('keyboardShow', this.onKeyboardShow);
    this.ability.on('keyboardHide', this.onKeyboardHide);
    try {
      // IMF allows this only for the system's preconfigured default IME.
      this.ability.on('privateCommand', this.onPrivateCommand);
      this.privateCommandError = undefined;
    } catch (e) {
      const error = e as BusinessError;
      this.privateCommandError = `${error.code ?? ''} ${error.message ?? String(e)}`;
      log(`privateCommand unavailable: ${this.privateCommandError}`);
    }
    const delegate = inputMethodEngine.getKeyboardDelegate();
    delegate.on('textChange', this.onTextChange);
    delegate.on('selectionChange', this.onSelectionChange);
    this.createPanel(context);
    this.listen(DEFAULT_PORT);
    this.subscribe();
  }

  stop(): void {
    log('onDestroy');
    this.started = false;
    if (this.subscriber !== undefined) {
      commonEventManager.unsubscribe(this.subscriber);
      this.subscriber = undefined;
    }
    this.closeServer();
    this.ability.off('inputStart', this.onInputStart);
    this.ability.off('inputStop', this.onInputStop);
    this.ability.off('keyboardShow', this.onKeyboardShow);
    this.ability.off('keyboardHide', this.onKeyboardHide);
    if (this.privateCommandError === undefined) {
      try {
        this.ability.off('privateCommand', this.onPrivateCommand);
      } catch (e) {
        log('privateCommand off failed');
      }
    }
    const delegate = inputMethodEngine.getKeyboardDelegate();
    delegate.off('textChange', this.onTextChange);
    delegate.off('selectionChange', this.onSelectionChange);
    if (this.panel !== undefined) {
      this.ability.destroyPanel(this.panel);
      this.panel = undefined;
    }
    this.client = undefined;
    this.keyboard = undefined;
  }

  private createPanel(context: InputMethodExtensionContext): void {
    const info: inputMethodEngine.PanelInfo = {
      type: inputMethodEngine.PanelType.SOFT_KEYBOARD,
      flag: inputMethodEngine.PanelFlag.FLG_FIXED
    };
    this.ability.createPanel(context, info).then((panel: inputMethodEngine.Panel) => {
      this.panel = panel;
      const d = display.getDefaultDisplaySync();
      panel.resize(d.width, Math.round(d.height * PANEL_HEIGHT_RATIO));
      panel.setUiContent('pages/KeyboardPanel');
      log(`panel created ${d.width}x${Math.round(d.height * PANEL_HEIGHT_RATIO)}`);
    }).catch((e: BusinessError) => log(`createPanel failed ${e.code} ${e.message}`));
  }

  /** Listens on the port of `PORT_EVENT` instead. Any app can send it: that ends the session but reveals nothing. */
  private subscribe(): void {
    const info: commonEventManager.CommonEventSubscribeInfo = { events: [PORT_EVENT] };
    commonEventManager.createSubscriber(info).then((subscriber: commonEventManager.CommonEventSubscriber) => {
      this.subscriber = subscriber;
      commonEventManager.subscribe(subscriber, (err: BusinessError, data: commonEventManager.CommonEventData) => {
        if (err) {
          log(`port event error ${err.code}`);
          return;
        }
        const port = Number(data.data ?? '');
        if (!Number.isInteger(port) || port <= 0 || port >= 65536) {
          log('ignored a port event without a valid port');
          return;
        }
        if (port !== this.port || !this.listening) {
          this.closeServer();
          this.listen(port);
        }
      });
    }).catch((e: BusinessError) => log(`subscribe failed ${e.code} ${e.message}`));
  }

  private logSession(): void {
    this.sessionLoggedAt = systemDateTime.getUptime(systemDateTime.TimeType.STARTUP);
    hilog.warn(DOMAIN, TAG, 'TESTIME session %{public}s port %{public}d', this.token, this.port);
  }

  private listen(port: number): void {
    const server: socket.TCPSocketServer = socket.constructTCPSocketServerInstance();
    this.server = server;
    this.port = port;
    this.listening = false;
    server.on('connect', (conn: socket.TCPSocketConnection) => this.accept(conn));
    const address: socket.NetAddress = { address: '127.0.0.1', port: port, family: 1 };
    server.listen(address)
      .then((): Promise<socket.NetAddress> => server.getLocalAddress())
      .then((bound: socket.NetAddress) => {
        // netstack binds a random port instead when the requested one is taken.
        if (bound.port !== port) {
          throw new Error(`port ${port} is taken`);
        }
        if (this.server === server) {
          this.listening = true;
          log(`listening on 127.0.0.1:${port}`);
          this.logSession();
        }
      })
      .catch((e: Error) => {
        log(`not listening: ${e.message}`);
        if (this.server === server) {
          this.closeServer();
        } else {
          server.close().catch((err: BusinessError) => log(`close failed ${err.code}`));
        }
      });
  }

  private closeServer(): void {
    this.closeConnections(true);
    const server: socket.TCPSocketServer | undefined = this.server;
    this.server = undefined;
    this.listening = false;
    if (server !== undefined) {
      server.off('connect');
      server.close().catch((e: BusinessError) => log(`close failed ${e.code} ${e.message}`));
    }
  }

  /** Closes the authenticated connections, or all of them. */
  private closeConnections(all: boolean): void {
    const remaining: Connection[] = [];
    for (const connection of this.connections) {
      if (all || connection.authenticated) {
        this.closeConnection(connection);
      } else {
        remaining.push(connection);
      }
    }
    this.connections = remaining;
  }

  private closeConnection(connection: Connection): void {
    if (!connection.closed) {
      connection.closed = true;
      connection.socket.close().catch((e: BusinessError) => log(`close failed ${e.code}`));
    }
  }

  private accept(conn: socket.TCPSocketConnection): void {
    log(`control connection ${conn.clientId}`);
    const connection = new Connection(conn);
    this.connections.push(connection);
    conn.on('close', () => {
      connection.closed = true;
      this.connections = this.connections.filter((c: Connection) => c !== connection);
    });
    conn.on('message', (info: socket.SocketMessageInfo) => {
      if (connection.closed) {
        return;
      }
      connection.pending += connection.decoder.decodeToString(new Uint8Array(info.message), { stream: true });
      let newline = connection.pending.indexOf('\n');
      while (newline >= 0) {
        const line = connection.pending.substring(0, newline);
        connection.pending = connection.pending.substring(newline + 1);
        connection.queue = connection.queue.then(() => this.handleLine(connection, line));
        newline = connection.pending.indexOf('\n');
      }
      if (connection.pending.length > MAX_LINE_LENGTH) {
        connection.pending = '';
        connection.queue = connection.queue.then(() => this.reject(connection, -1, 'line too long'));
      }
    });
  }

  private async handleLine(connection: Connection, line: string): Promise<void> {
    if (connection.closed) {
      return;
    }
    const command = parseCommand(line);
    if (typeof command === 'string') {
      if (connection.authenticated) {
        await this.send(connection, { id: -1, op: '', ok: false, error: command });
      } else {
        await this.reject(connection, -1, command);
      }
      return;
    }
    const id = command.id ?? 0;
    if (!connection.authenticated) {
      if (command.op !== 'hello' || !tokensEqual(command.token, this.token)) {
        // Again, in case the line left hilog's buffer; at most once a second, so other apps can't flood the log.
        const now = systemDateTime.getUptime(systemDateTime.TimeType.STARTUP);
        if (now - this.sessionLoggedAt >= SESSION_LOG_INTERVAL_MS) {
          this.logSession();
        }
        await this.reject(connection, id, `unauthorized: test IME ${this.bundle} needs the token from hilog`);
        return;
      }
      connection.authenticated = true;
      const hello: Hello = { bundle: this.bundle, port: this.port };
      await this.send(connection, { id: id, op: 'hello', ok: true, value: JSON.stringify(hello) });
      return;
    }
    if (command.op === 'bye') {
      // The session's token stops working; the next session reads the new one from hilog.
      this.token = newToken();
      this.logSession();
      await this.send(connection, { id: id, op: 'bye', ok: true, value: '' });
      this.closeConnections(true);
      return;
    }
    await this.send(connection, await this.run(command));
  }

  private async send(connection: Connection, reply: Reply): Promise<void> {
    if (connection.closed) {
      return;
    }
    const data: socket.TCPSendOptions = { data: JSON.stringify(reply) + '\n' };
    await connection.socket.send(data).catch((e: BusinessError) => log(`send failed ${e.code} ${e.message}`));
  }

  private async reject(connection: Connection, id: number, error: string): Promise<void> {
    await this.send(connection, { id: id, op: '', ok: false, error: error });
    this.closeConnection(connection);
  }

  async run(command: Command): Promise<Reply> {
    const id = command.id ?? 0;
    try {
      const value = await this.execute(command);
      log(`${command.op} ok`);
      return { id: id, op: command.op, ok: true, value: value };
    } catch (e) {
      const error = e as BusinessError;
      const message = `${error.code ?? ''} ${error.message ?? String(e)}`.trim();
      log(`${command.op} failed ${message}`);
      return { id: id, op: command.op, ok: false, error: message };
    }
  }

  private requireClient(): inputMethodEngine.InputClient {
    if (this.client === undefined) {
      throw new Error('no input client attached');
    }
    return this.client;
  }

  /**
   * Runs `c` and returns its value.
   * @throws { Error } when no input client is attached, or IMF or the editor refuses the command; `run` reports it.
   */
  private async execute(c: Command): Promise<string> {
    switch (c.op) {
      case 'state': {
        const state: State = {
          attached: this.client !== undefined,
          shown: this.shown,
          securityMode: this.ability.getSecurityMode(),
          starts: this.starts,
          lastTextLength: this.lastTextLength
        };
        return JSON.stringify(state);
      }
      case 'insert':
        return requireSuccess(await this.requireClient().insertText(c.text ?? ''), 'insertText');
      case 'backspace':
        // IMF's "forward" is towards the start of the text: deleteForward is Backspace.
        return requireSuccess(await this.requireClient().deleteForward(c.n ?? 1), 'deleteForward');
      case 'delete':
        return requireSuccess(await this.requireClient().deleteBackward(c.n ?? 1), 'deleteBackward');
      case 'before':
        return await this.requireClient().getForward(c.n ?? 1000);
      case 'after':
        return await this.requireClient().getBackward(c.n ?? 1000);
      case 'cursor':
        return String(await this.requireClient().getTextIndexAtCursor());
      case 'select': {
        const range: inputMethodEngine.Range = { start: c.start ?? 0, end: c.end ?? 0 };
        await this.requireClient().selectByRange(range);
        return '';
      }
      case 'preview': {
        const range: inputMethodEngine.Range = { start: c.start ?? -1, end: c.end ?? -1 };
        await this.requireClient().setPreviewText(c.text ?? '', range);
        return '';
      }
      case 'finishPreview':
        await this.requireClient().finishTextPreview();
        return '';
      case 'key':
        return requireSuccess(await this.requireClient().sendKeyFunction(c.action ?? 0), 'sendKeyFunction');
      case 'enter': {
        // The editor's own Enter key type: editors ignore ENTER_KEY_TYPE_UNSPECIFIED (0) unless they report it.
        const client = this.requireClient();
        const enterKeyType = (await client.getEditorAttribute()).enterKeyType;
        requireSuccess(await client.sendKeyFunction(enterKeyType), 'sendKeyFunction');
        return String(enterKeyType);
      }
      case 'move':
        await this.requireClient().moveCursor(c.direction ?? 0);
        return '';
      case 'extend':
        await this.requireClient().sendExtendAction(c.action ?? 0);
        return '';
      case 'attr': {
        const attr = await this.requireClient().getEditorAttribute();
        const info: EditorInfo = {
          inputPattern: attr.inputPattern,
          enterKeyType: attr.enterKeyType,
          isTextPreviewSupported: attr.isTextPreviewSupported ?? false
        };
        return JSON.stringify(info);
      }
      case 'privateCommands': {
        if (this.privateCommandError !== undefined) {
          throw new Error(`private commands unavailable: ${this.privateCommandError}`);
        }
        const received = this.privateCommands;
        this.privateCommands = [];
        return JSON.stringify(received);
      }
      case 'hide':
        if (this.keyboard === undefined) {
          throw new Error('no keyboard controller');
        }
        await this.keyboard.hide();
        return '';
      default:
        throw new Error(`unknown op ${c.op}`);
    }
  }

  private status(text: string): void {
    AppStorage.setOrCreate<string>('testImeStatus', text);
  }
}

export const testIme: TestIme = new TestIme();
