// The research lab's outside (docs/outside.md): a modern lab of pale panels and glass, with science kit on
// its roof and yard items to match. Everything is painted in code, and nothing here draws a word: an
// emblem is a picture on a plaque.
//
// world.js and outside-decor.js import from each other, so nothing here calls an imported function while
// this module loads. Every painter runs inside an exported entry.

import { bodyOf, colorOf, hexOf, numberOf, styleOf } from './outside-spec.js';
import { ball, glow, kit, pool } from './outside-decor.js';
import { P, box, dark, ell, fp, fp3, grad, ih, lite, line, mixHex, onL, onR, rgba, rr, shadow } from './world.js';

const TAU = 6.2832;
const DOOR_W = 4.8;
const DOOR_H = 8.8;
const FRAME = '#eef5f3';

// The words each part takes. The default of each is in LAB_DEFAULTS.
const FACADE_WORDS = ['panels', 'curtain_glass', 'brick_glass', 'copper', 'timber_glass'];
const WINDOW_WORDS = ['ribbon', 'grid', 'portholes', 'fins'];
const ROOF_WORDS = ['deck', 'sawtooth', 'green', 'vault'];
const WING_WORDS = ['none', 'glass_wing', 'observatory', 'greenhouse', 'clean_tower'];
const ENTRY_WORDS = ['sliding', 'airlock', 'canopy', 'ramp'];
const EMBLEM_WORDS = ['none', 'flask', 'atom', 'dna', 'microscope', 'leaf', 'brain'];
const GLOW_WORDS = ['cool', 'warm', 'all', 'off'];
const ROOFTOP_WORDS = ['fume_stacks', 'air_handler', 'weather_mast', 'solar_array', 'uplink_dish', 'wind_turbine'];

export const LAB_DEFAULTS = {
  lab_facade: 'panels',
  lab_windows: 'ribbon',
  lab_roof: 'deck',
  lab_wing: 'glass_wing',
  lab_entry: 'sliding',
  lab_emblem: 'flask',
  lab_glow: 'cool',
};
export const LAB_ROOFTOP_DEFAULT = ['air_handler'];

// lab_glow: the light colour, and how many windows in a hundred light up at night.
const GLOW = { cool: ['#e4fbff', 70], warm: ['#ffd58a', 70], all: ['#eefaff', 100], off: ['#ffd58a', 0] };

// lab_windows: the widest one bay can be, the gap between bays, and for a ribbon the length of a band.
const WIN = {
  ribbon: { w: 99, gap: 1.2, pitch: 16 },
  grid: { w: 3.6, gap: 2.2 },
  portholes: { w: 3.4, gap: 2.6 },
  fins: { w: 2.6, gap: 2.4 },
};

// Storeys: the first is 16 units tall and each one above is 11.
const storeyBase = (k) => (k === 0 ? 0 : 16 + 11 * (k - 1));

// Whether one window lights at night. It is picked from where the window is, so it never shimmers.
const litAt = (S, x, z, pct, salt) => ih(S.seed + salt, Math.round(x * 3), Math.round(z * 3)) % 100 < pct;

// --- The facade: what the wall is made of, painted in a face's own plane (y = -z) by day ---------

// A wall's face is clipped to its own height before any of this is painted.
function clipFace(g, len, H) {
  g.beginPath();
  g.rect(-0.5, -H - 0.5, len + 1, H + 1);
  g.clip();
}

function faceBase(g, len, H, col) {
  rr(g, grad(g, 0, -H, 0, 0, lite(col, 0.08), col, dark(col, 0.1)), 0, -H, len, H);
}

const FACADES = {
  // White composite panels with fine dark seams, in courses one storey tall.
  panels(g, len, H, f) {
    rr(g, rgba(dark(f.wall, 0.35), 0.8), 0, -H, len, H);
    for (let a = 0, i = 0; a < len; a += 3.2, i++) {
      for (let c = 0, k = 0; c < H; c += 5.5, k++) {
        const w = Math.min(3.2, len - a);
        const h = Math.min(5.5, H - c);
        const tone = (ih(f.seed, i, k) % 9) * 0.01;
        rr(g, lite(f.wall, 0.06 + tone), a + 0.25, -(c + h) + 0.25, w - 0.5, h - 0.5);
      }
    }
    for (const z of f.floors) line(g, rgba(dark(f.wall, 0.5), 0.6), 0.9, 0, -z, len, -z);
  },

  // A full glass curtain wall with thin aluminium mullions and a transom at each floor.
  curtain_glass(g, len, H, f) {
    const glass = mixHex(f.tint, '#7fb0c6', 0.35);
    rr(g, grad(g, 0, -H, 0, 0, lite(glass, 0.5), glass, dark(glass, 0.25)), 0, -H, len, H);
    fp(g, 'rgba(255,255,255,.2)', [[len * 0.15, -H], [len * 0.32, -H], [len * 0.12, 0], [len * -0.05, 0]]);
    for (let a = 2.2; a < len; a += 2.2) rr(g, f.frame, a - 0.25, -H, 0.5, H);
    for (const z of f.floors) rr(g, f.frame, 0, -z - 0.5, len, 1);
  },

  // Brick piers with tall glass bays between them, four and a bit units wide.
  brick_glass(g, len, H, f) {
    const brick = mixHex(f.wall, '#9c5a45', 0.55);
    rr(g, '#d9cbbd', 0, -H, len, H);
    for (let c = 0, k = 0; c < H; c += 1.5, k++) {
      for (let a = (k % 2) * 1.6 - 1.6; a < len; a += 3.2) {
        const x0 = Math.max(0, a);
        const x1 = Math.min(len, a + 3.2);
        if (x1 <= x0) continue;
        const col = ih(f.seed, k, Math.round(a * 2)) % 3 === 0 ? dark(brick, 0.1) : brick;
        rr(g, col, x0 + 0.1, -(c + 1.5) + 0.12, x1 - x0 - 0.2, 1.26);
      }
    }
    for (let a0 = 2.2; a0 + 4.4 <= len + 0.01; a0 += 6.6) {
      const a1 = a0 + 4.4;
      if (f.dz && a1 > f.dz[0] && a0 < f.dz[1]) continue;
      const top = -(H - 1.6);
      const bot = -1.6;
      rr(g, grad(g, 0, top, 0, bot, lite(f.tint, 0.4), f.tint, dark(f.tint, 0.25)), a0 + 0.15, top, 4.1, bot - top);
      rr(g, f.frame, a0 + 2.2 - 0.25, top, 0.5, bot - top);
      for (const z of f.floors) rr(g, f.frame, a0, -z - 0.3, 4.4, 0.6);
    }
  },

  // Patinated copper: a green-grey blend of the wall colour, with standing seams and a few streaks.
  copper(g, len, H, f) {
    const patina = mixHex(f.wall, '#3f9f88', 0.62);
    faceBase(g, len, H, patina);
    for (let a = 0.8; a < len; a += 1.6) {
      rr(g, rgba(dark(patina, 0.4), 0.45), a - 0.3, -H, 0.5, H);
      rr(g, 'rgba(255,255,255,.18)', a + 0.2, -H, 0.4, H);
    }
    for (let i = 0; i < len / 6; i++) {
      const a = ((ih(f.seed, i, 9) % 1000) / 1000) * len;
      rr(g, rgba(lite(patina, 0.5), 0.14), a, -H, 1.8, H);
    }
    for (const z of f.floors) rr(g, rgba(dark(patina, 0.5), 0.45), 0, -z - 0.3, len, 0.7);
  },

  // Vertical timber battens over the wall, with the glass set in them.
  timber_glass(g, len, H, f) {
    const wood = '#b98a58';
    faceBase(g, len, H, f.wall);
    for (let a = 0.6; a < len; a += 1.9) {
      rr(g, rgba(dark(wood, 0.6), 0.4), a + 0.9, -H, 0.35, H);
      rr(g, grad(g, a, 0, a + 0.9, 0, lite(wood, 0.25), wood, dark(wood, 0.25)), a, -H, 0.9, H);
    }
    for (const z of f.floors) rr(g, dark(wood, 0.15), 0, -z - 0.5, len, 0.9);
  },
};

// --- Windows, bays and the door ------------------------------------------------------------------

// Where the bays of one storey go along a face: [x, width] pairs, leaving `skip` clear on the ground floor.
function bayRuns(style, len, n, skip) {
  const cfg = WIN[style];
  const m = 1.8;
  const spans = skip ? [[m, skip[0] - 1.2], [skip[1] + 1.2, len - m]] : [[m, len - m]];
  const total = spans.reduce((t, [a, b]) => t + Math.max(0, b - a), 0);
  const out = [];
  if (total <= 0) return out;
  for (const [a, b] of spans) {
    const L = b - a;
    if (L < 2) continue;
    let cnt;
    if (n === undefined) {
      cnt = style === 'ribbon'
        ? Math.max(1, Math.round((L + cfg.gap) / cfg.pitch))
        : Math.floor((L + cfg.gap) / (cfg.w + cfg.gap));
    } else cnt = Math.round((n * L) / total);
    if (cnt < 1) continue;
    const pw = Math.min(cfg.w, (L - cfg.gap * (cnt - 1)) / cnt);
    if (pw < 0.8) continue;
    const span = pw * cnt + cfg.gap * (cnt - 1);
    const x0 = a + (L - span) / 2;
    for (let i = 0; i < cnt; i++) out.push([x0 + i * (pw + cfg.gap), pw]);
  }
  return out;
}

// A framed pane in the face plane: glass with a sheen, mullions and a sill.
function pane(g, x, z0, w, h, tint, frame, vert, horiz) {
  const t = -(z0 + h);
  rr(g, 'rgba(25,15,35,.22)', x - 1, t - 0.6, w + 2, h + 1.6);
  rr(g, frame, x - 0.7, t - 0.7, w + 1.4, h + 1.4);
  rr(g, grad(g, 0, t, 0, t + h, lite(tint, 0.5), tint, dark(tint, 0.22)), x, t, w, h);
  fp(g, 'rgba(255,255,255,.26)', [[x + w * 0.08, t], [x + w * 0.36, t], [x + w * 0.1, t + h * 0.55], [x, t + h * 0.55]]);
  for (let i = 1; i <= vert; i++) rr(g, frame, x + (w * i) / (vert + 1) - 0.25, t, 0.5, h);
  for (let j = 1; j <= horiz; j++) rr(g, frame, x, t + (h * j) / (horiz + 1) - 0.25, w, 0.5);
  rr(g, lite(frame, 0.1), x - 1, -z0, w + 2, 0.9);
}

// One window bay of one storey, by day as glass and at night as light (if it lights).
function windowBay(S, lab, x, w, k) {
  const g = S.g;
  const zb = storeyBase(k);
  const ground = k === 0;
  const lit = litAt(S, x, zb, lab.pct, 11 + k);
  if (lab.win === 'portholes') {
    const r = w / 2;
    const cx = x + r;
    const cy = ground ? -8.6 : -(zb + 5.6);
    if (S.day) {
      ell(g, FRAME, cx, cy, r + 0.7, r + 0.7);
      const gr = g.createRadialGradient(cx - r * 0.3, cy - r * 0.3, r * 0.1, cx, cy, r);
      gr.addColorStop(0, lite(lab.tint, 0.5));
      gr.addColorStop(0.6, lab.tint);
      gr.addColorStop(1, dark(lab.tint, 0.3));
      ell(g, gr, cx, cy, r, r);
      line(g, FRAME, 0.4, cx - r, cy, cx + r, cy);
    } else if (lit) {
      const gr = g.createRadialGradient(cx, cy, 0, cx, cy, r);
      gr.addColorStop(0, '#fffbe8');
      gr.addColorStop(0.5, lab.lightC);
      gr.addColorStop(1, rgba(lab.lightC, 0.6));
      ell(g, gr, cx, cy, r, r);
    }
    return;
  }
  // A ribbon is a shallow band; the other windows stand taller.
  const band = lab.win === 'ribbon';
  const z0 = band ? (ground ? 6 : zb + 3.2) : (ground ? 4.5 : zb + 2.4);
  const h = band ? (ground ? 5.6 : 5) : (ground ? 8 : 6.8);
  if (!S.day) {
    if (!lit) return;
    const t = -(z0 + h);
    rr(g, grad(g, 0, t, 0, t + h, lite(lab.lightC, 0.45), lab.lightC), x, t, w, h);
    rr(g, 'rgba(40,30,20,.3)', x + w / 2 - 0.2, t, 0.4, h);
    return;
  }
  if (lab.win === 'fins') {
    // A fin either side of the window, standing a little proud of the wall.
    const t = -(z0 + h);
    rr(g, rgba(dark(lab.wall, 0.4), 0.3), x - 1.1 + 0.6, t - 0.8, 0.5, h + 1.8);
    rr(g, rgba(dark(lab.wall, 0.4), 0.3), x + w + 0.8, t - 0.8, 0.5, h + 1.8);
    rr(g, lite(lab.wall, 0.12), x - 1.1, t - 0.8, 0.9, h + 1.8);
    rr(g, lite(lab.wall, 0.12), x + w + 0.2, t - 0.8, 0.9, h + 1.8);
    pane(g, x, z0, w, h, lab.tint, FRAME, 0, 0);
    return;
  }
  if (lab.win === 'grid') pane(g, x, z0, w, h, lab.tint, FRAME, w > 3 ? 1 : 0, ground ? 1 : 0);
  else pane(g, x, z0, w, h, lab.tint, FRAME, Math.max(0, Math.round(w / 3.6) - 1), 0);
}

// Lit cells of the glazing that a curtain wall or a brick-and-glass wall carries, at night.
function nightCells(S, lab, len, H, floors, dz) {
  const g = S.g;
  const brick = lab.facade === 'brick_glass';
  const cols = [];
  if (brick) {
    for (let a = 2.2; a + 4.4 <= len + 0.01; a += 6.6) cols.push([a, a + 4.4]);
  } else {
    for (let a = 0; a < len - 0.5; a += 2.2) cols.push([a, Math.min(len, a + 2.2)]);
  }
  for (const [a0, a1] of cols) {
    if (brick && dz && a1 > dz[0] && a0 < dz[1]) continue;
    for (let k = 0; k < floors; k++) {
      if (!litAt(S, a0, k, lab.pct, 31)) continue;
      const z0 = storeyBase(k) + 1.2;
      const z1 = (k + 1 < floors ? storeyBase(k + 1) : H) - 1.2;
      const t = -z1;
      const h = z1 - z0;
      rr(g, grad(g, 0, t, 0, t + h, lite(lab.lightC, 0.45), lab.lightC), a0 + 0.2, t, a1 - a0 - 0.4, h);
      rr(g, 'rgba(40,30,20,.25)', (a0 + a1) / 2 - 0.2, t, 0.4, h);
    }
  }
}

// The entrance door, on the face's plane. `x` is its left edge and `zb` its sill.
function entryDoor(S, lab, x, zb, w, h, kind) {
  const g = S.g;
  const t = -(zb + h);
  if (!S.day) {
    rr(g, grad(g, 0, t, 0, t + h, '#fffbe6', lab.lightC), x, t, w, h);
    rr(g, 'rgba(40,30,20,.35)', x + w / 2 - 0.2, t, 0.4, h);
    return;
  }
  rr(g, 'rgba(25,15,35,.25)', x - 1.5, t - 1.2, w + 3, h + 1.6);
  rr(g, '#e8f0ee', x - 1.1, t - 1.1, w + 2.2, h + 1.2);
  rr(g, grad(g, 0, t, 0, t + h, lite(lab.entryC, 0.45), lab.entryC, dark(lab.entryC, 0.3)), x, t, w, h);
  fp(g, 'rgba(255,255,255,.28)', [[x + w * 0.08, t], [x + w * 0.3, t], [x + w * 0.06, t + h * 0.6], [x, t + h * 0.6]]);
  rr(g, '#dfe9e6', x + w / 2 - 0.3, t, 0.6, h);
  if (kind === 'canopy') rr(g, '#dfe9e6', x, t + h * 0.3, w, 0.5);
  rr(g, '#a8b8b4', x + w / 2 - 1.3, t + h * 0.42, 0.4, 2.6);
  rr(g, '#a8b8b4', x + w / 2 + 0.9, t + h * 0.42, 0.4, 2.6);
  rr(g, '#c9d4d1', x - 1.6, -zb - 0.9, w + 3.2, 0.9);
}

// Pictures for the emblem plaque, drawn about the origin at scale s. They are pictures, not letters.
const EMBLEMS = {
  flask(g, col, s) {
    fp(g, col, [[-0.35 * s, -1.2 * s], [0.35 * s, -1.2 * s], [0.35 * s, -0.35 * s], [1.0 * s, 1.0 * s], [-1.0 * s, 1.0 * s], [-0.35 * s, -0.35 * s]]);
    fp(g, '#54e1cf', [[-0.75 * s, 0.25 * s], [0.75 * s, 0.25 * s], [0.9 * s, 0.92 * s], [-0.9 * s, 0.92 * s]]);
    ell(g, '#fff7dc', 0.3 * s, 0.6 * s, 0.22 * s, 0.22 * s);
  },
  atom(g, col, s) {
    for (const a of [0, 1.047, 2.094]) {
      g.save();
      g.rotate(a);
      g.beginPath();
      g.ellipse(0, 0, 1.45 * s, 0.55 * s, 0, 0, TAU);
      g.strokeStyle = col;
      g.lineWidth = 0.42;
      g.stroke();
      g.restore();
    }
    ell(g, '#ffd66b', 0, 0, 0.36 * s, 0.36 * s);
  },
  dna(g, col, s) {
    for (let i = 0; i <= 20; i++) {
      const t = i / 20;
      const y = (-1.25 + t * 2.5) * s;
      const x = Math.sin(t * TAU * 1.5) * 0.85 * s;
      ell(g, col, x, y, 0.2 * s, 0.2 * s);
      ell(g, '#7fe0d4', -x, y, 0.2 * s, 0.2 * s);
      if (i % 3 === 0) line(g, rgba(col, 0.7), 0.35, x, y, -x, y);
    }
  },
  microscope(g, col, s) {
    g.save();
    g.rotate(-0.45);
    rr(g, col, -0.22 * s, -1.25 * s, 0.44 * s, 1.7 * s);
    rr(g, dark(col, 0.2), -0.3 * s, -1.4 * s, 0.6 * s, 0.25 * s);
    g.restore();
    line(g, col, 0.5 * s, 0.55 * s, -0.2 * s, 0.55 * s, 0.9 * s);
    rr(g, col, -0.55 * s, 0.25 * s, 1.1 * s, 0.2 * s);
    rr(g, col, -0.9 * s, 1.0 * s, 1.8 * s, 0.3 * s);
  },
  leaf(g, col, s) {
    g.beginPath();
    g.moveTo(-1.0 * s, 0.9 * s);
    g.bezierCurveTo(-1.3 * s, -0.5 * s, 0.2 * s, -1.3 * s, 1.1 * s, -0.9 * s);
    g.bezierCurveTo(1.3 * s, 0.2 * s, 0.3 * s, 1.3 * s, -1.0 * s, 0.9 * s);
    g.fillStyle = col;
    g.fill();
    line(g, '#e9fff8', 0.4, -0.9 * s, 0.75 * s, 0.8 * s, -0.75 * s);
  },
  brain(g, col, s) {
    ell(g, lite(col, 0.2), -0.5 * s, -0.1 * s, 0.75 * s, 0.9 * s);
    ell(g, lite(col, 0.2), 0.5 * s, -0.1 * s, 0.75 * s, 0.9 * s);
    for (const x of [-0.9, -0.15, 0.6]) line(g, rgba(dark(col, 0.35), 0.8), 0.35, x * s, -0.8 * s, x * s + 0.3 * s, 0.7 * s);
  },
};

// A plaque with a picture on it, centred on the face at height `zc`.
function plaque(S, lab, cx, zc, kind) {
  const g = S.g;
  const w = 7.2;
  const h = 3.8;
  const y = -zc;
  rr(g, 'rgba(25,15,35,.25)', cx - w / 2 + 0.4, y - h / 2 + 0.6, w + 0.6, h + 0.8);
  rr(g, dark(lab.emblemC, 0.1), cx - w / 2 - 0.5, y - h / 2 - 0.5, w + 1, h + 1);
  rr(g, '#f5fbf9', cx - w / 2, y - h / 2, w, h);
  g.save();
  g.translate(cx, y);
  EMBLEMS[kind](g, lab.emblemC, 1.25);
  g.restore();
}

// --- The building's solid parts ------------------------------------------------------------------

// A box in world space with its visible faces: the front-left (v1), the front-right (u1) and the top.
function labBox(g, R, z0, z1, top, l, r) {
  const [u0, v0, u1, v1] = R;
  fp3(g, r, [u1, v0, z0], [u1, v1, z0], [u1, v1, z1], [u1, v0, z1]);
  fp3(g, l, [u0, v1, z0], [u1, v1, z0], [u1, v1, z1], [u0, v1, z1]);
  fp3(g, top, [u0, v0, z1], [u1, v0, z1], [u1, v1, z1], [u0, v1, z1]);
}

// A round-topped cylinder in world space, from its ground point (u, v), of radius r.
function cyl(g, u, v, r, z0, z1, col) {
  const [x, y0] = P(u, v, z0);
  const [, y1] = P(u, v, z1);
  const rx = r * 1.414;
  const ry = r * 0.707;
  g.beginPath();
  g.moveTo(x + rx, y1);
  g.lineTo(x + rx, y0);
  g.ellipse(x, y0, rx, ry, 0, 0, Math.PI);
  g.lineTo(x - rx, y1);
  g.ellipse(x, y1, rx, ry, 0, Math.PI, 0);
  g.closePath();
  g.fillStyle = grad(g, x - rx, 0, x + rx, 0, lite(col, 0.3), col, dark(col, 0.32));
  g.fill();
  ell(g, lite(col, 0.22), x, y1, rx, ry);
}

// A flat cap on a box, in world space.
function topCap(S, R, z, col) {
  const [u0, v0, u1, v1] = R;
  fp3(S.g, col, [u0, v0, z], [u1, v0, z], [u1, v1, z], [u0, v1, z]);
}

// A glazed face on a box: glass in the face's plane, mullions and transoms, and foliage behind it if asked.
function glassFace(S, lab, len, z0, h, tint, frame, plants) {
  const g = S.g;
  const t = -(z0 + h);
  if (!S.day) {
    for (let a = 0; a < len - 0.5; a += 2.4) {
      for (let z = z0; z < z0 + h - 0.5; z += 4) {
        if (!litAt(S, a, z, lab.pct, 41)) continue;
        const cw = Math.min(2.4, len - a) - 0.5;
        const ch = Math.min(4, z0 + h - z) - 0.5;
        rr(g, grad(g, 0, -(z + ch), 0, -z, lite(lab.lightC, 0.45), lab.lightC), a + 0.25, -(z + ch), cw, ch);
      }
    }
    return;
  }
  rr(g, grad(g, 0, t, 0, t + h, lite(tint, 0.45), tint, dark(tint, 0.25)), 0, t, len, h);
  if (plants) {
    for (let a = 1.6; a < len; a += 2.6) {
      ell(g, '#3f8f45', a, -z0 - 1.4, 1.5, 1.2);
      ell(g, '#66b35a', a - 0.4, -z0 - 2.1, 0.9, 0.7);
    }
  }
  for (let a = 2.4; a < len; a += 2.4) rr(g, frame, a - 0.25, t, 0.5, h);
  for (let z = z0 + 4; z < z0 + h - 0.5; z += 4) rr(g, frame, 0, -z - 0.25, len, 0.5);
  fp(g, 'rgba(255,255,255,.22)', [[len * 0.1, t], [len * 0.28, t], [len * 0.08, t + h], [0, t + h]]);
}

// The wings and towers on the roof. Each takes its base height `zb` and the rect it stands in.
function glassWing(S, lab, R, zb) {
  const h = 7.5;
  const frame = lite(lab.wingC, 0.35);
  box(S, R, zb, zb + h, { k: 'lab', c: lab.wingC }, (side, len) => glassFace(S, lab, len, zb, h, lab.tint, frame, false));
  if (S.day) topCap(S, R, zb + h, dark(lab.roofC, 0.15));
}

function observatory(S, lab, R, zb) {
  const g = S.g;
  const cu = (R[0] + R[2]) / 2;
  const cv = (R[1] + R[3]) / 2;
  const r = Math.min(R[2] - R[0], R[3] - R[1]) * 0.45;
  if (!S.day) return;
  const dz = zb + 6.5;
  cyl(g, cu, cv, r, zb, dz, '#f4f7f8');
  const [x, y] = P(cu, cv, dz);
  const Rd = r * 1.2;
  // The dome: a sphere sitting on the drum, lit at the top left.
  const gr = g.createRadialGradient(x - Rd * 0.35, y - Rd * 0.45, Rd * 0.1, x, y, Rd);
  gr.addColorStop(0, '#ffffff');
  gr.addColorStop(0.6, '#e6edf0');
  gr.addColorStop(1, '#8fa3ad');
  g.beginPath();
  g.arc(x, y, Rd, Math.PI, 0);
  g.closePath();
  g.fillStyle = gr;
  g.fill();
  ell(g, '#c9d4d9', x, y, Rd * 1.05, Rd * 0.3);
  // The slit in the dome, and the telescope tube going out of it.
  fp(g, '#27394a', [[x - 0.5, y - Rd * 0.97], [x + 0.5, y - Rd * 0.97], [x + Rd * 0.5, y - 0.2], [x + Rd * 0.25, y + 0.1]]);
  line(g, '#f7fbfc', 1.6, x + Rd * 0.15, y - Rd * 0.5, x + Rd * 0.9, y - Rd * 1.3);
}

function greenhouse(S, lab, R, zb) {
  const g = S.g;
  const h = 4.5;
  const tint = mixHex(lab.tint, '#8fd49a', 0.35);
  const frame = lite(lab.wingC, 0.35);
  box(S, R, zb, zb + h, { k: 'lab', c: lab.wingC }, (side, len) => glassFace(S, lab, len, zb, h, tint, frame, true));
  if (!S.day) return;
  // A pitched glass roof with its ridge along u, the front slope and the gable on the right visible.
  const [a0, b0, a1, b1] = R;
  const bm = (b0 + b1) / 2;
  const zr = zb + h + 3.6;
  const ze = zb + h;
  fp3(g, lite(tint, 0.12), [a0, bm, zr], [a1, bm, zr], [a1, b1, ze], [a0, b1, ze]);
  for (let u = a0 + 2; u < a1; u += 2.2) line(g, 'rgba(240,255,250,.5)', 0.4, ...P(u, bm, zr), ...P(u, b1, ze));
  fp3(g, dark(tint, 0.05), [a1, b0, ze], [a1, bm, zr], [a1, b1, ze]);
  for (let v = b0 + 2; v < b1; v += 2.4) line(g, FRAME, 0.4, ...P(a1, v, ze), ...P(a1, bm, zr) );
}

function cleanTower(S, lab, R, zb) {
  const h = 16;
  const g = S.g;
  box(S, R, zb, zb + h, { k: 'lab', c: lab.wingC }, (side, len) => {
    if (!S.day) return;
    for (let z = zb + 9.5; z < zb + h - 1.5; z += 1.3) rr(g, rgba(dark(lab.wingC, 0.45), 0.55), 0.6, -z - 0.5, len - 1.2, 0.5);
    for (let a = 3.2; a < len; a += 3.2) rr(g, rgba(dark(lab.wingC, 0.3), 0.35), a, -(zb + h), 0.35, h);
  });
  if (!S.day) return;
  topCap(S, R, zb + h, '#dfe9e7');
  const cu = (R[0] + R[2]) / 2;
  const cv = (R[1] + R[3]) / 2;
  cyl(g, cu, cv, 0.9, zb + h, zb + h + 4, '#eef4f3');
}

// The little rooftop kit. Each takes its base `H` (already lifted over a vault) and the scale `s`.
function airHandler(S, lab, u, v, H, s) {
  if (!S.day) return;
  const g = S.g;
  const R = [u, v, u + 5.6 * s, v + 4.2 * s];
  const z1 = H + 4.2 * s;
  labBox(g, R, H, z1, lite(lab.accent, 0.2), lab.accent, dark(lab.accent, 0.18));
  onL(g, R[0], R[3], () => {
    for (const k of [0.3, 0.7]) {
      ell(g, '#3e4856', k * 5.6 * s, -(H + 2.1 * s), 1.3 * s, 1.3 * s);
      ell(g, '#a9b6bf', k * 5.6 * s, -(H + 2.1 * s), 0.9 * s, 0.9 * s);
    }
  });
  const [x, y] = P(u + 2.8 * s, v + 2.1 * s, z1);
  ell(g, '#3e4856', x, y, 1.6 * s * 1.41, 1.6 * s * 0.7);
}

function fumeStacks(S, lab, u, v, H, s) {
  const g = S.g;
  for (let i = 0; i < 4; i++) {
    const a = u + i * 1.9 * s;
    const h = (11 + (i % 2) * 3) * s;
    if (S.day) {
      cyl(g, a, v, 0.55 * s, H, H + h, lab.accent);
    } else {
      const [x, y] = P(a, v, H + h);
      glow(g, x, y, 5, '#ff4a3a', 0.9);
      ell(g, '#ff6a5a', x, y, 0.8, 0.8);
    }
  }
}

function weatherMast(S, lab, u, v, H, s) {
  const g = S.g;
  const top = H + 14 * s;
  const [x, y] = P(u, v, top);
  if (!S.day) {
    glow(g, x, y, 5, '#ff4a3a', 0.9);
    ell(g, '#ff6a5a', x, y, 0.8, 0.8);
    return;
  }
  const [bx, by] = P(u, v, H);
  line(g, dark(lab.accent, 0.1), 0.9 * s, bx, by, x, y);
  const ya = P(u, v, top - 2 * s);
  const l = P(u - 1.8 * s, v, top - 2 * s);
  const r = P(u + 1.8 * s, v, top - 2 * s);
  line(g, dark(lab.accent, 0.1), 0.5 * s, l[0], l[1], r[0], r[1]);
  for (const p of [l, r]) ell(g, lite(lab.accent, 0.3), p[0], p[1] - 0.6 * s, 1.0 * s, 0.7 * s);
  ball(g, ya[0], ya[1] - 0.4 * s, 0.9 * s, lite(lab.accent, 0.2));
  ball(g, x, y, 1.2 * s, lite(lab.accent, 0.2));
}

function solarArray(S, lab, u, v, H, s) {
  if (!S.day) return;
  const g = S.g;
  const zBack = H + 4.2 * s;
  const zFront = H + 1.6 * s;
  for (let i = 0; i < 2; i++) {
    const a = u + i * 3.6 * s;
    const w = 3.2 * s;
    const b0 = v;
    const b1 = v + 3.4 * s;
    line(g, dark(lab.accent, 0.1), 0.6 * s, ...P(a + 0.4 * s, b0, H), ...P(a + 0.4 * s, b0, zBack));
    line(g, dark(lab.accent, 0.1), 0.6 * s, ...P(a + w - 0.4 * s, b1, H), ...P(a + w - 0.4 * s, b1, zFront));
    fp3(g, '#2c4f79', [a, b0, zBack], [a + w, b0, zBack], [a + w, b1, zFront], [a, b1, zFront]);
    for (let k = 1; k < 3; k++) {
      line(g, 'rgba(200,220,240,.45)', 0.35, ...P(a + (w * k) / 3, b0, zBack), ...P(a + (w * k) / 3, b1, zFront));
      const bb = b0 + ((b1 - b0) * k) / 3;
      const zz = zBack + ((zFront - zBack) * k) / 3;
      line(g, 'rgba(200,220,240,.45)', 0.35, ...P(a, bb, zz), ...P(a + w, bb, zz));
    }
    line(g, lite(lab.accent, 0.3), 0.6 * s, ...P(a, b0, zBack), ...P(a + w, b0, zBack));
  }
}

function uplinkDish(S, lab, u, v, H, s) {
  if (!S.day) return;
  const g = S.g;
  const top = H + 8 * s;
  cyl(g, u, v, 0.6 * s, H, top, lab.accent);
  const [x, y] = P(u, v, top + 0.8 * s);
  ell(g, lite(lab.accent, 0.45), x, y, 3.6 * s, 2.0 * s);
  const gr = g.createRadialGradient(x, y + 0.5 * s, 0.2 * s, x, y, 3.1 * s);
  gr.addColorStop(0, '#f1f7f9');
  gr.addColorStop(1, dark(lab.accent, 0.2));
  ell(g, gr, x, y, 3.1 * s, 1.6 * s);
  line(g, dark(lab.accent, 0.3), 0.5 * s, x, y, x, y - 3 * s);
  ball(g, x, y - 3.2 * s, 0.7 * s, '#ffffff');
}

function windTurbine(S, lab, u, v, H, s) {
  if (!S.day) return;
  const g = S.g;
  const top = H + 12 * s;
  const [bx, by] = P(u, v, H);
  const [hx, hy] = P(u, v, top);
  line(g, '#eef4f5', 0.9 * s, bx, by, hx, hy);
  for (let k = 0; k < 3; k++) {
    const ang = -1.5708 + k * 2.0944 + 0.35;
    const L = 6 * s;
    const tx = hx + Math.cos(ang) * L;
    const ty = hy + Math.sin(ang) * L;
    const nx = -Math.sin(ang) * 0.6 * s;
    const ny = Math.cos(ang) * 0.6 * s;
    fp(g, '#f4f9fa', [[hx + nx * 0.2, hy + ny * 0.2], [tx, ty], [hx - nx * 0.3, hy - ny * 0.3]]);
  }
  ball(g, hx, hy, 1.1 * s, lite(lab.accent, 0.2));
}

const ROOFTOP = {
  fume_stacks: fumeStacks,
  air_handler: airHandler,
  weather_mast: weatherMast,
  solar_array: solarArray,
  uplink_dish: uplinkDish,
  wind_turbine: windTurbine,
};

// The roof's own surface: a vault of glass, a planted lawn, or a deck with sawtooth glass on it.
function vault(S, lab, rect, H, vh) {
  const g = S.g;
  const [u0, v0, u1, v1] = rect;
  const W = u1 - u0;
  const N = 14;
  const z = (u) => H + vh * Math.sqrt(Math.max(0, 1 - ((u - (u0 + u1) / 2) / (W / 2)) ** 2));
  for (let i = 0; i < N; i++) {
    const a = u0 + (W * i) / N;
    const b = u0 + (W * (i + 1)) / N;
    const f = (i + 0.5) / N;
    const col = mixHex(lab.tint, '#4d8f98', 0.2 + 0.4 * Math.abs(2 * f - 1));
    fp3(g, col, [a, v0, z(a)], [a, v1, z(a)], [b, v1, z(b)], [b, v0, z(b)]);
    line(g, 'rgba(230,250,248,.35)', 0.5, ...P(a, v0, z(a)), ...P(a, v1, z(a)));
  }
  // The front end is a wall with the arch of the vault over it.
  const pts = [[u0, v1, H]];
  for (let i = 0; i <= N; i++) {
    const a = u0 + (W * i) / N;
    pts.push([a, v1, z(a)]);
  }
  pts.push([u1, v1, H]);
  fp3(g, lite(lab.wall, 0.05), ...pts);
  for (let i = 0; i < N; i++) {
    const a = u0 + (W * i) / N;
    const b = u0 + (W * (i + 1)) / N;
    line(g, FRAME, 1.1, ...P(a, v1, z(a)), ...P(b, v1, z(b)));
  }
}

function greenPatches(S, rect, H) {
  const g = S.g;
  const [u0, v0, u1, v1] = rect;
  for (let i = 0; i < 26; i++) {
    const u = u0 + 2 + ((ih(S.seed, i, 21) % 1000) / 1000) * (u1 - u0 - 4);
    const v = v0 + 2 + ((ih(S.seed, i, 22) % 1000) / 1000) * (v1 - v0 - 4);
    const [x, y] = P(u, v, H);
    ell(g, ['#7fb35a', '#5f9a45', '#93c466'][i % 3], x, y, 1.6 + (i % 4) * 0.4, 0.9 + (i % 3) * 0.3);
  }
}

function sawtooth(S, lab, rect, H, s) {
  const g = S.g;
  const [u0, v0, u1, v1] = rect;
  const span = u1 - u0 - 4;
  const n = Math.max(2, Math.round(span / 3.4));
  const pitch = span / n;
  const rise = 4.2 * s;
  const glassC = mixHex(lab.tint, '#88d9d0', 0.3);
  for (let i = 0; i < n; i++) {
    const a = u0 + 2 + i * pitch;
    const b = a + pitch;
    // Alternate teeth catch the light differently, so each sloped pane reads on its own.
    const col = i % 2 ? glassC : dark(glassC, 0.22);
    fp3(g, col, [a, v0 + 1, H + rise], [a, v1 - 1, H + rise], [b, v1 - 1, H], [b, v0 + 1, H]);
    line(g, 'rgba(40,70,75,.5)', 0.6, ...P(a, v0 + 1, H + rise), ...P(a, v1 - 1, H + rise));
    for (let v = v0 + 4; v < v1 - 1; v += 3) line(g, 'rgba(230,255,252,.4)', 0.35, ...P(a, v, H + rise), ...P(b, v, H));
  }
}

// The parapet along the two front edges, which is drawn last so it sits in front of what is on the roof.
function parapet(S, lab, rect, H) {
  const g = S.g;
  const [u0, v0, u1, v1] = rect;
  const W = u1 - u0;
  const D = v1 - v0;
  const t = 2.5;
  const coping = lite(lab.wall, 0.05);
  onL(g, u0, v1, () => {
    rr(g, grad(g, 0, -H - t, 0, -H, lite(coping, 0.15), coping), -0.3, -H - t, W + 0.6, t);
    rr(g, 'rgba(25,15,35,.24)', -0.3, -H - 0.8, W + 0.6, 0.8);
  });
  onR(g, u1, v1, () => {
    rr(g, grad(g, 0, -H - t, 0, -H, dark(coping, 0.12), dark(coping, 0.25)), -0.3, -H - t, D + 0.6, t);
    rr(g, 'rgba(36,26,70,.34)', -0.3, -H - t, D + 0.6, t);
  });
  void v0;
}

// A ramp of concrete out of the door, with a rail either side of it.
function ramp(S, lab, pm, drop) {
  if (!S.day) return;
  const g = S.g;
  const zAt = (o) => drop * (1 - o / 5.5);
  fp3(g, '#c6d1ce', [...pm(-2.6, 0), drop], [...pm(2.6, 0), drop], [...pm(2.6, 5.5), 0], [...pm(-2.6, 5.5), 0]);
  for (const a of [-2.6, 2.6]) {
    line(g, lab.entryC, 0.7, ...P(...pm(a, 0.3), zAt(0.3) + 3), ...P(...pm(a, 5.3), zAt(5.3) + 3));
    for (const o of [0.3, 2.8, 5.3]) line(g, lab.entryC, 0.6, ...P(...pm(a, o), zAt(o)), ...P(...pm(a, o), zAt(o) + 3));
  }
}

// --- The building -----------------------------------------------------------------------------------

export function paintLab(S, outside, rect, c) {
  void c;
  const g = S.g;
  const [u0, v0, u1, v1] = rect;
  const W = u1 - u0;
  const D = v1 - v0;
  const floors = Math.min(3, Math.max(1, bodyOf('research_lab', outside)?.floors || 1));
  const H = 16 + 11 * (floors - 1);
  const glowMode = styleOf(outside, 'lab_glow', GLOW_WORDS, 'cool');
  const lab = {
    facade: styleOf(outside, 'lab_facade', FACADE_WORDS, 'panels'),
    win: styleOf(outside, 'lab_windows', WINDOW_WORDS, 'ribbon'),
    roof: styleOf(outside, 'lab_roof', ROOF_WORDS, 'deck'),
    wing: styleOf(outside, 'lab_wing', WING_WORDS, 'glass_wing'),
    entry: styleOf(outside, 'lab_entry', ENTRY_WORDS, 'sliding'),
    emblem: styleOf(outside, 'lab_emblem', EMBLEM_WORDS, 'flask'),
    wall: colorOf(outside, 'lab_facade', '#e1eee8'),
    tint: colorOf(outside, 'lab_windows', '#a1f2e4'),
    roofC: colorOf(outside, 'lab_roof', '#357b82'),
    wingC: colorOf(outside, 'lab_wing', '#b4dedc'),
    accent: colorOf(outside, 'lab_rooftop', '#748f9b'),
    entryC: colorOf(outside, 'lab_entry', '#287a84'),
    emblemC: colorOf(outside, 'lab_emblem', '#3d9d9a'),
    lightC: colorOf(outside, 'lab_glow', GLOW[glowMode][0]),
    pct: GLOW[glowMode][1],
  };
  const winN = numberOf(outside, 'lab_windows', undefined);
  const n = winN === undefined ? undefined : Math.min(6, Math.max(0, winN));
  const rooftopOn = Array.isArray(outside?.parts?.lab_rooftop?.on) ? outside.parts.lab_rooftop.on : LAB_ROOFTOP_DEFAULT;
  const doorSide = S.door === 'R' ? 'R' : 'L';
  const uc = (u0 + u1) / 2;
  const vc = (v0 + v1) / 2;
  const own = lab.facade === 'curtain_glass' || lab.facade === 'brick_glass';
  const boundaries = [];
  for (let k = 1; k < floors; k++) boundaries.push(storeyBase(k));
  const face = { wall: lab.wall, tint: lab.tint, frame: FRAME, seed: S.seed, floors: boundaries };

  // The door's ground point, and the way out from it: along the door face (a) and away from it (o).
  const pm = (a, o) => (doorSide === 'L' ? [uc + a, v1 + o] : [u1 + o, vc + a]);
  const proud = (half, out) => (doorSide === 'L' ? [uc - half, v1, uc + half, v1 + out] : [u1, vc - half, u1 + out, vc + half]);

  shadow(S, rect, H);
  const detail = (side, len) => {
    const door = side === doorSide;
    const mid = len / 2;
    const dz = door ? [mid - 5, mid + 5] : null;
    if (S.day) {
      g.save();
      clipFace(g, len, H);
      FACADES[lab.facade](g, len, H, { ...face, dz });
      g.restore();
    }
    if (own) {
      nightCells(S, lab, len, H, floors, dz);
    } else {
      for (let k = 0; k < floors; k++) {
        const skip = door && k === 0 ? dz : null;
        for (const [x, w] of bayRuns(lab.win, len, n, skip)) windowBay(S, lab, x, w, k);
      }
    }
    if (door) {
      const zb = lab.entry === 'ramp' ? 1.6 : 0;
      if (lab.entry !== 'airlock') entryDoor(S, lab, mid - DOOR_W / 2, zb, DOOR_W, DOOR_H, lab.entry);
      if (S.day && lab.emblem !== 'none') plaque(S, lab, mid, 13.9, lab.emblem);
    }
  };
  box(S, rect, 0, H, { k: 'lab', c: lab.wall }, detail);

  // Things that stand out from the door face.
  if (lab.entry === 'airlock') {
    const R = proud(3.2, 3.4);
    if (S.day) {
      labBox(g, R, 0, 9.6, '#dfe9e6', mixHex(lab.entryC, '#bfe8e4', 0.45), dark(mixHex(lab.entryC, '#bfe8e4', 0.45), 0.15));
    }
    const outer = () => entryDoor(S, lab, 0.8, 0, DOOR_W, DOOR_H, 'sliding');
    if (doorSide === 'L') onL(g, R[0], R[3], outer);
    else onR(g, R[2], R[3], outer);
  }
  if (lab.entry === 'canopy' && S.day) {
    labBox(g, proud(3.8, 3.2), DOOR_H + 1.6, DOOR_H + 2.9, '#e8f0ee', lab.entryC, dark(lab.entryC, 0.2));
  }
  if (lab.entry === 'ramp') ramp(S, lab, pm, 1.6);
  if (!S.day) {
    const [x, y] = P(...pm(0, 2), 0);
    pool(g, x, y, 9, lab.lightC, 0.3);
  }

  // The roof: a floor or a vault, then the wing and the rooftop kit back to front, then the parapet.
  const vaultH = Math.min(6.5, W * 0.3);
  const vaultOn = lab.roof === 'vault';
  const zAt = (u) => H + (vaultOn ? vaultH * Math.sqrt(Math.max(0, 1 - ((u - uc) / (W / 2)) ** 2)) : 0);
  const IU0 = u0 + 2.5;
  const IV0 = v0 + 2.5;
  const IW = Math.max(1, W - 5);
  const ID = Math.max(1, D - 5);
  const at = (fu, fv) => [IU0 + fu * IW, IV0 + fv * ID];
  const s = Math.max(0.45, Math.min(1.5, Math.min(IW, ID) / 12));
  if (S.day) {
    if (vaultOn) {
      vault(S, lab, rect, H, vaultH);
    } else {
      const floor = lab.roof === 'green' ? mixHex(lab.roofC, '#6aa552', 0.6) : lab.roofC;
      fp3(g, grad(g, ...P(u0, v0, H), ...P(u1, v1, H), lite(floor, 0.12), dark(floor, 0.08)), [u0, v0, H], [u1, v0, H], [u1, v1, H], [u0, v1, H]);
      if (lab.roof === 'green') greenPatches(S, rect, H);
      if (lab.roof === 'sawtooth') sawtooth(S, lab, rect, H, s);
      if (lab.roof === 'deck') {
        for (let i = 1; i < 4; i++) line(g, 'rgba(20,40,40,.14)', 0.6, ...P(u0 + (W * i) / 4, v0, H), ...P(u0 + (W * i) / 4, v1, H));
      }
    }
  }

  const items = [];
  const add = (u, v, fn) => items.push({ d: u + v, fn });
  if (lab.wing !== 'none') {
    const WR = [at(0.5, 0.04)[0], at(0.5, 0.04)[1], at(0.92, 0.58)[0], at(0.92, 0.58)[1]];
    const zb = zAt((WR[0] + WR[2]) / 2);
    const fn = {
      glass_wing: () => glassWing(S, lab, WR, zb),
      observatory: () => observatory(S, lab, WR, zb),
      greenhouse: () => greenhouse(S, lab, WR, zb),
      clean_tower: () => cleanTower(S, lab, WR, zb),
    }[lab.wing];
    if (fn) add((WR[0] + WR[2]) / 2, (WR[1] + WR[3]) / 2, fn);
  }
  const spots = {
    air_handler: at(0.03, 0.03),
    weather_mast: at(0.36, 0.14),
    uplink_dish: at(0.38, 0.48),
    solar_array: at(0.03, 0.64),
    wind_turbine: at(0.62, 0.66),
  };
  for (const kind of ROOFTOP_WORDS) {
    if (!rooftopOn.includes(kind)) continue;
    if (kind === 'fume_stacks') {
      const [au, av] = at(0.6, 0.93);
      add(au, av, () => fumeStacks(S, lab, au, av, zAt(au), s));
      continue;
    }
    const [au, av] = spots[kind];
    const fn = ROOFTOP[kind];
    add(au, av, () => fn(S, lab, au, av, zAt(au), s));
  }
  items.sort((p, q) => p.d - q.d);
  for (const it of items) it.fn();
  if (S.day && !vaultOn) parapet(S, lab, rect, H);
}

// --- Yard items ------------------------------------------------------------------------------------

export const LAB_YARD_ART = {
  // A caged rack of three upright gas cylinders.
  gas_cylinders(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const col = hexOf(item.color, '#3f7fbf');
    const cage = '#5b6470';
    shadow(S, K.rect(0.2), 6);
    K.contact(0.6);
    const post = (a, b) => () => K.rod(a, b, 0, 6, 0.3, cage);
    K.parts([
      [0.2, 0.3, post(0.2, 0.3)],
      [1.8, 0.3, post(1.8, 0.3)],
      [0.2, 1.7, post(0.2, 1.7)],
      [1.8, 1.7, post(1.8, 1.7)],
      [0.55, 1.0, () => K.cyl(0.55, 1.0, 0.3, 0.4, 5.2, col)],
      [1.0, 1.0, () => K.cyl(1.0, 1.0, 0.3, 0.4, 5.2, lite(col, 0.08))],
      [1.45, 1.0, () => K.cyl(1.45, 1.0, 0.3, 0.4, 5.2, dark(col, 0.08))],
      [1.0, 1.0, () => {
        K.seg(cage, 0.4, [0.2, 0.3, 5.8], [1.8, 0.3, 5.8]);
        K.seg(cage, 0.4, [0.2, 1.7, 5.8], [1.8, 1.7, 5.8]);
        K.seg(cage, 0.4, [0.2, 0.3, 0.8], [1.8, 0.3, 0.8]);
        K.seg(cage, 0.4, [0.2, 1.7, 0.8], [1.8, 1.7, 0.8]);
      }, 2],
    ]);
  },

  // A chest freezer with a frost-blue panel and a little green status light, which is the only glow.
  sample_cooler(S, item) {
    const K = kit(S, item);
    const { g, at } = K;
    const body = hexOf(item.color, '#e8eef0');
    if (!S.day) {
      const [x, y] = at(1.4, 1.84, 3.4);
      glow(g, x, y, 5, '#7dff9a', 0.7);
      ball(g, x, y, 0.5, '#9dffb8');
      return;
    }
    shadow(S, K.rect(0.2), 5);
    K.contact(0.55);
    K.box(0.15, 0.15, 1.85, 1.85, 0, 4.4, body);
    K.q('#9fd6f0', [0, 1, 0], [0.4, 1.86, 1.0], [1.6, 1.86, 1.0], [1.6, 1.86, 3.0], [0.4, 1.86, 3.0]);
    K.seg('rgba(60,70,80,.4)', 0.5, [0.15, 1.85, 4.4], [1.85, 1.85, 4.4]);
    K.box(0.8, 1.88, 1.2, 1.95, 3.1, 3.5, '#8a9aa3');
    const [x, y] = at(1.7, 1.84, 3.7);
    ball(g, x, y, 0.45, '#5ee08a');
  },

  // A white refractor on a three-legged mount, tilted up at the sky.
  telescope(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const white = hexOf(item.color, '#f2f5f7');
    const legs = '#4a4f5a';
    shadow(S, K.rect(0.2), 9);
    K.contact(0.5);
    K.parts([
      [0.5, 0.5, () => K.seg(legs, 0.5, [1.0, 1.0, 5.2], [0.2, 0.2, 0])],
      [1.8, 0.4, () => K.seg(legs, 0.5, [1.0, 1.0, 5.2], [1.8, 0.2, 0])],
      [1.0, 1.9, () => K.seg(legs, 0.5, [1.0, 1.0, 5.2], [1.0, 1.9, 0])],
      [1.0, 1.0, () => {
        K.seg(white, 1.8, [1.0, 1.0, 5.6], [1.5, 1.25, 9.2]);
        K.seg('#2b3340', 1.4, [1.45, 1.22, 9.0], [1.55, 1.28, 9.3]);
        K.seg('rgba(255,255,255,.6)', 0.5, [0.9, 0.95, 5.8], [1.3, 1.15, 8.9]);
      }, 1],
    ]);
  },

  // Three by two raised research plots: wooden frames, soil, a different crop in each, and stakes.
  test_plots(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const wood = '#8a5c37';
    const soil = '#6b4a2f';
    const crops = ['#6fbf4a', '#e0553a', '#e8c04a', '#7ab648', '#b07ad8', '#8fd0a0'];
    shadow(S, K.rect(0.2), 2);
    K.contact(0.6);
    const parts = [];
    for (let r = 0; r < 2; r++) {
      for (let c = 0; c < 3; c++) {
        const a0 = 0.25 + c * 1.95;
        const b0 = 0.35 + r * 2.0;
        const crop = crops[r * 3 + c];
        parts.push([a0 + 0.8, b0 + 0.7, () => {
          K.box(a0, b0, a0 + 1.6, b0 + 1.4, 0, 1.2, wood);
          K.flat(soil, [a0 + 0.15, b0 + 0.15, 1.25], [a0 + 1.45, b0 + 0.15, 1.25], [a0 + 1.45, b0 + 1.25, 1.25], [a0 + 0.15, b0 + 1.25, 1.25]);
          K.ball(a0 + 0.5, b0 + 0.45, 1.8, 0.55, crop);
          K.ball(a0 + 1.1, b0 + 0.9, 1.7, 0.5, lite(crop, 0.2));
          K.rod(a0 + 1.5, b0 + 0.1, 1.2, 3.4, 0.35, '#e8e8e8');
          K.flat(lite(crop, 0.1), [a0 + 1.5, b0 + 0.1, 3.4], [a0 + 1.5, b0 + 0.1, 3.4], [a0 + 1.2, b0 + 0.1, 3.0], [a0 + 1.2, b0 + 0.1, 3.0]);
        }]);
      }
    }
    K.parts(parts);
  },

  // A pole-mounted solar panel, tilted to the sky.
  solar_tracker(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const frame = '#c9d3d8';
    const panel = '#2b4f7e';
    const sheen = 'rgba(200,225,245,.45)';
    const z = (b) => 8.4 + ((b - 1) / 2.2) * 3.4;
    shadow(S, K.rect(0.2), 10);
    K.contact(0.6);
    K.parts([
      [2, 2, () => K.cyl(2, 2, 0.28, 0, 8.4, '#9aa6ad')],
      [2, 2.05, () => K.seg(frame, 0.9, [2, 2, 8.2], [2, 2.1, 10.1])],
      [2, 2.6, () => {
        K.flat(panel, [0.3, 1.0, 8.4], [3.7, 1.0, 8.4], [3.7, 3.2, 11.8], [0.3, 3.2, 11.8]);
        for (let i = 1; i < 4; i++) K.seg(sheen, 0.35, [0.3 + i * 0.85, 1.0, 8.4], [0.3 + i * 0.85, 3.2, 11.8]);
        for (let j = 1; j < 3; j++) {
          const b = 1 + j * 0.733;
          K.seg(sheen, 0.35, [0.3, b, z(b)], [3.7, b, z(b)]);
        }
        K.seg(frame, 0.6, [0.3, 3.2, 11.8], [3.7, 3.2, 11.8]);
        K.seg(frame, 0.6, [0.3, 1.0, 8.4], [0.3, 3.2, 11.8]);
        K.seg(frame, 0.6, [3.7, 1.0, 8.4], [3.7, 3.2, 11.8]);
      }, 1],
    ]);
  },
};
