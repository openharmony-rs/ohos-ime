import { inputMethodEngine, InputMethodExtensionContext } from '@kit.IMEKit';
import { display } from '@kit.ArkUI';
import { socket } from '@kit.NetworkKit';
import { BusinessError, commonEventManager } from '@kit.BasicServicesKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { util } from '@kit.ArkTS';

const DOMAIN: number = 0xE0C5;
const TAG: string = 'TestIme';
export const CONTROL_PORT: number = 8971;
export const CONTROL_EVENT: string = 'org.openharmonyrs.testime.CMD';
const PANEL_HEIGHT_RATIO: number = 0.3;

/** One control request, sent as a JSON line over TCP or as (base64) JSON in a common event. */
export interface Command {
  id?: number;
  op: string;
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
  lastText: string;
}

export function log(message: string): void {
  hilog.info(DOMAIN, TAG, 'TESTIME %{public}s', message);
}

class TestIme {
  private ability: inputMethodEngine.InputMethodAbility = inputMethodEngine.getInputMethodAbility();
  private panel?: inputMethodEngine.Panel;
  private client?: inputMethodEngine.InputClient;
  private keyboard?: inputMethodEngine.KeyboardController;
  private server?: socket.TCPSocketServer;
  private subscriber?: commonEventManager.CommonEventSubscriber;
  private shown: boolean = false;
  private starts: number = 0;
  private lastText: string = '';
  private privateCommands: Record<string, inputMethodEngine.CommandDataType>[] = [];
  private privateCommandError?: string;

  start(context: InputMethodExtensionContext): void {
    log('onCreate');
    this.ability.on('inputStart', (kb: inputMethodEngine.KeyboardController, client: inputMethodEngine.InputClient) => {
      this.keyboard = kb;
      this.client = client;
      this.starts++;
      log(`inputStart #${this.starts}`);
      this.status(`attached (#${this.starts})`);
    });
    this.ability.on('inputStop', () => {
      log('inputStop');
      this.client = undefined;
      this.keyboard = undefined;
      this.status('detached');
    });
    this.ability.on('keyboardShow', () => {
      this.shown = true;
      log('keyboardShow');
    });
    this.ability.on('keyboardHide', () => {
      this.shown = false;
      log('keyboardHide');
    });
    try {
      // IMF allows this only for the system's preconfigured default IME.
      this.ability.on('privateCommand', (command: Record<string, inputMethodEngine.CommandDataType>) => {
        this.privateCommands.push(command);
        log(`privateCommand ${JSON.stringify(command)}`);
      });
    } catch (e) {
      const error = e as BusinessError;
      this.privateCommandError = `${error.code ?? ''} ${error.message ?? String(e)}`;
      log(`privateCommand unavailable: ${this.privateCommandError}`);
    }
    const delegate = inputMethodEngine.getKeyboardDelegate();
    delegate.on('textChange', (text: string) => {
      this.lastText = text;
      log(`textChange ${JSON.stringify(text)}`);
    });
    delegate.on('selectionChange', (oldBegin: number, oldEnd: number, newBegin: number, newEnd: number) => {
      log(`selectionChange ${oldBegin},${oldEnd} -> ${newBegin},${newEnd}`);
    });
    this.createPanel(context);
    this.listen();
    this.subscribe();
  }

  stop(): void {
    log('onDestroy');
    this.server?.off('connect');
    if (this.subscriber !== undefined) {
      commonEventManager.unsubscribe(this.subscriber);
    }
    if (this.panel !== undefined) {
      this.ability.destroyPanel(this.panel);
    }
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

  private listen(): void {
    const server = socket.constructTCPSocketServerInstance();
    this.server = server;
    server.on('connect', (conn: socket.TCPSocketConnection) => {
      log(`control connection ${conn.clientId}`);
      const decoder = util.TextDecoder.create('utf-8');
      let pending = '';
      conn.on('message', (info: socket.SocketMessageInfo) => {
        pending += decoder.decodeToString(new Uint8Array(info.message), { stream: true });
        let newline = pending.indexOf('\n');
        while (newline >= 0) {
          const line = pending.substring(0, newline);
          pending = pending.substring(newline + 1);
          this.handleLine(line).then((reply: Reply) => {
            const data: socket.TCPSendOptions = { data: JSON.stringify(reply) + '\n' };
            conn.send(data).catch((e: BusinessError) => log(`send failed ${e.code} ${e.message}`));
          });
          newline = pending.indexOf('\n');
        }
      });
    });
    const address: socket.NetAddress = { address: '127.0.0.1', port: CONTROL_PORT, family: 1 };
    server.listen(address)
      .then(() => log(`listening on 127.0.0.1:${CONTROL_PORT}`))
      .catch((e: BusinessError) => log(`listen failed ${e.code} ${e.message}`));
  }

  private subscribe(): void {
    const info: commonEventManager.CommonEventSubscribeInfo = { events: [CONTROL_EVENT] };
    commonEventManager.createSubscriber(info).then((subscriber: commonEventManager.CommonEventSubscriber) => {
      this.subscriber = subscriber;
      commonEventManager.subscribe(subscriber, (err: BusinessError, data: commonEventManager.CommonEventData) => {
        if (err) {
          log(`event error ${err.code} ${err.message}`);
          return;
        }
        let payload = data.data ?? '';
        if (!payload.startsWith('{')) {
          const bytes = new util.Base64Helper().decodeSync(payload);
          payload = util.TextDecoder.create('utf-8').decodeToString(bytes);
        }
        this.handleLine(payload).then((reply: Reply) => log(`REPLY ${JSON.stringify(reply)}`));
      });
      log(`subscribed to ${CONTROL_EVENT}`);
    }).catch((e: BusinessError) => log(`subscribe failed ${e.code} ${e.message}`));
  }

  private async handleLine(line: string): Promise<Reply> {
    let command: Command;
    try {
      command = JSON.parse(line) as Command;
    } catch (e) {
      return { id: -1, op: '', ok: false, error: `bad json: ${line}` };
    }
    return this.run(command);
  }

  async run(command: Command): Promise<Reply> {
    const id = command.id ?? 0;
    try {
      const value = await this.execute(command);
      log(`${command.op} ok ${value}`);
      return { id: id, op: command.op, ok: true, value: value };
    } catch (e) {
      const error = e as BusinessError;
      const message = `${error.code ?? ''} ${error.message ?? String(e)}`;
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
          lastText: this.lastText
        };
        return JSON.stringify(state);
      }
      case 'insert':
        return String(await this.requireClient().insertText(c.text ?? ''));
      case 'backspace':
        // IMF's "forward" is towards the start of the text: deleteForward is Backspace.
        return String(await this.requireClient().deleteForward(c.n ?? 1));
      case 'delete':
        return String(await this.requireClient().deleteBackward(c.n ?? 1));
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
        return String(await this.requireClient().sendKeyFunction(c.action ?? 0));
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
