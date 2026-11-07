// The recycling plant's outside (docs/outside.md): an industrial sorting shed in recycling green, with a
// bay door, a smokestack and a yard of bins, bales and a conveyor. It grows up today's placedPlant in
// world.js: with nothing chosen it comes out close to that, a green steel shed with a roll-up bay door,
// high strip windows and a brick stack.
//
// This module imports from world.js, which imports it back, so nothing here calls an imported function
// while the module loads. The tables below hold literals and functions, and they run later.

import { GLASS_BOT, GLASS_TOP, P, box, dark, ell, fp, fp3, flatRoof, grad, line, lite, mixHex, rgba, rr, seeded, shadow, win } from './world.js';
import { kit } from './outside-decor.js';
import { bodyOf, colorOf, hexOf, isOn, numberOf, styleOf } from './outside-spec.js';

const TAU = 6.2832;
const STOREY = 13; // the first storey's height in units; each one above it is UPPER
const UPPER = 9;
const PLANT_WALL = '#5e8a5a';
const CREAM = '#f4f1e8';
const STEEL = '#b9c0c4';
const PLATFORM = '#b8b4a8';
const ROOFTOP = ['solar_panels', 'hopper', 'conveyor', 'water_tank', 'vents'];

export const PLANT_DEFAULTS = { plant_cladding: 'corrugated', plant_windows: 'clerestory', plant_roof: 'flat', plant_bay: 'roller', plant_stack: 'brick', plant_emblem: 'arrows' };
export const PLANT_ROOFTOP_DEFAULT = [];

// --- Cladding: each paints a wall's face into its plane, x along it and y down (top to bottom) -----

const CLADDING = {
  // Corrugated steel sheets, the ribs running up the wall, with the light off each crest.
  corrugated(g, len, top, bot, c) {
    const h = bot - top;
    g.fillStyle = c;
    g.fillRect(0, top, len, h);
    for (let x = 0; x < len; x += 2.2) {
      const gr = g.createLinearGradient(x, 0, x + 2.2, 0);
      gr.addColorStop(0, dark(c, 0.22));
      gr.addColorStop(0.5, lite(c, 0.2));
      gr.addColorStop(1, dark(c, 0.3));
      g.fillStyle = gr;
      g.fillRect(x, top, Math.min(2.2, len - x), h);
    }
    g.fillStyle = grad(g, 0, top, 0, bot, 'rgba(255,250,230,.16)', 'rgba(0,0,0,0)', 'rgba(20,10,30,.2)');
    g.fillRect(0, top, len, h);
    rr(g, 'rgba(20,30,20,.3)', 0, top, len, 0.8);
  },

  // Industrial brick in a running bond, with a little tone change from brick to brick.
  brick(g, len, top, bot, c, rand) {
    const bh = 2.5;
    const bw = 5.4;
    g.fillStyle = mixHex(c, '#d6cdb8', 0.55);
    g.fillRect(0, top, len, bot - top);
    let row = 0;
    for (let y = bot - bh; y + bh > top; y -= bh, row++) {
      for (let x = -(row % 2) * bw / 2; x < len; x += bw) {
        g.fillStyle = mixHex(c, rand() < 0.5 ? dark(c, 0.3) : lite(c, 0.22), rand() * 0.6);
        g.fillRect(x + 0.3, y + 0.3, bw - 0.6, bh - 0.6);
        rr(g, 'rgba(255,240,220,.12)', x + 0.3, y + 0.3, bw - 0.6, 0.45);
      }
    }
    g.fillStyle = grad(g, 0, top, 0, bot, 'rgba(255,240,210,.14)', 'rgba(0,0,0,0)', 'rgba(20,10,30,.16)');
    g.fillRect(0, top, len, bot - top);
  },

  // Precast concrete panels: each panel a shade off its neighbours, with pebbles in the mix and fine joints.
  concrete(g, len, top, bot, c, rand) {
    const pw = 8;
    const ph = 6;
    g.fillStyle = c;
    g.fillRect(0, top, len, bot - top);
    for (let y = bot; y > top; y -= ph) {
      for (let x = 0; x < len; x += pw) {
        const x1 = Math.min(len, x + pw);
        const y0 = Math.max(top, y - ph);
        g.fillStyle = rand() < 0.5 ? lite(c, 0.04 + rand() * 0.08) : dark(c, 0.03 + rand() * 0.06);
        g.fillRect(x, y0, x1 - x, y - y0);
        g.strokeStyle = 'rgba(25,28,30,.38)';
        g.lineWidth = 0.5;
        g.beginPath();
        g.rect(x, y0, x1 - x, y - y0);
        g.stroke();
        for (let i = 0; i < 9; i++) {
          rr(g, rand() < 0.5 ? 'rgba(255,255,250,.3)' : 'rgba(30,30,30,.25)', x + rand() * (x1 - x - 0.8), y0 + rand() * (y - y0 - 0.8), 0.7, 0.6);
        }
      }
    }
  },

  // Timber boards, each one a little different, with grain and a dark joint along the bottom of each.
  timber(g, len, top, bot, c, rand) {
    const bh = 2.6;
    for (let y = bot - bh; y + bh > top; y -= bh) {
      const y0 = Math.max(top, y);
      g.fillStyle = mixHex(c, dark(c, 0.3), 0.15 + rand() * 0.25);
      g.fillRect(0, y0, len, y + bh - y0);
      for (let k = 0; k < 3; k++) {
        const gy = y + 0.6 + rand() * (bh - 1.2);
        const gx = rand() * len * 0.4;
        line(g, 'rgba(255,240,210,.14)', 0.4, gx, gy, gx + len * (0.3 + rand() * 0.3), gy + 0.2);
      }
      rr(g, 'rgba(20,20,20,.5)', 0, y, len, 0.45);
      for (let x = rand() * 9; x < len; x += 9) rr(g, 'rgba(20,20,20,.4)', x, y, 0.45, bh);
    }
  },

  // Reclaimed panels in a patchwork of muted colours, each with a bevel, a dark edge and two bolts.
  recycled(g, len, top, bot, c, rand) {
    const cols = [mixHex(c, '#a88f6a', 0.5), mixHex(c, '#6e8c9a', 0.5), mixHex(c, '#b06c5a', 0.45), mixHex(c, '#c9bd8c', 0.5), c, mixHex(c, '#4b5a66', 0.4)];
    let y = bot;
    while (y > top) {
      const rh = Math.min(y - top, 3 + rand() * 2.2);
      let x = -rand() * 3;
      while (x < len) {
        const w = 3 + rand() * 3.5;
        const col = cols[Math.floor(rand() * cols.length)];
        const x0 = Math.max(0, x);
        const x1 = Math.min(len, x + w);
        if (x1 > x0) {
          g.fillStyle = grad(g, 0, y - rh, 0, y, lite(col, 0.1), col, dark(col, 0.12));
          g.fillRect(x0, y - rh, x1 - x0, rh);
          rr(g, 'rgba(255,255,240,.18)', x0, y - rh, x1 - x0, 0.5);
          g.strokeStyle = 'rgba(20,20,26,.5)';
          g.lineWidth = 0.45;
          g.beginPath();
          g.rect(x, y - rh, w, rh);
          g.stroke();
          ell(g, 'rgba(30,30,30,.35)', x + 0.8, y - rh + 0.8, 0.3, 0.3);
          ell(g, 'rgba(30,30,30,.35)', x + w - 0.8, y - 0.8, 0.3, 0.3);
        }
        x += w;
      }
      y -= rh;
    }
  },
};

// --- Windows: a row across a face, `number` per wall or as many as fit when `number` is null ----------

const WINDOWS = {
  // A high strip of small windows near the top of each storey, as the plant has always had.
  clerestory: { w: 3.2, h: 2.6, gap: 2.3, lit: 65, at: (z0, hs) => z0 + hs - 4.2 },
  // Square windows at about mid-storey.
  square: { w: 3.6, h: 4.6, gap: 3.2, lit: 60, at: (z0, hs) => z0 + (hs - 4.6) / 2 + 0.6 },
  // Louvred vents: slatted, and dark at night.
  louvres: { w: 4.4, h: 5.2, gap: 3, lit: 0, at: (z0, hs) => z0 + (hs - 5.2) / 2 },
};

// --- Bays: the big door on the door face, with its height and the top of its housing -----------------

const BAYS = {
  // One roll-up door with slats, as the plant has always had.
  roller: {
    top: 8.8,
    extra: 0,
    draw(S, d, col) {
      if (S.day) rollerDoor(S.g, d.cx - d.bw / 2, d.bw, 0, 7, col, true);
      else lampOver(S, d.cx, 8.8);
    },
  },
  // Two roll-up doors side by side.
  twin: {
    top: 8.8,
    extra: 0,
    draw(S, d, col) {
      if (!S.day) return lampOver(S, d.cx, 8.8);
      const w = (d.bw - 0.6) / 2;
      rollerDoor(S.g, d.cx - d.bw / 2, w, 0, 7, col, false);
      rollerDoor(S.g, d.cx + 0.3, w, 0, 7, col, false);
    },
  },
  // A raised loading dock: a platform out from the wall, with bumpers and a shorter door above it.
  dock: {
    top: 10.2,
    extra: 4,
    draw(S, d, col) {
      if (!S.day) return lampOver(S, d.cx, 10.2);
      const g = S.g;
      rollerDoor(g, d.cx - d.bw / 2, d.bw, 3.2, 5.2, col, false);
      for (const x of [d.cx - d.bw / 2 - 0.9, d.cx + d.bw / 2]) rr(g, '#262626', x, -4.4, 0.9, 1.2);
    },
  },
  // A glazed sorting-hall front, with a conveyor inside that you can see through the glass.
  glass: {
    top: 8.9,
    extra: 0,
    draw(S, d, col) {
      glassFront(S, d.cx - d.bw / 2, d.bw, 8, col);
    },
  },
};

// --- Emblems: a picture panel on the wall above the bay. No words. -----------------------------------

const EMBLEMS = {
  none() {},
  // The three chasing recycling arrows, each an arc with a head.
  arrows(g, x, y, r) {
    for (let k = 0; k < 3; k++) {
      const a0 = k * TAU / 3 - 1.2;
      const a1 = a0 + 1.7;
      g.strokeStyle = CREAM;
      g.lineWidth = r * 0.3;
      g.beginPath();
      g.arc(x, y, r, a0, a1);
      g.stroke();
      const hx = x + Math.cos(a1) * r;
      const hy = y + Math.sin(a1) * r;
      const tx = -Math.sin(a1);
      const ty = Math.cos(a1);
      const nx = Math.cos(a1);
      const ny = Math.sin(a1);
      g.fillStyle = CREAM;
      g.beginPath();
      g.moveTo(hx + tx * r * 0.6, hy + ty * r * 0.6);
      g.lineTo(hx + nx * r * 0.42, hy + ny * r * 0.42);
      g.lineTo(hx - nx * r * 0.42, hy - ny * r * 0.42);
      g.closePath();
      g.fill();
    }
  },
  // A leaf with its vein.
  leaf(g, x, y, r) {
    g.save();
    g.translate(x, y);
    g.rotate(-0.6);
    g.fillStyle = CREAM;
    g.beginPath();
    g.ellipse(0, 0, r * 1.1, r * 0.55, 0, 0, TAU);
    g.fill();
    line(g, 'rgba(46,125,50,.8)', 0.3, -r, 0, r, 0);
    g.restore();
  },
  // A bottle, its neck and its cap.
  bottle(g, x, y, r) {
    g.fillStyle = CREAM;
    g.fillRect(x - r * 0.5, y - r * 0.2, r, r * 1.2);
    g.fillRect(x - r * 0.2, y - r * 0.95, r * 0.4, r * 0.8);
    g.fillRect(x - r * 0.27, y - r * 1.1, r * 0.54, r * 0.2);
  },
  // A cog, with eight teeth and a hole.
  gear(g, x, y, r, col) {
    g.fillStyle = CREAM;
    for (let k = 0; k < 8; k++) {
      g.save();
      g.translate(x, y);
      g.rotate((k * TAU) / 8);
      g.fillRect(-r * 0.22, -r * 1.05, r * 0.44, r * 0.45);
      g.restore();
    }
    g.beginPath();
    g.arc(x, y, r * 0.8, 0, TAU);
    g.fill();
    g.fillStyle = col;
    g.beginPath();
    g.arc(x, y, r * 0.33, 0, TAU);
    g.fill();
  },
  // A globe with meridians, the equator, and two landmasses.
  globe(g, x, y, r, col) {
    g.fillStyle = CREAM;
    g.beginPath();
    g.arc(x, y, r, 0, TAU);
    g.fill();
    g.strokeStyle = col;
    g.lineWidth = r * 0.15;
    g.beginPath();
    g.ellipse(x, y, r * 0.5, r, 0, 0, TAU);
    g.stroke();
    g.beginPath();
    g.moveTo(x - r, y);
    g.lineTo(x + r, y);
    g.stroke();
    g.fillStyle = col;
    g.beginPath();
    g.ellipse(x - r * 0.35, y - r * 0.3, r * 0.3, r * 0.2, 0.3, 0, TAU);
    g.fill();
    g.beginPath();
    g.ellipse(x + r * 0.3, y + r * 0.35, r * 0.25, r * 0.15, -0.2, 0, TAU);
    g.fill();
  },
};

// --- Roofs: each returns how a roof rises (zAt) and the deck its items stand on -----------------------

const ROOFS = {
  // A flat roof with a parapet and a tarred floor, its parapet in the roof colour.
  flat(S, R, H, col) {
    flatRoof(S, R, H, { k: 'steel', c: col }, { coping: lite(col, 0.08) });
    const z = H + 2.5;
    return { zAt: () => z, deck: z };
  },

  // A planted roof: the same parapet, and a bed of grass with tufts and a few flowers.
  green(S, R, H, col) {
    const g = S.g;
    const [u0, v0, u1, v1] = R;
    flatRoof(S, R, H, { k: 'steel', c: col }, { coping: col });
    const z = H + 2.5;
    fp3(g, mixHex(col, '#2f4a2c', 0.4), [u0 + 1, v0 + 1, z], [u1 - 1, v0 + 1, z], [u1 - 1, v1 - 1, z], [u0 + 1, v1 - 1, z]);
    const rand = seeded(Math.round(u0 * 31 + v0 * 17) + 3);
    for (let i = 0; i < 90; i++) {
      const [x, y] = P(u0 + 1.5 + rand() * (u1 - u0 - 3), v0 + 1.5 + rand() * (v1 - v0 - 3), z);
      ell(g, lite(col, rand() * 0.2 - 0.05), x, y - 0.5, 1.6, 1.0);
      line(g, dark(col, 0.3), 0.3, x, y, x - 0.6, y - 1.3);
      if (rand() < 0.2) ell(g, '#f0e070', x + 0.8, y - 1.0, 0.35, 0.35);
    }
    return { zAt: () => z, deck: z };
  },

  // A factory sawtooth roof: glazed slopes facing the viewer, with a tooth's upright side hidden behind each.
  sawtooth(S, R, H, col) {
    const g = S.g;
    const [u0, v0, u1, v1] = R;
    const rise = 3.6;
    fp3(g, '#4a5061', [u0, v0, H], [u1, v0, H], [u1, v1, H], [u0, v1, H]);
    const n = Math.max(2, Math.round((u1 - u0) / 4.6));
    const p = (u1 - u0) / n;
    const glass = mixHex('#7fb0c8', col, 0.2);
    for (let k = 0; k < n; k++) {
      const ua = u0 + k * p;
      const ub = ua + p;
      fp3(g, glass, [ua, v0, H + rise], [ua, v1, H + rise], [ub, v1, H], [ub, v0, H]);
      for (let j = 1; j < 4; j++) {
        const v = v0 + ((v1 - v0) * j) / 4;
        line(g, 'rgba(30,45,50,.45)', 0.4, ...P(ua, v, H + rise), ...P(ub, v, H));
      }
      line(g, dark(col, 0.2), 0.8, ...P(ua, v0, H + rise), ...P(ua, v1, H + rise));
      fp3(g, dark(col, 0.1), [ua, v1, H], [ua, v1, H + rise], [ub, v1, H]);
    }
    return { zAt: () => H, deck: H };
  },

  // A barrel roof: a curved roof running along the building, with ribs and a shaded end.
  barrel(S, R, H, col) {
    const g = S.g;
    const [u0, v0, u1, v1] = R;
    const a = (v1 - v0) / 2;
    const vc = (v0 + v1) / 2;
    const rise = 5.5;
    const N = 10;
    const prof = (t) => [vc - a * Math.cos(Math.PI * t), H + rise * Math.sin(Math.PI * t)];
    for (let i = 0; i < N; i++) {
      const t0 = i / N;
      const t1 = (i + 1) / N;
      const tm = (t0 + t1) / 2;
      const [va, za] = prof(t0);
      const [vb, zb] = prof(t1);
      // The outward normal in the section: which way the strip faces.
      const nv = -rise * Math.PI * Math.cos(Math.PI * tm);
      const nz = a * Math.PI * Math.sin(Math.PI * tm);
      const k = (nz * 0.3 - nv * 0.2) / Math.hypot(nv, nz);
      const shade = k >= 0 ? lite(col, k * 0.3) : dark(col, -k * 0.35);
      fp3(g, shade, [u0, va, za], [u1, va, za], [u1, vb, zb], [u0, vb, zb]);
      line(g, 'rgba(20,30,20,.2)', 0.4, ...P(u0, va, za), ...P(u1, va, za));
    }
    const cap = [];
    for (let i = 0; i <= N; i++) {
      const [v, z] = prof(i / N);
      cap.push([u1, v, z]);
    }
    fp3(g, dark(col, 0.25), ...cap);
    line(g, lite(col, 0.35), 1, ...P(u0, vc, H + rise), ...P(u1, vc, H + rise));
    return {
      zAt: (u, v) => H + rise * Math.sin(Math.PI * Math.min(1, Math.max(0, (v - v0) / (v1 - v0)))),
      deck: H,
    };
  },
};

// --- Rooftop kit: each returns entries { d, run } that are painted back to front ----------------------

function cylinder(g, u, v, r, z0, z1, col) {
  const [x, y0] = P(u, v, z0);
  const [, y1] = P(u, v, z1);
  const rx = r * 1.414;
  const ry = r * 0.707;
  g.beginPath();
  g.moveTo(x + rx, y1);
  g.lineTo(x + rx, y0);
  g.ellipse(x, y0, rx, ry, 0, 0, Math.PI);
  g.lineTo(x - rx, y1);
  g.closePath();
  g.fillStyle = grad(g, x - rx, 0, x + rx, 0, lite(col, 0.32), col, dark(col, 0.34));
  g.fill();
  for (let z = z0 + 1.2; z < z1 - 0.6; z += 1.4) {
    const [, yz] = P(u, v, z);
    g.strokeStyle = rgba(dark(col, 0.3), 0.45);
    g.lineWidth = 0.35;
    g.beginPath();
    g.ellipse(x, yz, rx, ry, 0, 0, Math.PI);
    g.stroke();
  }
  ell(g, lite(col, 0.22), x, y1, rx, ry);
}

const ROOFTOP_ITEMS = {
  // Panels on a tilt over the back left of the roof, raised at the back and facing the yard.
  solar_panels({ S, R, zAt, s }) {
    const [u0, v0, u1, v1] = R;
    const L = u1 - u0;
    const D = v1 - v0;
    const ul = u0 + 1.5 * s;
    const ur = u0 + L * 0.36;
    const vl = v0 + 1.5 * s;
    const vh = v0 + D * 0.4;
    const z = zAt((ul + ur) / 2, (vl + vh) / 2);
    const n = Math.max(2, Math.round((ur - ul) / 2.8));
    return [{
      d: (ul + ur) / 2 + (vl + vh) / 2,
      run() {
        const g = S.g;
        for (let i = 0; i < n; i++) {
          const a0 = ul + ((ur - ul) * i) / n + 0.2;
          const a1 = ul + ((ur - ul) * (i + 1)) / n - 0.2;
          const zb0 = z + 1.8 * s;
          const zb1 = z + 0.5 * s;
          line(g, '#8b939a', 0.5, ...P(a0, vl, z), ...P(a0, vl, zb0));
          fp3(g, '#2b4d72', [a0, vl, zb0], [a1, vl, zb0], [a1, vh, zb1], [a0, vh, zb1]);
          for (let j = 1; j < 3; j++) {
            const t = j / 3;
            line(g, 'rgba(170,195,220,.45)', 0.35, ...P(a0, vl + (vh - vl) * t, zb0 + (zb1 - zb0) * t), ...P(a1, vl + (vh - vl) * t, zb0 + (zb1 - zb0) * t));
          }
          const am = (a0 + a1) / 2;
          line(g, 'rgba(170,195,220,.4)', 0.35, ...P(am, vl, zb0), ...P(am, vh, zb1));
          fp3(g, 'rgba(255,255,255,.12)', [a0, vl, zb0], [am, vl, zb0], [am, vh, zb1], [a0, vh, zb1]);
          line(g, 'rgba(210,225,235,.7)', 0.5, ...P(a0, vl, zb0), ...P(a1, vl, zb0));
        }
      },
    }];
  },

  // A sorting hopper: a funnel, wide at the mouth on top and narrowing to a spout on the roof.
  hopper({ S, R, zAt, s, accent }) {
    const [u0, v0, u1, v1] = R;
    const cu = u0 + (u1 - u0) * 0.52;
    const cv = v0 + (v1 - v0) * 0.6;
    const z = zAt(cu, cv);
    const r0 = 0.8 * s;
    const r1 = 2.4 * s;
    const zt = z + 5 * s;
    return [{
      d: cu + cv,
      run() {
        const g = S.g;
        const body = mixHex(accent, '#c9d0d4', 0.45);
        fp3(g, body, [cu - r0, cv + r0, z], [cu + r0, cv + r0, z], [cu + r1, cv + r1, zt], [cu - r1, cv + r1, zt]);
        fp3(g, dark(body, 0.28), [cu + r0, cv - r0, z], [cu + r0, cv + r0, z], [cu + r1, cv + r1, zt], [cu + r1, cv - r1, zt]);
        fp3(g, lite(body, 0.2), [cu - r1, cv - r1, zt], [cu + r1, cv - r1, zt], [cu + r1, cv + r1, zt], [cu - r1, cv + r1, zt]);
        fp3(g, '#20282a', [cu - r1 * 0.7, cv - r1 * 0.7, zt], [cu + r1 * 0.7, cv - r1 * 0.7, zt], [cu + r1 * 0.7, cv + r1 * 0.7, zt], [cu - r1 * 0.7, cv + r1 * 0.7, zt]);
        line(g, lite(body, 0.4), 0.6, ...P(cu - r0, cv + r0, z), ...P(cu - r1, cv + r1, zt));
      },
    }];
  },

  // An enclosed conveyor bridge from the upper wall, sloping down to the ground beside the shed. It leaves
  // by a face without the door, so it never lands in front of the bay.
  conveyor({ S, R, H, s, accent }) {
    const [u0, v0, u1, v1] = R;
    const right = S.door !== 'R';
    const edge = right ? u1 : v1;
    const mid = right ? (v0 + v1) / 2 : (u0 + u1) / 2;
    // A point on the bridge: `run` out from the wall, `lat` across it, `z` up.
    const pt = (run, lat, z) => (right ? [edge + run, lat, z] : [lat, edge + run, z]);
    const w = 1.5 * s;
    const L = Math.max(3, Math.min(8.5 * s, 31.5 - edge));
    const zt0 = H - 2.6 * s;
    const zt1 = 2.6 * s;
    const zb0 = H - 5 * s;
    const zb1 = 0.2;
    return [{
      d: edge + L / 2 + mid,
      run() {
        const g = S.g;
        const a0 = mid - w;
        const a1 = mid + w;
        fp3(g, lite(accent, 0.12), pt(0, a0, zt0), pt(0, a1, zt0), pt(L, a1, zt1), pt(L, a0, zt1));
        fp3(g, accent, pt(0, a1, zb0), pt(L, a1, zb1), pt(L, a1, zt1), pt(0, a1, zt0));
        for (let k = 1; k < 6; k++) {
          const t = k / 6;
          line(g, 'rgba(20,30,30,.25)', 0.4, ...P(...pt(L * t, a1, zb0 + (zb1 - zb0) * t)), ...P(...pt(L * t, a1, zt0 + (zt1 - zt0) * t)));
        }
        fp3(g, dark(accent, 0.2), pt(L, a0, zb1), pt(L, a1, zb1), pt(L, a1, zt1), pt(L, a0, zt1));
        // a leg under the middle of the run, and a footing on the ground at the far end
        const zs = zb0 + (zb1 - zb0) * 0.5;
        line(g, '#6e7478', 1.1, ...P(...pt(L * 0.5, a1, zs)), ...P(...pt(L * 0.5, a1, 0)));
        fp3(g, '#8a9096', pt(L - 1.4, a0 - 0.5, 0), pt(L + 0.5, a0 - 0.5, 0), pt(L + 0.5, a1 + 0.5, 0), pt(L - 1.4, a1 + 0.5, 0));
      },
    }];
  },

  // A rainwater tank: a cylinder with hoops and a lit top.
  water_tank({ S, R, zAt, s, accent }) {
    const [, , u1, v1] = R;
    const cu = u1 - 3.5 * s;
    const cv = v1 - 3.5 * s;
    const z = zAt(cu, cv);
    return [{ d: cu + cv, run: () => cylinder(S.g, cu, cv, 1.8 * s, z, z + 3.6 * s, accent) }];
  },

  // A row of three roof ventilators: a short cylinder each, with a cowl.
  vents({ S, R, zAt, s, accent }) {
    const [u0, , , v1] = R;
    const out = [];
    for (let i = 0; i < 3; i++) {
      const cu = u0 + 3 * s + i * 3.4 * s;
      const cv = v1 - 2.6 * s;
      const z = zAt(cu, cv);
      out.push({
        d: cu + cv,
        run() {
          const g = S.g;
          cylinder(g, cu, cv, 0.8 * s, z, z + 2.2 * s, accent);
          const [x, y] = P(cu, cv, z + 2.2 * s);
          ell(g, lite(accent, 0.1), x, y - 0.8 * s, 1.3 * s * 1.414, 1.3 * s * 0.707);
        },
      });
    }
    return out;
  },
};

// --- Stacks: each draws from its base z up, with the same accent as the plant's stack colour ---------

function smokeFrom(S, cu, cv, z) {
  if (!S.info) return;
  const [x, y] = P(cu, cv, z);
  S.info.smoke.push([Math.round(x), Math.round(y)]);
}

const STACKS = {
  none() {},

  // A brick chimney, as the plant has always had: the same shape as world.js's brickChimney, in the stack colour.
  brick(S, cu, cv, zb, col) {
    const g = S.g;
    const w = 2.6;
    const top = zb + 12;
    const R = [cu - w / 2, cv - w / 2, cu + w / 2, cv + w / 2];
    box(S, R, zb, top, { k: 'brick', c: col });
    const [u0, v0, u1, v1] = R;
    fp3(g, '#7a6a62', [u0 - 0.8, v0 - 0.8, top], [u1 + 0.8, v0 - 0.8, top], [u1 + 0.8, v1 + 0.8, top], [u0 - 0.8, v1 + 0.8, top]);
    fp3(g, '#2b2528', [u0 + 0.6, v0 + 0.6, top], [u1 - 0.6, v0 + 0.6, top], [u1 - 0.6, v1 - 0.6, top], [u0 + 0.6, v1 - 0.6, top]);
    fp3(g, '#9a8d86', [u0 - 0.8, v1 + 0.8, top - 1.2], [u1 + 0.8, v1 + 0.8, top - 1.2], [u1 + 0.8, v1 + 0.8, top], [u0 - 0.8, v1 + 0.8, top]);
    smokeFrom(S, cu, cv, top);
    shadow(S, R, top - zb);
  },

  // A tall steel flue with rings, the steel tinted by the stack colour.
  steel(S, cu, cv, zb, col) {
    const g = S.g;
    const w = 1.8;
    const top = zb + 18;
    const R = [cu - w / 2, cv - w / 2, cu + w / 2, cv + w / 2];
    box(S, R, zb, top, { k: 'steel', c: mixHex(STEEL, col, 0.3) }, (side, len) => {
      for (let z = zb + 1.5; z < top - 0.6; z += 2.4) {
        rr(g, 'rgba(40,46,50,.45)', 0, -z, len, 0.6);
        rr(g, 'rgba(255,255,255,.25)', 0, -z + 0.6, len, 0.3);
      }
    });
    const [u0, v0, u1, v1] = R;
    fp3(g, '#5c646b', [u0 - 0.3, v0 - 0.3, top], [u1 + 0.3, v0 - 0.3, top], [u1 + 0.3, v1 + 0.3, top], [u0 - 0.3, v1 + 0.3, top]);
    fp3(g, '#24282c', [u0 + 0.3, v0 + 0.3, top], [u1 - 0.3, v0 + 0.3, top], [u1 - 0.3, v1 - 0.3, top], [u0 + 0.3, v1 - 0.3, top]);
    smokeFrom(S, cu, cv, top);
    shadow(S, R, top - zb);
  },

  // A squat clean-air vent tower with louvres, an accent band and a fan on top.
  vent(S, cu, cv, zb, col) {
    const g = S.g;
    const w = 3.6;
    const top = zb + 7;
    const R = [cu - w / 2, cv - w / 2, cu + w / 2, cv + w / 2];
    box(S, R, zb, top, { k: 'steel', c: '#d6dde0' }, (side, len) => {
      rr(g, col, 0, -top + 0.4, len, 1.5);
      for (let z = zb + 1.6; z < top - 2.4; z += 1.5) {
        rr(g, 'rgba(40,46,50,.5)', 1.2, -z - 1.0, len - 2.4, 0.7);
        rr(g, 'rgba(255,255,255,.4)', 1.2, -z - 0.3, len - 2.4, 0.3);
      }
    });
    const [x, y] = P(cu, cv, top);
    ell(g, '#3c4349', x, y, 2.4, 1.2);
    for (let k = 0; k < 3; k++) {
      const a = k * TAU / 3 + 0.3;
      line(g, '#e8ecef', 0.7, x, y, x + Math.cos(a) * 2.2, y + Math.sin(a) * 1.1);
    }
    ell(g, '#20262a', x, y, 0.45, 0.25);
    shadow(S, R, top - zb);
  },

  // A small wind turbine on a mast: three blades turning on the hub, with a tip in the stack colour.
  turbine(S, cu, cv, zb, col) {
    const g = S.g;
    const top = zb + 12;
    const R = [cu - 0.7, cv - 0.7, cu + 0.7, cv + 0.7];
    box(S, [cu - 1, cv - 1, cu + 1, cv + 1], zb, zb + 1.4, { k: 'steel', c: '#c9cdd0' });
    const [x0, y0] = P(cu, cv, zb + 1.4);
    const [x1, y1] = P(cu, cv, top);
    line(g, '#eef1f3', 1.3, x0, y0, x1, y1);
    box(S, [cu - 0.9, cv - 0.5, cu + 0.9, cv + 0.5], top - 0.8, top + 0.6, { k: 'steel', c: '#eef1f3' });
    for (let k = 0; k < 3; k++) {
      const a = -1.9 + (k * TAU) / 3;
      const tx = x1 + Math.cos(a) * 7;
      const ty = y1 + Math.sin(a) * 7;
      line(g, '#f2f5f7', 1.1, x1, y1, tx, ty);
      ell(g, col, tx, ty, 0.9, 0.9);
    }
    ell(g, '#dfe4e8', x1, y1, 1, 1);
    shadow(S, R, top - zb);
  },
};

// --- Doors ---------------------------------------------------------------------------------------------

// Where the door face's things stand, in the face's own x (from 0 at its left): the bay in the middle,
// the bay as wide as the door face allows, and a person door beside it when there is room. `extra` is
// what a bay takes beyond its door (a dock's platform).
function doorLayout(len, extra) {
  const pw = 2.4;
  const gap = 0.9;
  const cx = len / 2;
  let bw = Math.min(11, len * 0.5);
  const room = cx - 1.2 - pw - gap - extra / 2;
  if (bw / 2 + extra / 2 > room) bw = Math.max(3.5, room * 2 - extra);
  const outer = bw + extra;
  const right = cx + outer / 2 + gap;
  const left = cx - outer / 2 - gap - pw;
  let px = null;
  if (right + pw <= len - 1.2) px = right;
  else if (left >= 1.2) px = left;
  return { cx, bw, outer, px, pw, ew: Math.min(bw + 4, len - 3) };
}

// A soft light in the wall's plane, fading out, for lamps at night.
function lampGlow(g, x, y, r) {
  const gr = g.createRadialGradient(x, y, 0, x, y, r);
  gr.addColorStop(0, 'rgba(255,220,130,.55)');
  gr.addColorStop(0.4, 'rgba(255,200,110,.18)');
  gr.addColorStop(1, 'rgba(255,200,110,0)');
  g.fillStyle = gr;
  g.fillRect(x - r, y - r, r * 2, r * 2);
}

// A lamp over a bay at night: the glow, a cage and the bulb.
function lampOver(S, cx, top) {
  if (S.day) return;
  const g = S.g;
  lampGlow(g, cx, -top - 1.2, 9);
  rr(g, '#2c2f3c', cx - 0.9, -top - 1.6, 1.8, 0.8);
  rr(g, '#fff4b8', cx - 0.6, -top - 0.9, 1.2, 0.9);
}

// A roll-up door with slats, its foot at z0 and its height h, in a face's plane.
function rollerDoor(g, x0, w, z0, h, col, chain) {
  const top = -(z0 + h);
  const bot = -z0;
  rr(g, '#1c2220', x0, top, w, bot - top);
  for (let y = top; y < bot - 0.5; y += 0.85) {
    g.fillStyle = grad(g, 0, y, 0, y + 0.8, lite(col, 0.2), dark(col, 0.1));
    g.fillRect(x0, y, w, 0.75);
    rr(g, 'rgba(10,14,12,.45)', x0, y + 0.75, w, 0.1);
  }
  // Guide rails each side, the housing above and the shade it throws under it.
  rr(g, dark(col, 0.35), x0 - 0.9, top, 0.9, bot - top);
  rr(g, dark(col, 0.35), x0 + w, top, 0.9, bot - top);
  g.fillStyle = grad(g, 0, top - 1.8, 0, top, lite(col, 0.25), col, dark(col, 0.2));
  g.fillRect(x0 - 1, top - 1.8, w + 2, 1.8);
  rr(g, 'rgba(25,15,35,.4)', x0, top, w, 0.7);
  // A yellow and black threshold at the foot.
  for (let x = 0; x < w; x += 1.2) {
    rr(g, Math.floor(x / 1.2) % 2 ? '#2a2a2a' : '#e2b02a', x0 + x, bot - 0.8, Math.min(1.2, w - x), 0.8);
  }
  if (chain) {
    const cx = x0 + w - 1.1;
    line(g, '#2a2f2c', 0.35, cx, top + 0.8, cx, bot - 3.2);
    for (let y = top + 1.6; y < bot - 3; y += 0.9) ell(g, '#c9c3ae', cx, y, 0.32, 0.32);
  }
}

// A glazed sorting-hall front: in the day, the hall inside with a sloped conveyor and parcels on it; at
// night, the whole front lit warm.
function glassFront(S, x0, w, h, col) {
  const g = S.g;
  const top = -h;
  rr(g, dark(col, 0.1), x0 - 0.9, top - 0.9, w + 1.8, h + 0.9);
  if (!S.day) {
    rr(g, grad(g, 0, top, 0, 0, '#fff0b8', '#ffc260'), x0, top, w, h);
    for (let x = x0 + 2.6; x < x0 + w; x += 2.6) rr(g, 'rgba(120,70,20,.5)', x, top, 0.5, h);
    rr(g, 'rgba(120,70,20,.45)', x0, top + h * 0.45, w, 0.5);
    return;
  }
  rr(g, grad(g, 0, top, 0, 0, '#6b8a7c', '#2c3c35'), x0, top, w, h);
  for (let x = x0 + 1.2; x < x0 + w - 1; x += 3.2) rr(g, 'rgba(255,250,215,.85)', x, top + 0.8, 1.8, 0.5);
  const xa = x0 + 0.8;
  const xb = x0 + w - 0.8;
  const y0 = -2.6;
  const y1 = -4.9;
  const yAt = (x) => y0 + ((x - xa) / (xb - xa)) * (y1 - y0);
  fp(g, '#161c19', [[xa, y0], [xb, y1], [xb, y1 + 0.8], [xa, y0 + 0.8]]);
  const parcels = ['#c9a06a', '#3a7fb0', '#9aa0a8', '#c9a06a'];
  for (let i = 0; i < parcels.length; i++) {
    const x = xa + (xb - xa) * (0.14 + i * 0.22);
    rr(g, parcels[i], x, yAt(x) - 1.5, 1.6, 1.5);
  }
  for (let x = x0 + 2.4; x < x0 + w - 1; x += 3.6) rr(g, dark(col, 0.1), x, top, 0.5, h);
  rr(g, dark(col, 0.1), x0, top + h * 0.5, w, 0.5);
  fp(g, 'rgba(255,255,255,.15)', [[x0 + 0.5, top + 0.6], [x0 + 3.5, top + 0.6], [x0 + 0.5, top + 4.2]]);
}

// A person door with a small pane of glass and a handle, or its lamp at night.
function personDoor(S, x, w, col) {
  const g = S.g;
  const h = 7.2;
  if (!S.day) {
    lampGlow(g, x + w / 2, -h - 1.4, 7);
    rr(g, '#fff4b8', x + w / 2 - 0.5, -h - 1.8, 1, 0.8);
    return;
  }
  rr(g, dark(col, 0.3), x - 0.9, -h - 0.9, w + 1.8, h + 0.9);
  g.fillStyle = grad(g, x, 0, x + w, 0, lite(col, 0.12), col, dark(col, 0.18));
  g.fillRect(x, -h, w, h);
  rr(g, grad(g, 0, -h + 1.3, 0, -h + 3.4, GLASS_TOP, GLASS_BOT), x + 0.5, -h + 1.2, w - 1, 2.2);
  rr(g, '#d8d4c8', x + w - 0.7, -h * 0.42, 0.35, 1.5);
  rr(g, '#2c2f3c', x + w / 2 - 0.6, -h - 1.8, 1.2, 0.8);
}

// The emblem panel, a pale surround with a picture of the chosen kind in its middle.
function emblem(g, cx, e0, e1, w, word, col) {
  if (word === 'none' || !Object.hasOwn(EMBLEMS, word)) return;
  const top = -e1;
  const bot = -e0;
  const h = e1 - e0;
  const x0 = cx - w / 2;
  rr(g, '#d9d4c4', x0 - 0.8, top - 0.8, w + 1.6, h + 1.6);
  g.fillStyle = grad(g, 0, top, 0, bot, lite(col, 0.14), col, dark(col, 0.22));
  g.fillRect(x0, top, w, h);
  EMBLEMS[word](g, cx, (top + bot) / 2, Math.min(h * 0.42, 2.4), col);
}

// A row of windows across a face, from the storey's base z0 for hs units up. The row is spaced evenly,
// and any window overlapping a `skips` range (x0, x1) is left out.
function windowRow(S, len, z0, hs, word, col, number, skips) {
  const cfg = WINDOWS[word];
  const span = len - 4;
  const n = number === null ? Math.max(1, Math.floor((span + cfg.gap) / (cfg.w + cfg.gap))) : number;
  if (n <= 0 || span <= 0) return;
  const bay = span / n;
  const ww = Math.min(cfg.w, bay * 0.72);
  const z = cfg.at(z0, hs);
  for (let i = 0; i < n; i++) {
    const x = 2 + bay * (i + 0.5) - ww / 2;
    if (skips.some(([a, b]) => x < b && x + ww > a)) continue;
    if (word === 'louvres') louvres(S, x, z, ww, cfg.h, col);
    else win(S, x, z, ww, cfg.h, { frame: col, lit: cfg.lit });
  }
}

// Louvred vents: a dark frame, slats across it and a sill. Daytime only.
function louvres(S, x, z, w, h, col) {
  if (!S.day) return;
  const g = S.g;
  const t = -(z + h);
  rr(g, dark(col, 0.2), x - 1, t - 1, w + 2, h + 2);
  rr(g, '#1c2220', x, t, w, h);
  for (let y = t + 0.2; y < t + h - 0.5; y += 0.9) {
    rr(g, lite(col, 0.18), x + 0.2, y, w - 0.4, 0.55);
    rr(g, dark(col, 0.2), x + 0.2, y + 0.55, w - 0.4, 0.35);
  }
  rr(g, lite(col, 0.1), x - 1.2, -z, w + 2.4, 1);
}

// The dock's platform: a concrete slab out from the door face, with a yellow nose and a dark end.
// It is painted in tile space, after the walls, since it stands out from them.
function dockPlatform(S, rect, lay) {
  const g = S.g;
  const [u0, v0, u1, v1] = rect;
  const L = S.door === 'L';
  // A point on the door face: x along it, z up and d out from the wall.
  const T = (x, z, d) => (L ? [u0 + x, v1 + d, z] : [u1 + d, v1 - x, z]);
  const xa = lay.cx - lay.outer / 2;
  const xb = lay.cx + lay.outer / 2;
  const dd = 3.4;
  const zt = 3.2;
  fp3(g, lite(PLATFORM, 0.12), T(xa, zt, 0), T(xb, zt, 0), T(xb, zt, dd), T(xa, zt, dd));
  fp3(g, PLATFORM, T(xa, 0, dd), T(xb, 0, dd), T(xb, zt, dd), T(xa, zt, dd));
  fp3(g, '#e2b02a', T(xa, zt - 0.6, dd), T(xb, zt - 0.6, dd), T(xb, zt, dd), T(xa, zt, dd));
  const xe = L ? xb : xa;
  fp3(g, dark(PLATFORM, 0.3), T(xe, 0, 0), T(xe, 0, dd), T(xe, zt, dd), T(xe, zt, 0));
  line(g, 'rgba(40,40,40,.35)', 0.4, ...P(...T(xa, 1.6, dd)), ...P(...T(xb, 1.6, dd)));
}

// --- The building ---------------------------------------------------------------------------------------

// The plant's building on `rect` (u0, v0, u1, v1), its floors from the body and its parts from `outside`.
export function paintPlantBuilding(S, outside, rect, c) {
  const g = S.g;
  const [u0, v0, u1, v1] = rect;
  const floors = Math.min(3, Math.max(1, bodyOf('plant', outside)?.floors || 1));
  const H = STOREY + UPPER * (floors - 1);

  const cladWord = styleOf(outside, 'plant_cladding', Object.keys(CLADDING), PLANT_DEFAULTS.plant_cladding);
  const wall = colorOf(outside, 'plant_cladding', PLANT_WALL);
  const winWord = styleOf(outside, 'plant_windows', Object.keys(WINDOWS), PLANT_DEFAULTS.plant_windows);
  const winCol = colorOf(outside, 'plant_windows', '#3a3f38');
  const winNum = numberOf(outside, 'plant_windows', null);
  const bayWord = styleOf(outside, 'plant_bay', Object.keys(BAYS), PLANT_DEFAULTS.plant_bay);
  const bayCol = colorOf(outside, 'plant_bay', '#3f4a3c');
  const bay = BAYS[bayWord];
  const emblemWord = styleOf(outside, 'plant_emblem', Object.keys(EMBLEMS), PLANT_DEFAULTS.plant_emblem);
  const emblemCol = colorOf(outside, 'plant_emblem', '#2e7d32');
  const personCol = c && typeof c.door === 'string' ? c.door : '#2f5d9a';

  const cladPaint = CLADDING[cladWord];
  const mat = { k: 'steel', c: wall, paint: (gg, len, z0, z1, rand) => cladPaint(gg, len, -z1, -z0, wall, rand) };

  const doorLen = S.door === 'L' ? u1 - u0 : v1 - v0;
  const lay = doorLayout(doorLen, bay.extra);
  const emblemTop = bay.top + 0.4;
  const emblemOn = emblemWord !== 'none' && 12.6 - emblemTop >= 1.5;
  const doorSkips = [[lay.cx - lay.ew / 2 - 0.8, lay.cx + lay.ew / 2 + 0.8]];
  if (lay.px !== null) doorSkips.push([lay.px - 1.2, lay.px + lay.pw + 1.2]);

  shadow(S, rect, H);
  for (let k = 0; k < floors; k++) {
    const z0 = k === 0 ? 0 : STOREY + UPPER * (k - 1);
    const z1 = k === 0 ? STOREY : z0 + UPPER;
    box(S, rect, z0, z1, mat, (side, len) => {
      const onDoor = side === S.door;
      if (S.day) {
        if (k === 0) rr(g, dark(wall, 0.3), 0, -1.6, len, 1.6);
        rr(g, lite(wall, 0.22), 0, -z1, len, 1.1);
      }
      let skips = [];
      if (onDoor && k === 0) {
        bay.draw(S, lay, bayCol);
        if (lay.px !== null) personDoor(S, lay.px, lay.pw, personCol);
        if (S.day && emblemOn) emblem(g, lay.cx, emblemTop, 12.6, lay.ew, emblemWord, emblemCol);
        skips = doorSkips;
      }
      windowRow(S, len, z0, z1 - z0, winWord, winCol, winNum === null ? null : Math.min(6, Math.max(0, winNum)), skips);
    });
  }

  if (!S.day) return;
  if (bayWord === 'dock') dockPlatform(S, rect, lay);

  const roofWord = styleOf(outside, 'plant_roof', Object.keys(ROOFS), PLANT_DEFAULTS.plant_roof);
  const roofCol = colorOf(outside, 'plant_roof', '#7c9a76');
  const roof = ROOFS[roofWord](S, rect, H, roofCol);

  // The stack and the rooftop kit stand on the roof's deck, painted back to front.
  const L = u1 - u0;
  const D = v1 - v0;
  const s = Math.min(1.3, Math.max(0.6, Math.min(L, D) / 16));
  const accent = colorOf(outside, 'plant_rooftop', '#9aa59a');
  const ctx = { S, R: rect, H, zAt: roof.zAt, s, accent };
  const items = [];
  const stackWord = styleOf(outside, 'plant_stack', Object.keys(STACKS), PLANT_DEFAULTS.plant_stack);
  const stackCol = colorOf(outside, 'plant_stack', '#a95e45');
  if (stackWord !== 'none') {
    const cu = u1 - 3.8 * s;
    const cv = v0 + 3.6 * s;
    items.push({ d: cu + cv, run: () => STACKS[stackWord](S, cu, cv, roof.zAt(cu, cv), stackCol) });
  }
  for (const word of ROOFTOP) {
    if (!isOn(outside, 'plant_rooftop', word)) continue;
    items.push(...ROOFTOP_ITEMS[word](ctx));
  }
  items.sort((p, q) => p.d - q.d);
  for (const it of items) it.run();
}

// --- The yard -------------------------------------------------------------------------------------------

// A wheelie bin with its lid, wheels and a recycling triangle on the front.
function wheelieBin(K, a0, a1, col) {
  const m = (a0 + a1) / 2;
  K.disc(a0 + 0.2, 0.45, 0.45, '#1f2326', 0.45);
  K.disc(a1 - 0.2, 0.45, 0.45, '#1f2326', 0.45);
  K.box(a0, 0.3, a1, 1.7, 0.4, 3.0, col);
  K.box(a0 - 0.05, 0.25, a1 + 0.05, 1.75, 3.0, 3.4, lite(col, 0.12));
  K.q(CREAM, [0, 1, 0], [m - 0.34, 1.7, 1.5], [m + 0.34, 1.7, 1.5], [m, 1.7, 2.3]);
  K.q(col, [0, 1, 0], [m - 0.18, 1.7, 1.6], [m + 0.18, 1.7, 1.6], [m, 1.7, 2.0]);
}

// One crushed bale: a cube with two straps on its front and side, and a few flecks of other colour.
function bale(K, a, b, layer, col) {
  const z0 = layer * 1.9;
  const z1 = z0 + 1.9;
  const a1 = a + 1.85;
  const b1 = b + 1.85;
  K.box(a, b, a1, b1, z0, z1, col);
  const m = (a + a1) / 2;
  const n = (b + b1) / 2;
  K.q('#3a3d42', [0, 1, 0], [m - 0.06, b1, z0 + 0.1], [m + 0.06, b1, z0 + 0.1], [m + 0.06, b1, z1 - 0.1], [m - 0.06, b1, z1 - 0.1]);
  K.q('#3a3d42', [1, 0, 0], [a1, n - 0.06, z0 + 0.1], [a1, n + 0.06, z0 + 0.1], [a1, n + 0.06, z1 - 0.1], [a1, n - 0.06, z1 - 0.1]);
  K.q(lite(col, 0.25), [0, 1, 0], [a + 0.3, b1, z0 + 0.4], [a + 0.6, b1, z0 + 0.4], [a + 0.6, b1, z0 + 0.7], [a + 0.3, b1, z0 + 0.7]);
  K.q(dark(col, 0.3), [0, 1, 0], [a1 - 0.5, b1, z1 - 0.6], [a1 - 0.2, b1, z1 - 0.6], [a1 - 0.2, b1, z1 - 0.3], [a1 - 0.5, b1, z1 - 0.3]);
}

// A soft wisp of steam: a vertical radial gradient, fading to nothing.
function wisp(g, x, y, r) {
  g.save();
  g.translate(x, y);
  g.scale(0.6, 1.7);
  const gr = g.createRadialGradient(0, 0, 0, 0, 0, r);
  gr.addColorStop(0, 'rgba(255,255,255,.3)');
  gr.addColorStop(1, 'rgba(255,255,255,0)');
  g.fillStyle = gr;
  g.fillRect(-r, -r, r * 2, r * 2);
  g.restore();
}

export const PLANT_YARD_ART = {
  // Four wheelie bins in a row, blue, green, yellow and grey.
  recycling_bins(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    shadow(S, K.rect(), 4);
    K.contact(0.8);
    const cols = ['#2f6fbf', '#3f8f4a', '#e8c020', '#6f7378'];
    K.parts(cols.map((col, k) => {
      const a0 = 0.3 + k * 1.4;
      const a1 = a0 + 1.1;
      return [(a0 + a1) / 2, 1, () => wheelieBin(K, a0, a1, col)];
    }));
  },

  // Crushed bales stacked in three layers, in mixed colours and strapped.
  bale_stack(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    shadow(S, K.rect(), 6);
    K.contact(0.9);
    const cols = ['#b8925a', '#e6e2d4', '#4c7fb0', '#5f9a4f', '#8a8f98', '#c9c4b0', '#a94f3f'];
    const cubes = [
      [0.1, 0.1, 0], [2.05, 0.1, 0], [0.1, 2.05, 0], [2.05, 2.05, 0],
      [0.1, 0.1, 1], [2.05, 0.1, 1], [0.1, 2.05, 1],
      [0.6, 0.6, 2],
    ];
    K.parts(cubes.map(([a, b, layer], i) => [a + 0.9, b + 0.9, () => bale(K, a, b, layer, cols[i % cols.length]), layer]));
  },

  // An open steel skip with a sloped front end, some scrap inside and a rebar end sticking up.
  skip_bin(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const col = hexOf(item.color, '#e0a020');
    shadow(S, K.rect(), 4);
    K.contact(0.8);
    K.parts([
      [2, 0.3, () => K.box(0.2, 0.15, 3.8, 0.35, 0, 2.2, col), 0],
      [2, 1, () => K.box(0.35, 0.35, 3.65, 1.65, 0, 0.5, dark(col, 0.55)), 0],
      [0.3, 1, () => K.box(0.15, 0.35, 0.35, 1.65, 0, 2.2, col), 0],
      [1.4, 1.1, () => K.box(0.8, 0.6, 2.0, 1.4, 0.5, 1.5, '#8c6a4c'), 1],
      [2.7, 0.9, () => K.box(2.2, 0.5, 3.2, 1.2, 0.5, 2.0, '#a4a9b0'), 1],
      [2.0, 1.2, () => K.box(1.4, 0.9, 2.4, 1.5, 1.4, 2.6, '#6e7680'), 1],
      [3, 1, () => K.seg('#5c5f66', 0.3, [2.9, 1.0, 1.6], [3.5, 1.2, 2.9]), 1],
      [2, 1.8, () => K.box(0.2, 1.65, 3.8, 1.85, 0, 2.2, col), 2],
      [3.8, 1, () => K.q(col, [1, 0, 0.6], [3.8, 0.2, 0.3], [3.8, 1.8, 0.3], [3.3, 1.8, 2.2], [3.3, 0.2, 2.2]), 2],
      [2, 1.85, () => {
        for (const a of [1, 2, 3]) K.seg(dark(col, 0.25), 0.25, [a, 1.86, 0.3], [a, 1.86, 2.1]);
        K.seg(lite(col, 0.3), 0.5, [0.2, 1.85, 2.3], [3.8, 1.85, 2.3]);
      }, 2],
    ]);
  },

  // A long dark compost windrow, lumpy and warm, with soft steam rising from it by day.
  compost_windrow(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    shadow(S, K.rect(), 3);
    K.contact(0.9);
    // A long low ridge, sloping in from the ground on every side, with its crest along the middle.
    const soil = '#33251a';
    const z = 1.6;
    K.q(soil, [0, -1, 1], [0.2, 0.2, 0], [5.8, 0.2, 0], [5.2, 1, z], [0.8, 1, z]);
    K.q(soil, [-1, 0, 1], [0.2, 0.2, 0], [0.8, 1, z], [0.2, 1.8, 0]);
    K.q(soil, [1, 0, 1], [5.8, 0.2, 0], [5.8, 1.8, 0], [5.2, 1, z]);
    K.q(soil, [0, 1, 1], [0.2, 1.8, 0], [0.8, 1, z], [5.2, 1, z], [5.8, 1.8, 0]);
    // Clumps of turned compost and pale flecks of straw along the crest.
    for (let i = 0; i < 9; i++) {
      const a = 1 + i * 0.5;
      const b = 0.75 + (i % 3) * 0.25;
      K.ball(a, b, z * (1 - Math.abs(b - 1) * 1.2) + 0.1, 0.32, i % 2 ? '#3b2a1c' : '#5a4128');
      K.seg('#c9a86a', 0.18, [a - 0.15, b + 0.35, z * 0.55], [a + 0.15, b + 0.45, z * 0.5]);
    }
    for (let i = 0; i < 3; i++) {
      const [x, y] = K.at(1.6 + i * 1.4, 1, z + 0.6);
      wisp(S.g, x, y, 3 + i * 0.4);
    }
  },

  // A belt conveyor on legs, sloping up to the right, with cardboard boxes and bottles on it.
  sorting_conveyor(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const frame = hexOf(item.color, '#2f6f9f');
    const belt = '#262b30';
    const zAt = (a) => 1.2 + ((a - 0.3) * 2.2) / 5.4;
    shadow(S, K.rect(), 4);
    K.contact(0.7);
    for (const a of [0.8, 3.0, 5.2]) K.rod(a, 1.0, 0, zAt(a) - 0.5, 0.45, dark(frame, 0.2));
    K.q(belt, [-0.3, 0, 1], [0.3, 0.3, zAt(0.3)], [5.7, 0.3, zAt(5.7)], [5.7, 1.7, zAt(5.7)], [0.3, 1.7, zAt(0.3)]);
    for (let a = 0.8; a < 5.6; a += 0.6) K.seg('rgba(230,235,240,.2)', 0.3, [a, 0.35, zAt(a)], [a, 1.65, zAt(a)]);
    K.disc(0.3, 1.0, 0.6, '#1d2226', zAt(0.3));
    K.disc(5.7, 1.0, 0.6, '#1d2226', zAt(5.7));
    const things = [
      [1.2, 0.9, 'box', '#c9a06a'], [1.9, 1.2, 'cyl', '#3a7fb0'], [2.8, 0.8, 'box', '#c4c9cf'],
      [3.6, 1.1, 'cyl', '#4a9a5a'], [4.4, 0.9, 'box', '#c9a06a'], [5.1, 1.2, 'cyl', '#c4c9cf'],
    ];
    for (const [a, b, kind, col] of things) {
      const z = zAt(a);
      if (kind === 'box') K.box(a, b - 0.3, a + 0.7, b + 0.3, z, z + 0.7, col);
      else K.cyl(a + 0.3, b, 0.3, z, z + 0.9, col);
    }
    K.q(frame, [0, 1, 0], [0.3, 1.7, zAt(0.3) - 0.9], [5.7, 1.7, zAt(5.7) - 0.9], [5.7, 1.7, zAt(5.7)], [0.3, 1.7, zAt(0.3)]);
  },
};
