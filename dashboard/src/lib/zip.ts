// A ZIP archive written without compression. Exports are text the reader will
// unpack once, so storing the files as they are keeps this to the container
// format alone and avoids a compression dependency.

const table = (() => {
  const values = new Uint32Array(256);
  for (let index = 0; index < 256; index += 1) {
    let value = index;
    for (let bit = 0; bit < 8; bit += 1)
      value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
    values[index] = value >>> 0;
  }
  return values;
})();

export function crc32(bytes: Uint8Array): number {
  let value = 0xffffffff;
  for (const byte of bytes) value = (table[(value ^ byte) & 0xff] ?? 0) ^ (value >>> 8);
  return (value ^ 0xffffffff) >>> 0;
}

/** The classic format's limits; larger exports are refused, not corrupted. */
const MAX_ENTRIES = 0xffff;
const MAX_BYTES = 0xffffffff;
const UTF8_NAMES = 0x0800;

interface Entry {
  readonly name: Uint8Array;
  readonly crc: number;
  readonly size: number;
  readonly offset: number;
}

export class ZipWriter {
  readonly #parts: Uint8Array<ArrayBuffer>[] = [];
  readonly #entries: Entry[] = [];
  readonly #time: number;
  readonly #date: number;
  #offset = 0;

  constructor(stamp: Date) {
    this.#time = (stamp.getHours() << 11) | (stamp.getMinutes() << 5) | (stamp.getSeconds() >> 1);
    this.#date =
      (Math.max(0, stamp.getFullYear() - 1980) << 9) |
      ((stamp.getMonth() + 1) << 5) |
      stamp.getDate();
  }

  /** Adds one file. Names use forward slashes for folders. */
  add(path: string, content: Uint8Array<ArrayBuffer>): void {
    const name = new TextEncoder().encode(path);
    if (this.#entries.length >= MAX_ENTRIES)
      throw new Error("This export has more files than one ZIP archive can hold.");
    if (name.length > 0xffff) throw new Error("A file name in this export is too long.");
    const entry = { name, crc: crc32(content), size: content.length, offset: this.#offset };
    const header = new DataView(new ArrayBuffer(30));
    header.setUint32(0, 0x04034b50, true);
    this.#describe(header, 4, entry);
    this.#push(new Uint8Array(header.buffer), name, content);
    this.#entries.push(entry);
  }

  finish(): Blob {
    const start = this.#offset;
    for (const entry of this.#entries) {
      const header = new DataView(new ArrayBuffer(46));
      header.setUint32(0, 0x02014b50, true);
      header.setUint16(4, 20, true);
      this.#describe(header, 6, entry);
      header.setUint32(42, entry.offset, true);
      this.#push(new Uint8Array(header.buffer), entry.name);
    }
    const end = new DataView(new ArrayBuffer(22));
    end.setUint32(0, 0x06054b50, true);
    end.setUint16(8, this.#entries.length, true);
    end.setUint16(10, this.#entries.length, true);
    end.setUint32(12, this.#offset - start, true);
    end.setUint32(16, start, true);
    this.#push(new Uint8Array(end.buffer));
    return new Blob(this.#parts, { type: "application/zip" });
  }

  // The fields a local and a central header share, from "version needed" on.
  #describe(header: DataView, at: number, entry: Entry): void {
    header.setUint16(at, 20, true);
    header.setUint16(at + 2, UTF8_NAMES, true);
    header.setUint16(at + 6, this.#time, true);
    header.setUint16(at + 8, this.#date, true);
    header.setUint32(at + 10, entry.crc, true);
    header.setUint32(at + 14, entry.size, true);
    header.setUint32(at + 18, entry.size, true);
    header.setUint16(at + 22, entry.name.length, true);
  }

  #push(...parts: Uint8Array[]): void {
    for (const part of parts) {
      this.#offset += part.length;
      if (this.#offset > MAX_BYTES)
        throw new Error(
          "This export is larger than 4 GB, the most one ZIP archive can hold. Export fewer chats at a time.",
        );
      this.#parts.push(part as Uint8Array<ArrayBuffer>);
    }
  }
}
