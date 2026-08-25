// Design tokens for the codanna-toolset visualizations (graph disc, x-ray views).
// One source for role colours, the categorical palette, and typography; consumers
// inject cssBlock() into HTML or read jsTheme() from canvas/WebGL code at build
// time. Canon: the graph skill's vendored-template roles (light + dark), which are
// measured and shipped -- with ONE deviation, recorded here:
//
// Slot order. The template's shipped slot order fails the dataviz palette gate:
// adjacent slots 9-10 (cyan-orchid) sit at dE 2.6 light / 2.3 dark under deutan
// simulation (floor 8; adjacent slots become adjacent wedges on the disc). Same
// ten hues, re-assigned: slots 1-4 pinned (the dominant wedges keep their look),
// slots 5-10 re-ordered to violet, magenta, green, cyan, red, orchid. 86 of 720
// permutations pass; this one maximises the worst adjacent pair: min adjacent
// dE 8.4 (protan), all CVD and normal-vision checks pass in both modes
// (validate_palette, light on surface-0 #f4f3f0, dark on #121211).
//
// Light-mode contrast: six hues sit below 3:1 against the light surface-0. The
// validator marks this "relief required": consumers must keep non-colour identity
// available -- the discs' legend plus hover/click labels satisfy it. Not waivable
// by a future consumer that drops those.

const ROLES = {
  light: {
    surface0: "#f4f3f0", surface1: "#fcfcfb", surface2: "#ffffff",
    border: "#dedcd5", borderStrong: "#c4c2b8",
    text1: "#0b0b0b", text2: "#52514e", text3: "#86857d",
    edge: "#d9d7cf", edgeHi: "#2a78d6", dim: "#e6e5df",
    today: "#0b0b0b", accent: "#2a78d6",
  },
  dark: {
    surface0: "#121211", surface1: "#1a1a19", surface2: "#232322",
    border: "#33332f", borderStrong: "#4a4a45",
    text1: "#ffffff", text2: "#c3c2b7", text3: "#8d8c84",
    edge: "#333330", edgeHi: "#3987e5", dim: "#2a2a28",
    today: "#ffffff", accent: "#3987e5",
  },
};

// Categorical slots in the validated order (see header): blue, orange, aqua,
// yellow, violet, magenta, green, cyan, red, orchid.
const SLOTS = {
  light: ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#4a3aa7",
          "#e87ba4", "#008300", "#00aecb", "#e34948", "#c26ed3"],
  dark:  ["#3987e5", "#d95926", "#199e70", "#c98500", "#9085e9",
          "#d55181", "#008300", "#009fbb", "#e66767", "#b560bd"],
};

const NEUTRALS = {
  light: ["#6f6e67", "#45443f", "#8a897f"],
  dark:  ["#8d8c84", "#bdbcb2", "#77766d"],
};

const FONT = {
  ui: "ui-sans-serif, -apple-system, 'Segoe UI', system-ui, sans-serif",
  mono: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
};

// px scale as used by the graph chrome; body is base/1.45.
const SIZES = { caption: 9, micro: 10, small: 11, body: 12, base: 13, title: 14 };
const LINE_HEIGHT = 1.45;

const theme = (t) => (t === "light" ? "light" : "dark");

export function tokens(t = "dark") {
  t = theme(t);
  return { theme: t, roles: { ...ROLES[t] }, slots: [...SLOTS[t]], neutrals: [...NEUTRALS[t]], font: { ...FONT }, sizes: { ...SIZES }, lineHeight: LINE_HEIGHT };
}

const KEBAB = { surface0: "surface-0", surface1: "surface-1", surface2: "surface-2", borderStrong: "border-strong", edgeHi: "edge-hi", text1: "text-1", text2: "text-2", text3: "text-3" };

/** `<selector> { --surface-0: ...; --g1: ...; --font-ui: ...; }` for one theme.
 *  The selector matters to consumers with their own theme blocks: an override for
 *  the graph template mirrors its selectors (`:root` light, `:root:not([data-theme="light"])`
 *  dark) so the cascade resolves the same way the template's own styles do. */
export function cssBlock(t = "dark", selector = ":root") {
  const tk = tokens(t);
  const lines = [];
  for (const [k, v] of Object.entries(tk.roles)) lines.push(`  --${KEBAB[k] || k}: ${v};`);
  tk.slots.forEach((v, i) => lines.push(`  --g${i + 1}: ${v};`));
  tk.neutrals.forEach((v, i) => lines.push(`  --n${i + 1}: ${v};`));
  lines.push(`  --font-ui: ${tk.font.ui};`);
  lines.push(`  --font-mono: ${tk.font.mono};`);
  for (const [k, v] of Object.entries(tk.sizes)) lines.push(`  --fs-${k}: ${v}px;`);
  return `${selector} {\n${lines.join("\n")}\n}`;
}

/** Flat object for canvas/WebGL code that cannot read CSS custom properties. */
export function jsTheme(t = "dark") {
  const tk = tokens(t);
  return { ...tk.roles, slots: tk.slots, neutrals: tk.neutrals, fontUI: tk.font.ui, fontMono: tk.font.mono, sizes: tk.sizes, lineHeight: tk.lineHeight, dark: tk.theme === "dark" };
}

/** n categorical colours: the validated slots first, golden-angle OKLCH beyond 10
 *  at the slots' median lightness and chroma, so extras read as the same family. */
export function categorical(n, t = "dark") {
  const s = SLOTS[theme(t)];
  if (n <= s.length) return s.slice(0, n);
  return [...s, ...generatePalette(n - s.length, s, s.length)];
}

/* ---- golden-angle OKLCH generation (absorbed from graph lib/palette.mjs) ---- */
const clamp01 = (v) => Math.min(1, Math.max(0, v));
const srgbToLinear = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
const linearToSrgb = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);

function hexToOklab(hex) {
  const n = parseInt(hex.slice(1), 16);
  const r = srgbToLinear((n >> 16) & 255), g = srgbToLinear((n >> 8) & 255), b = srgbToLinear(n & 255);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
          1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
          0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s];
}
function oklabToRgb(L, a, b) {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3;
  return [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
          -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
          -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s];
}
const inGamut = (rgb) => rgb.every((c) => c >= -0.002 && c <= 1.002);
function oklchToHex(L, C, hDeg) {
  const h = (hDeg * Math.PI) / 180;
  let c = C, rgb;
  for (let i = 0; i < 40; i++) { rgb = oklabToRgb(L, c * Math.cos(h), c * Math.sin(h)); if (inGamut(rgb)) break; c *= 0.92; }
  return "#" + rgb.map((v) => Math.round(clamp01(linearToSrgb(clamp01(v))) * 255).toString(16).padStart(2, "0")).join("");
}
const median = (xs) => { const s = [...xs].sort((a, b) => a - b); const m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };

/** n colours in the family of `slots`; hue steps by the golden angle from slot 1,
 *  offset by `skip` steps so a continuation does not restart at slot 1's hue. */
export function generatePalette(n, slots, skip = 0) {
  const labs = slots.map(hexToOklab);
  const L = median(labs.map((x) => x[0]));
  const C = median(labs.map((x) => Math.hypot(x[1], x[2])));
  const h0 = (Math.atan2(labs[0][2], labs[0][1]) * 180) / Math.PI;
  const GOLDEN = 137.50776405;
  return Array.from({ length: n }, (_, i) => oklchToHex(L, C, (h0 + (skip + i) * GOLDEN) % 360));
}

/* ------------------------------------------------------------- smoke CLI */
import { pathToFileURL } from "node:url";
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const t = process.argv.includes("--light") ? "light" : "dark";
  if (process.argv.includes("--css")) console.log(cssBlock(t));
  else console.log(JSON.stringify({ theme: t, roles: tokens(t).roles, slots: SLOTS[t], categorical14: categorical(14, t) }, null, 2));
}
