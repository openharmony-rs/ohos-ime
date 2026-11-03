import { Command, testIme } from '../ime/TestIme';

@Entry
@Component
struct KeyboardPanel {
  @StorageLink('testImeStatus') status: string = 'idle';

  private send(command: Command): void {
    testIme.run(command);
  }

  build() {
    Column({ space: 8 }) {
      Text('Test IME').fontSize(18).fontWeight(FontWeight.Bold).id('testime_title')
      Text(this.status).fontSize(14).id('testime_status')
      Row({ space: 8 }) {
        Button('a').id('testime_key_a').onClick(() => this.send({ op: 'insert', text: 'a' }))
        Button('⌫').id('testime_key_backspace').onClick(() => this.send({ op: 'backspace', n: 1 }))
        Button('⏎').id('testime_key_enter').onClick(() => this.send({ op: 'key', action: 0 }))
        Button('hide').id('testime_hide').onClick(() => this.send({ op: 'hide' }))
      }
    }
    .width('100%')
    .height('100%')
    .padding(12)
    .backgroundColor('#dde3ea')
  }
}
