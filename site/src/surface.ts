/**
 * The landing hero's background: a dotted surface seen in perspective, rippling
 * slowly, with the supported agents' marks standing on it. It is drawn in the
 * canvas's own text colour, so it follows the site's theme.
 */

/** One agent's mark: a single path in a 24 by 24 box. */
export interface Logo {
  readonly name: string;
  readonly d: string;
  readonly rule: CanvasFillRule;
}

interface Mark {
  readonly path: Path2D;
  readonly rule: CanvasFillRule;
  /** Where it stands: a fraction of the half-width at its depth. */
  readonly u: number;
  readonly z: number;
  x: number;
}

/** A ring that spreads from a point and dies away. */
interface Drop {
  readonly x: number;
  readonly z: number;
  readonly born: number;
  readonly amp: number;
}

const Z_NEAR = 1;
const Z_FAR = 11;
/** Starlight stacks the hero below this width; the surface then sits under the copy. */
const NARROW = 800;
/** Surface seconds per real second: a slow swell and unhurried rings. */
const SPEED = 0.55;
/** Where the marks stand, clear of the copy on the hero's far side, by `u` and depth. */
const SPOTS: readonly (readonly [number, number])[] = [
  [0.62, 2.3],
  [0.9, 3.0],
  [0.45, 3.9],
  [0.76, 4.6],
  [0.94, 5.6],
  [0.58, 6.6],
];

const smooth = (from: number, to: number, value: number): number => {
  const t = Math.min(1, Math.max(0, (value - from) / (to - from)));
  return t * t * (3 - 2 * t);
};
const fog = (f: number): number => 0.05 + 0.8 * (1 - f) ** 1.6;

function isLogo(value: unknown): value is Logo {
  if (typeof value !== "object" || value === null) return false;
  const { name, d, rule } = value as Record<string, unknown>;
  return (
    typeof name === "string" && typeof d === "string" && (rule === "evenodd" || rule === "nonzero")
  );
}

function parseLogos(text: string | undefined): Logo[] {
  try {
    const parsed: unknown = JSON.parse(text ?? "[]");
    return Array.isArray(parsed) ? parsed.filter(isLogo) : [];
  } catch {
    return [];
  }
}

/** Draws the surface on `canvas` until it leaves the page. Its parent is the hero it fills. */
export function mountSurface(canvas: HTMLCanvasElement): void {
  const hero = canvas.parentElement;
  const context = canvas.getContext("2d");
  if (!hero || !context || canvas.dataset.mounted) return;
  const ctx: CanvasRenderingContext2D = context;
  canvas.dataset.mounted = "true";

  const marks: Mark[] = parseLogos(canvas.dataset.logos)
    .flatMap((logo, index) => {
      const spot = SPOTS[index];
      if (!spot) return [];
      return [{ path: new Path2D(logo.d), rule: logo.rule, u: spot[0], z: spot[1], x: 0 }];
    })
    .sort((a, b) => b.z - a.z);
  let drops: Drop[] = [
    { x: -1.6, z: 3.2, born: 12.3, amp: 0.06 },
    { x: 2.6, z: 5.2, born: 11.0, amp: 0.06 },
  ];
  // The pointer is one broad, soft swell. It trails the cursor and rises and
  // settles slowly, and is smooth at its centre, so nothing on the surface jumps.
  const pointer = { x: 0, z: 4, toX: 0, toZ: 4, gain: 0, want: 0 };

  let width = 0;
  let height = 0;
  let horizon = 0;
  let focal = 1;
  let narrow = false;
  let fadeFrom = 0;
  let fadeTo = 1;
  let ink = "#f0ece4";
  let clock = 14;
  let last = 0;
  let nextDrop = 15.5;
  let visible = true;
  let looping = false;
  let seed = 7;
  const random = (): number => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed / 4294967296;
  };

  // A slow fold, two steady sources that interfere, small rings around each
  // mark and the pointer, and the occasional drop.
  function lift(x: number, z: number): number {
    const t = clock;
    let y =
      0.22 * Math.sin(x * 0.42 + t * 0.11) * Math.cos(z * 0.36 - t * 0.08) +
      0.1 * Math.sin((x + z) * 0.9 - t * 0.23);
    const first = Math.hypot(x + 2.2, z - 4.5);
    const second = Math.hypot(x - 3.1, z - 6.5);
    y +=
      (0.06 * Math.sin(first * 3.2 - t * 1.3)) / (1 + first * 0.35) +
      (0.05 * Math.sin(second * 2.6 - t * 1.1)) / (1 + second * 0.3);
    for (const mark of marks) {
      const r = Math.hypot(x - mark.x, z - mark.z);
      if (r < 2.4) y += 0.02 * Math.sin(r * 9 - t * 1.6) * Math.exp(-r * 1.6);
    }
    if (pointer.gain > 0.01) {
      const r = Math.hypot(x - pointer.x, z - pointer.z);
      if (r < 4) y += 0.05 * pointer.gain * Math.cos(r * 2.4 - t * 1.2) * Math.exp(-r * r * 0.55);
    }
    for (const drop of drops) {
      const age = t - drop.born;
      const offset = Math.hypot(x - drop.x, z - drop.z) - age * 1.1;
      const q = offset / 0.55;
      if (q > 3 || q < -3) continue;
      // Fade in over the first moments so a new ring never pops.
      const swell = Math.min(1, age / 0.8) * Math.exp(-age / 4);
      y += drop.amp * swell * Math.cos(offset * 5) * Math.exp(-q * q);
    }
    return y;
  }

  /** Dims the surface behind the copy and fades it out towards the hero's lower edge. */
  const veil = (sx: number, sy: number): number =>
    (narrow ? 1 : 0.38 + 0.62 * smooth(fadeFrom, fadeTo, sx)) *
    (1 - smooth(height * 0.8, height, sy));

  function drawMark(mark: Mark): void {
    const reach = 0.12;
    const slope = (lift(mark.x + reach, mark.z) - lift(mark.x - reach, mark.z)) / (2 * reach);
    const sx = width / 2 + (mark.x / mark.z) * focal;
    const sy = horizon + ((1 - lift(mark.x, mark.z)) / mark.z) * focal;
    const size = Math.min(72, (0.25 * focal) / mark.z);
    const depth = Math.log(mark.z / Z_NEAR) / Math.log(Z_FAR / Z_NEAR);
    const alpha = Math.min(1, 0.4 + 1.3 * fog(depth)) * veil(sx, sy);
    ctx.globalAlpha = alpha * 0.14;
    ctx.beginPath();
    ctx.ellipse(sx, sy, size * 0.5, size * 0.11, 0, 0, Math.PI * 2);
    ctx.fill();
    // The mark stands on the surface and leans a little with its slope.
    ctx.save();
    ctx.translate(sx, sy);
    ctx.rotate(-Math.atan(slope) * 0.4);
    ctx.scale(size / 24, size / 24);
    ctx.translate(-12, -26);
    ctx.globalAlpha = alpha;
    ctx.fill(mark.path, mark.rule);
    ctx.restore();
  }

  function draw(): void {
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = ink;
    const columns = Math.round(Math.min(150, Math.max(70, width / 9)));
    const rows = 58;
    for (let j = rows - 1; j >= 0; j--) {
      const f = j / (rows - 1);
      const z = Z_NEAR * (Z_FAR / Z_NEAR) ** f;
      const half = (width / 2 / focal) * z * 1.04;
      const size = 1 + (1 - f) * 1.8;
      const base = fog(f);
      let beforeX = 0;
      let beforeY = 0;
      let x0 = 0;
      let y0 = 0;
      for (let i = 0; i < columns; i++) {
        const x = ((i / (columns - 1)) * 2 - 1) * half;
        const sx = width / 2 + (x / z) * focal;
        const sy = horizon + ((1 - lift(x, z)) / z) * focal;
        if (i >= 2) {
          // Brighter where the surface tilts towards the light on the left.
          const tilt = (sy - beforeY) / (sx - beforeX);
          const alpha = base * (0.55 + tilt * 2.2) * veil(x0, y0);
          if (alpha > 0.02) {
            ctx.globalAlpha = Math.min(1, alpha);
            ctx.fillRect(x0 - size / 2, y0 - size / 2, size, size);
          }
        }
        beforeX = x0;
        beforeY = y0;
        x0 = sx;
        y0 = sy;
      }
    }
    for (const mark of marks) drawMark(mark);
    ctx.globalAlpha = 1;
  }

  function resize(): void {
    const box = canvas.parentElement?.getBoundingClientRect();
    if (!box) return;
    const ratio = Math.min(2, devicePixelRatio || 1);
    width = Math.max(1, document.documentElement.clientWidth);
    height = Math.max(1, box.height);
    // The canvas spans the window, not just the hero's column.
    canvas.style.left = `${-box.left}px`;
    canvas.style.width = `${width}px`;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    narrow = width < NARROW;
    horizon = narrow ? height - Math.min(270, height * 0.42) : height * 0.16;
    focal = (height - horizon) * 1.45;
    fadeFrom = box.left + box.width * 0.5;
    fadeTo = box.left + box.width * 0.8;
    for (const mark of marks) {
      const u = narrow ? (mark.u - 0.68) * 2.6 : mark.u;
      mark.x = ((u * (width / 2)) / focal) * mark.z;
    }
    draw();
  }

  function frame(now: number): void {
    if (!canvas.isConnected) {
      unmount();
      return;
    }
    if (!visible) {
      looping = false;
      return;
    }
    const elapsed = Math.min(0.05, (now - last) / 1000);
    last = now;
    clock += elapsed * SPEED;
    const follow = 1 - Math.exp(-elapsed * 0.9);
    pointer.x += (pointer.toX - pointer.x) * follow;
    pointer.z += (pointer.toZ - pointer.z) * follow;
    pointer.gain += (pointer.want - pointer.gain) * (1 - Math.exp(-elapsed * 0.6));
    if (clock > nextDrop) {
      const drop = {
        x: (random() * 2 - 1) * 5,
        z: 1.8 + random() * 5,
        born: clock,
        amp: 0.04 + random() * 0.02,
      };
      drops = [...drops.filter((d) => clock - d.born < 12).slice(-7), drop];
      nextDrop = clock + 2.5 + random() * 2.5;
    }
    draw();
    requestAnimationFrame(frame);
  }
  function start(): void {
    if (looping) return;
    looping = true;
    last = performance.now();
    requestAnimationFrame(frame);
  }

  const readInk = (): void => {
    ink = getComputedStyle(canvas).color;
    draw();
  };
  const life = new AbortController();
  const sized = new ResizeObserver(resize);
  const themed = new MutationObserver(readInk);
  const seen = new IntersectionObserver((entries) => {
    visible = entries.some((entry) => entry.isIntersecting);
    if (visible) start();
  });
  function unmount(): void {
    life.abort();
    sized.disconnect();
    themed.disconnect();
    seen.disconnect();
  }

  addEventListener(
    "pointermove",
    (event) => {
      const box = canvas.getBoundingClientRect();
      const y = event.clientY - box.top;
      if (y < horizon + 14 || y > height) {
        pointer.want = 0;
        return;
      }
      const z = Math.min(7, focal / (y - horizon));
      pointer.toX = ((event.clientX - box.left - width / 2) * z) / focal;
      pointer.toZ = z;
      pointer.want = 1;
    },
    { signal: life.signal, passive: true },
  );
  addEventListener("resize", resize, { signal: life.signal });
  document.addEventListener("astro:before-swap", unmount, { signal: life.signal });
  themed.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  sized.observe(hero);
  seen.observe(hero);
  ink = getComputedStyle(canvas).color;
  resize();
  start();
}
