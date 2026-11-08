/** Reading positions belong to this browser session, never the archived history. */
interface Position {
  readonly id: string;
  readonly node: string;
  readonly message: string;
  readonly shared: boolean;
}
class Reading {
  positions = $state<readonly Position[]>([]);
  remember(position: Position): void {
    this.positions = [
      position,
      ...this.positions.filter(
        (item) =>
          item.id !== position.id || item.node !== position.node || item.shared !== position.shared,
      ),
    ].slice(0, 50);
  }
}
export const reading = new Reading();
