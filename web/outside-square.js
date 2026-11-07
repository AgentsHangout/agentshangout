// Yard items only a town square can hold (docs/outside.md): a market stall, a notice board, a flower
// cart, a drinking fountain and a street clock. Each is drawn in its own frame, facing 0, and kit()
// turns it to the way the item faces. A square is drawn at the same unit scale as the rest of the
// yard, so these carry more detail rather than more size.

import { hexOf } from './outside-spec.js';
import { glow, kit, pool } from './outside-decor.js';
import { dark, ell, ih, lite, mixHex, rgba, shadow } from './world.js';

const TAU = 6.2832;
const CLOTH = '#f4ecd8';
const IRON = '#3a3d4a';
const BRASS = '#c9a24a';
const LEAF = '#4f8a3a';

export const SQUARE_YARD_ART = {
  // A striped canopy on four posts, with produce on the counter and a crate of fruit and vegetables.
  market_stall(S, item) {
    const K = kit(S, item);
    const { g, at } = K;
    const stripe = hexOf(item.color, '#c0392b');
    const lamp = '#ffc766';
    if (!S.day) {
      // Only the hanging lantern shows at night.
      const [lx, ly] = at(2, 3.7, 4.2);
      pool(g, ...at(2, 2, 0), 14, lamp, 0.3);
      glow(g, lx, ly, 10, lamp, 0.7);
      ell(g, lite(lamp, 0.6), lx, ly, 0.8, 1.1);
      return;
    }
    const wood = '#8a5a34';
    const crate = '#c08a52';
    // The canopy slopes down from the back posts to the front edge.
    const top = (b) => 7.4 - 0.3 * (b - 0.1);
    const strip = (i) => (i % 2 ? CLOTH : stripe);
    const post = (a, b) => () => K.rod(a, b, 0, top(b) - 0.3, 0.4, dark(wood, 0.2));
    shadow(S, K.rect(0.2), 9);
    K.contact(1);
    K.parts([
      // Back of the stall: the posts, a plank backboard and a shelf of goods.
      [0.35, 0.35, post(0.35, 0.35), 0],
      [3.65, 0.35, post(3.65, 0.35), 0],
      [2, 0.2, () => {
        K.box(0.35, 0.12, 3.65, 0.3, 0.4, 5.2, dark(wood, 0.1));
        for (const z of [1.6, 2.8, 4.0]) K.seg(dark(wood, 0.35), 0.15, [0.35, 0.31, z], [3.65, 0.31, z]);
        K.box(0.35, 0.3, 3.65, 0.62, 3.0, 3.22, wood);
      }, 0],
      [1, 0.45, () => {
        K.ball(1.0, 0.45, 3.65, 0.42, '#e8902a');
        K.ball(2.2, 0.45, 3.68, 0.45, '#6aa84f');
        K.ball(3.1, 0.45, 3.6, 0.38, '#d9731f');
      }, 1],
      // The counter, with a row of planks down its front and a lighter top.
      [2, 2.95, () => {
        K.box(0.5, 2.5, 3.5, 3.35, 0, 2.5, wood);
        for (const z of [0.8, 1.6]) K.seg(dark(wood, 0.3), 0.16, [0.5, 3.36, z], [3.5, 3.36, z]);
        K.box(0.4, 2.45, 3.6, 3.55, 2.5, 2.85, lite(wood, 0.14));
      }, 1],
      // Crate of fruit.
      [1.35, 2.95, () => {
        K.box(0.8, 2.75, 1.9, 3.25, 2.85, 3.5, crate);
        for (const z of [3.1, 3.3]) K.seg(dark(crate, 0.3), 0.14, [0.8, 3.26, z], [1.9, 3.26, z]);
        K.ball(1.0, 2.9, 3.85, 0.4, '#d8462e');
        K.ball(1.9, 2.92, 3.85, 0.4, '#d8462e');
        K.ball(1.45, 2.9, 3.9, 0.4, '#f08a24');
        K.ball(1.2, 3.1, 3.8, 0.4, '#f2b13a');
        K.ball(1.7, 3.1, 3.8, 0.4, '#f08a24');
      }, 2],
      // Crate of vegetables: cabbages, and carrots with their green tops.
      [2.65, 2.95, () => {
        K.box(2.1, 2.75, 3.2, 3.25, 2.85, 3.5, crate);
        for (const z of [3.1, 3.3]) K.seg(dark(crate, 0.3), 0.14, [2.1, 3.26, z], [3.2, 3.26, z]);
        K.ball(2.4, 2.9, 3.85, 0.42, '#7aa84f');
        K.ball(3.0, 2.9, 3.85, 0.42, '#8fbf5c');
        K.ball(2.7, 3.1, 3.8, 0.38, '#6e9c46');
        K.seg('#e8742a', 0.45, [2.45, 3.02, 3.6], [2.6, 3.02, 4.2]);
        K.seg('#e8742a', 0.4, [2.95, 3.05, 3.6], [3.05, 3.05, 4.05]);
        K.ball(2.65, 3.02, 4.35, 0.2, LEAF);
        K.ball(3.08, 3.05, 4.2, 0.18, LEAF);
      }, 2],
      // Front posts and the lantern, which hangs under the front valance.
      [0.35, 3.65, post(0.35, 3.65), 3],
      [3.65, 3.65, post(3.65, 3.65), 3],
      [2, 3.7, () => {
        const lamp2 = mixHex(lamp, '#8a8272', 0.5);
        K.seg(IRON, 0.22, [2, 3.7, 5.8], [2, 3.7, 4.7]);
        K.box(1.8, 3.55, 2.2, 3.85, 3.7, 4.5, lamp2);
        K.box(1.75, 3.5, 2.25, 3.9, 4.5, 4.7, IRON);
        K.box(1.8, 3.55, 2.2, 3.85, 3.5, 3.7, IRON);
      }, 3],
      // Striped canopy on top, with its scalloped valances on the front and the right side.
      [2, 2, () => {
        for (let i = 0; i < 8; i++) {
          const a0 = i * 0.5;
          K.q(strip(i), [0, 0.3, 1], [a0, 0, top(0)], [a0 + 0.5, 0, top(0)], [a0 + 0.5, 4, top(4)], [a0, 4, top(4)]);
        }
        for (let i = 0; i < 8; i++) {
          const a0 = i * 0.5;
          K.q(strip(i), [0, 1, 0], [a0, 4, top(4)], [a0 + 0.5, 4, top(4)], [a0 + 0.5, 4, 5.0], [a0 + 0.25, 4, 4.6], [a0, 4, 5.0]);
        }
        for (let i = 0; i < 8; i++) {
          const b0 = i * 0.5;
          const b1 = b0 + 0.5;
          const bm = b0 + 0.25;
          K.q(strip(i), [1, 0, 0], [4, b0, top(b0)], [4, b1, top(b1)], [4, b1, top(b1) - 1.1], [4, bm, top(bm) - 1.5], [4, b0, top(b0) - 1.1]);
        }
      }, 4],
    ]);
  },

  // A little roofed board on two posts, with coloured cards and a photo pinned to the cork. No words.
  notice_board(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const wood = '#9a6a3c';
    const cork = '#c9a676';
    const roof = '#8a4a32';
    const card = (a0, z0, a1, z1, col) => K.q(col, [0, 1, 0], [a0, 0.86, z0], [a1, 0.86, z0], [a1, 0.86, z1], [a0, 0.86, z1]);
    const pin = (a, z, col) => K.circ(col, a, 0.88, z, 0.09, 'a', 1);
    shadow(S, K.rect(0.2), 6);
    K.contact(0.5);
    K.parts([
      [0.3, 0.35, () => K.rod(0.3, 0.35, 0, 5.8, 0.4, dark(wood, 0.2)), 0],
      [1.7, 0.35, () => K.rod(1.7, 0.35, 0, 5.8, 0.4, dark(wood, 0.2)), 0],
      [1, 0.8, () => {
        K.box(0.25, 0.7, 1.75, 0.84, 1.4, 4.6, wood);
        K.q(cork, [0, 1, 0], [0.33, 0.84, 1.6], [1.67, 0.84, 1.6], [1.67, 0.84, 4.4], [0.33, 0.84, 4.4]);
        card(0.45, 3.1, 0.95, 3.95, '#e05a4a');
        pin(0.7, 3.9, '#2e6fd0');
        card(1.05, 3.4, 1.5, 4.2, '#f2c94c');
        pin(1.27, 4.1, '#e05a4a');
        card(1.05, 2.9, 1.5, 3.25, '#f7f4ea');
        pin(1.27, 3.2, '#2e6fd0');
        // A photo: a white print with a sky, a green hill and a sun.
        card(0.4, 1.8, 1.05, 2.7, '#fbfaf5');
        card(0.46, 2.2, 0.99, 2.66, '#8fc7e8');
        K.q('#5f9a5a', [0, 1, 0], [0.46, 0.86, 2.2], [0.99, 0.86, 2.2], [0.99, 0.86, 2.42], [0.75, 0.86, 2.6], [0.46, 0.86, 2.36]);
        K.circ('#ffe27a', 0.82, 0.86, 2.5, 0.09, 'a', 1);
        pin(0.72, 2.6, '#e8e8e8');
        card(1.15, 1.85, 1.6, 2.6, '#8ccf9a');
        pin(1.37, 2.5, '#f2c94c');
      }, 1],
      // Roof: two slopes meeting at a ridge, with shingle rows and gable ends. K.q paints only the
      // faces that turn to the viewer, so every facing shows the right ones.
      [1, 0.7, () => {
        K.q(roof, [0, 1.2, 0.6], [-0.05, 1.25, 5.6], [2.05, 1.25, 5.6], [2.05, 0.68, 6.8], [-0.05, 0.68, 6.8]);
        K.q(dark(roof, 0.1), [0, -1.2, 0.6], [-0.05, 0.1, 5.6], [2.05, 0.1, 5.6], [2.05, 0.68, 6.8], [-0.05, 0.68, 6.8]);
        for (const t of [0.3, 0.6]) {
          const b = 1.25 - 0.57 * t;
          const z = 5.6 + 1.2 * t;
          K.seg(dark(roof, 0.3), 0.18, [-0.05, b, z], [2.05, b, z]);
        }
        K.seg(lite(roof, 0.2), 0.5, [-0.05, 0.68, 6.8], [2.05, 0.68, 6.8]);
        for (const a of [-0.05, 2.05]) {
          K.q(lite(wood, 0.1), [1, 0, 0], [a, 0.1, 5.6], [a, 1.25, 5.6], [a, 0.68, 6.8]);
          K.q(dark(wood, 0.1), [-1, 0, 0], [a, 0.1, 5.6], [a, 1.25, 5.6], [a, 0.68, 6.8]);
        }
      }, 2],
    ]);
  },

  // A wooden hand cart with two spoked wheels, and buckets of cut flowers in many colours.
  flower_cart(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const body = hexOf(item.color, '#3b6ea8');
    const wood = '#9a6a3c';
    const flowers = ['#e8506a', '#f2c94c', '#9a6bd0', '#fbfaf5', '#f0883a', '#e25aa8', '#5cb0e0'];
    shadow(S, K.rect(0.2), 5);
    K.contact(0.7);
    // A spoked wheel with its axle along b, so it is seen from the near side.
    const wheel = (b) => () => {
      const a = 2.1;
      const z = 1.5;
      const r = 1.4;
      K.circ(wood, a, b, z, r, 'a', 1);
      for (let k = 0; k < 6; k++) {
        const t = (k / 6) * TAU;
        K.seg(dark(wood, 0.3), 0.2, [a, b, z], [a + Math.cos(t) * r * 0.92, b, z + Math.sin(t) * r * 0.92]);
      }
      K.circ(IRON, a, b, z, r, 'a', 1, 0.3);
      K.circ(dark(wood, 0.3), a, b, z, 0.22, 'a', 1);
    };
    // A handle or a prop leg, from the bed's back end down or out to the ground.
    const handle = (b) => () => {
      K.seg(wood, 0.45, [1.0, b, 2.6], [0.2, b, 2.9]);
      K.ball(0.2, b, 2.95, 0.3, dark(wood, 0.2));
    };
    const leg = (b) => () => K.seg(dark(wood, 0.15), 0.4, [1.3, b, 2.3], [1.3, b, 0]);
    // A bucket of tin with stems and flower heads, and a few leaves among them.
    const bucket = (a, b, seed) => () => {
      K.cyl(a, b, 0.5, 2.5, 3.7, '#a8b3bd', 0.42);
      K.disc(a, b, 0.36, '#5b6470', 3.7);
      // Stems first, then the heads, each head's place and colour fixed by its seed.
      const heads = [];
      for (let k = 0; k < 5; k++) {
        const h = ih(S.seed, seed * 7 + k, 3);
        const t = k * 1.25 + seed;
        const hx = a + Math.cos(t) * 0.32;
        const hy = b + Math.sin(t) * 0.26;
        const hz = 4.7 + (h % 5) * 0.14;
        K.seg(LEAF, 0.14, [a, b, 3.6], [hx, hy, hz]);
        if (k % 2 === 0) K.ball(hx - 0.2, hy, hz - 0.5, 0.18, '#6aa84f');
        heads.push([hx, hy, hz, flowers[(h >>> 4) % flowers.length]]);
      }
      for (const [hx, hy, hz, col] of heads) K.ball(hx, hy, hz, 0.3, col);
    };
    K.parts([
      // The far side: wheel, handle and prop, then the bed's floor and far wall.
      [2.1, 0.3, wheel(0.3), 0],
      [1, 0.45, handle(0.45), 0],
      [1.3, 0.45, leg(0.45), 0],
      [2.3, 0.6, () => {
        K.box(1.0, 0.4, 3.6, 1.6, 2.2, 2.5, dark(body, 0.15));
        K.box(1.0, 0.25, 3.6, 0.4, 2.5, 3.2, body);
        K.box(1.0, 0.22, 3.6, 0.42, 3.2, 3.35, lite(body, 0.2));
        K.seg(dark(body, 0.25), 0.14, [1.0, 0.41, 2.9], [3.6, 0.41, 2.9]);
      }, 0],
      // Buckets stand in the bed, taller than its low sides.
      [1.5, 0.85, bucket(1.5, 0.85, 1), 1],
      [2.3, 0.95, bucket(2.3, 0.95, 2), 1],
      [3.1, 0.8, bucket(3.1, 0.8, 3), 1],
      // The front end and the near wall, which hide the bed's near side.
      [3.5, 1.0, () => {
        K.box(3.45, 0.4, 3.6, 1.6, 2.5, 3.3, body);
        K.box(3.42, 0.38, 3.62, 1.62, 3.3, 3.45, lite(body, 0.2));
        K.seg(dark(body, 0.25), 0.14, [3.6, 0.5, 2.9], [3.6, 1.5, 2.9]);
      }, 2],
      [2.5, 1.7, () => {
        K.box(1.0, 1.6, 3.6, 1.75, 2.5, 3.2, body);
        K.box(1.0, 1.58, 3.6, 1.78, 3.2, 3.35, lite(body, 0.2));
        K.seg(dark(body, 0.25), 0.14, [1.0, 1.76, 2.9], [3.6, 1.76, 2.9]);
      }, 3],
      [1, 1.55, handle(1.55), 3],
      [1.3, 1.55, leg(1.55), 3],
      [2.1, 1.7, wheel(1.7), 3],
    ]);
  },

  // A cast-iron column on a plinth, a stone basin with water in it, and a small arc of water.
  drinking_fountain(S, item) {
    if (!S.day) return;
    const K = kit(S, item);
    const { g, at } = K;
    const iron = '#4c5260';
    const stone = '#d2ccbd';
    const water = '#6fb6d8';
    shadow(S, K.rect(0.2), 5);
    K.contact(0.5);
    K.cyl(1, 1, 0.75, 0, 0.8, dark(stone, 0.1), 0.7);
    K.cyl(1, 1, 0.45, 0.8, 4.2, iron, 0.36);
    // Flutes down the front of the column, at the angles that face the viewer.
    for (const t of [0.45, 0.785, 1.12]) {
      K.seg(dark(iron, 0.35), 0.15, [1 + 0.4 * Math.cos(t), 1 + 0.4 * Math.sin(t), 1.0], [1 + 0.4 * Math.cos(t), 1 + 0.4 * Math.sin(t), 4.0]);
    }
    K.cyl(1, 1, 0.52, 4.2, 4.4, dark(iron, 0.1), 0.5);
    // The basin: a stone bowl with the water lying in it.
    K.cyl(1, 1, 0.5, 4.4, 4.9, stone, 0.95);
    K.disc(1, 1, 0.72, water, 4.9);
    K.disc(1, 1, 0.35, lite(water, 0.4), 4.92);
    K.disc(1.3, 0.9, 0.1, '#ffffff', 4.94);
    // The bubbler and its arc: water rises from the top and falls back into the basin.
    K.rod(1, 1, 4.9, 5.6, 0.3, BRASS);
    K.ball(1, 1, 5.75, 0.3, BRASS);
    const [sx, sy] = at(1, 1, 5.7);
    const [cx, cy] = at(1.5, 1.1, 7.6);
    const [ex, ey] = at(2.05, 1.35, 4.95);
    g.beginPath();
    g.moveTo(sx, sy);
    g.quadraticCurveTo(cx, cy, ex, ey);
    g.strokeStyle = rgba('#9fd8f0', 0.9);
    g.lineWidth = 0.5;
    g.stroke();
    g.beginPath();
    g.moveTo(sx, sy);
    g.quadraticCurveTo(cx, cy, ex, ey);
    g.strokeStyle = 'rgba(255,255,255,0.9)';
    g.lineWidth = 0.1;
    g.stroke();
    // A glint on the arc where it is highest.
    const mx = 0.25 * sx + 0.5 * cx + 0.25 * ex;
    const my = 0.25 * sy + 0.5 * cy + 0.25 * ey;
    ell(g, '#ffffff', mx, my, 0.35, 0.3);
    // A brass tap on the front of the column.
    K.seg(BRASS, 0.35, [1, 1.4, 2.4], [1, 1.9, 2.5]);
    K.ball(1, 2.0, 2.5, 0.22, BRASS);
  },

  // A tall ornate street clock on a post, with a round white face on each lit side and no numerals.
  clock_post(S, item) {
    const K = kit(S, item);
    const { g, at } = K;
    const post = hexOf(item.color, '#1f3a2e');
    const face = '#fbf8ef';
    const zc = 16.1;
    const fb = 0.82;
    // The four sides of the head, as [a, b] normals. Only the ones turned to the viewer are painted.
    const sides = [[0, 1], [0, -1], [1, 0], [-1, 0]];
    if (!S.day) {
      // The faces glow softly; nothing else is drawn.
      pool(g, ...at(1, 1, 0), 9, '#ffe9b0', 0.25);
      for (const [na, nb] of sides) {
        const [du, dv] = K.dir(na, nb);
        if (du + dv <= 0.001) continue;
        const [x, y] = at(1 + fb * na, 1 + fb * nb, zc);
        glow(g, x, y, 2.6, '#fff3d0', 0.9);
        glow(g, x, y, 7, '#fff0c8', 0.55);
      }
      return;
    }
    // A point on a face: x runs to the right as seen, y up. The right is (nb, -na) for normal (na, nb).
    const onFace = ([na, nb], x, y) => [1 + fb * na + nb * x, 1 + fb * nb - na * x, zc + y];
    shadow(S, K.rect(0.3), 17);
    K.contact(0.45);
    K.cyl(1, 1, 0.7, 0, 1.2, dark(post, 0.1), 0.6);
    K.cyl(1, 1, 0.5, 1.2, 1.6, BRASS, 0.45);
    K.cyl(1, 1, 0.3, 1.6, 13.6, post, 0.24);
    K.cyl(1, 1, 0.42, 13.6, 14.0, BRASS, 0.36);
    // The head: a box with a cornice, a face on each side and a pyramid cap with a finial.
    K.box(0.18, 0.18, 1.82, 1.82, 14.0, 18.2, post);
    K.box(0.05, 0.05, 1.95, 1.95, 18.2, 18.5, lite(post, 0.12));
    for (const n of sides) {
      const [du, dv] = K.dir(n[0], n[1]);
      if (du + dv <= 0.001) continue;
      const [ca, cb] = [1 + fb * n[0], 1 + fb * n[1]];
      const axis = n[0] ? 'b' : 'a';
      const nrm = n[0] || n[1];
      // A brass rim, a white face, ticks at each hour (none for numerals), and two hands.
      K.circ(BRASS, ca, cb, zc, 0.72, axis, nrm);
      K.circ(face, ca, cb, zc, 0.6, axis, nrm);
      for (let i = 0; i < 12; i++) {
        const t = (i / 12) * TAU;
        const r1 = i % 3 === 0 ? 0.4 : 0.5;
        K.seg(dark(face, 0.7), i % 3 === 0 ? 0.14 : 0.08, onFace(n, Math.sin(t) * r1, Math.cos(t) * r1), onFace(n, Math.sin(t) * 0.57, Math.cos(t) * 0.57));
      }
      // Hands at ten past ten.
      K.seg(dark(face, 0.8), 0.22, onFace(n, 0, 0), onFace(n, -0.82 * 0.32, 0.57 * 0.32));
      K.seg(dark(face, 0.8), 0.14, onFace(n, 0, 0), onFace(n, 0.87 * 0.48, 0.5 * 0.48));
      K.circ(BRASS, ca, cb, zc, 0.07, axis, nrm);
    }
    K.q(lite(post, 0.1), [0, 1, 0.8], [0.05, 1.95, 18.5], [1.95, 1.95, 18.5], [1, 1, 20.4]);
    K.q(post, [1, 0, 0.8], [1.95, 0.05, 18.5], [1.95, 1.95, 18.5], [1, 1, 20.4]);
    K.seg(BRASS, 0.25, [1, 1, 20.4], [1, 1, 21.0]);
    K.ball(1, 1, 21.2, 0.3, BRASS);
  },
};
