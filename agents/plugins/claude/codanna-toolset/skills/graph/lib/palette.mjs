// A generated wedge palette for indexes with more groups than the template's ten
// documented hue slots. Hues step by the golden angle, so consecutive groups (which sit
// side by side on the disc) land far apart on the hue circle; lightness and chroma are
// the median of the documented slots for that theme, so the result reads as one family.
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

/** The template's documented slots per theme, read from its own CSS (light block first, dark second). */
export function documentedSlots(templateHtml) {
  const blocks = [];
  const re = /--g1:\s*(#[0-9a-f]{6})[^]*?--g10:\s*(#[0-9a-f]{6})/gi;
  let m;
  while ((m = re.exec(templateHtml))) {
    const hexes = [...m[0].matchAll(/--g(\d+):\s*(#[0-9a-f]{6})/gi)].sort((a, b) => +a[1] - +b[1]).map((x) => x[2].toLowerCase());
    if (hexes.length === 10) blocks.push(hexes);
  }
  return { light: blocks[0] || [], dark: blocks[1] || blocks[0] || [] };
}

/** n colours in the documented family of `slots` (hex list); golden-angle hue stepping from slot 1's hue. */
export function generatePalette(n, slots) {
  const labs = slots.map(hexToOklab);
  const L = median(labs.map((x) => x[0]));
  const C = median(labs.map((x) => Math.hypot(x[1], x[2])));
  const h0 = (Math.atan2(labs[0][2], labs[0][1]) * 180) / Math.PI;
  const GOLDEN = 137.50776405;
  return Array.from({ length: n }, (_, i) => oklchToHex(L, C, (h0 + i * GOLDEN) % 360));
}
