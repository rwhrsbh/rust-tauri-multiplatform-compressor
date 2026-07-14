// Генерирует исходную иконку 1024x1024 (PNG) для `tauri icon`.
// Тёмный фон с неоновым градиентом и символом сжатия (шевроны к центру).
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const S = 1024;
const px = new Uint8Array(S * S * 4);

const lerp = (a, b, t) => a + (b - a) * t;

// Расстояние до полосы шеврона "V" толщиной w, вершина в (cx, cy), размах arm
function chevron(x, y, cx, cy, arm, w, dir) {
  // dir=1: остриё вниз, dir=-1: остриё вверх
  const dx = Math.abs(x - cx);
  if (dx > arm) return false;
  const yLine = cy - dir * dx * 0.55;
  return Math.abs(y - yLine) < w;
}

for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    const i = (y * S + x) * 4;
    const t = y / S;
    // Фон: тёмно-синий -> тёмно-фиолетовый
    let r = lerp(8, 24, t);
    let g = lerp(10, 8, t);
    let b = lerp(22, 40, t);

    // Мягкое центральное свечение
    const d = Math.hypot(x - S / 2, y - S / 2) / (S / 2);
    const glow = Math.max(0, 1 - d) ** 2;
    r += glow * 8;
    g += glow * 30;
    b += glow * 45;

    // Два шеврона, сжимающихся к центру
    const arm = S * 0.26;
    const w = S * 0.045;
    const top = chevron(x, y, S / 2, S * 0.40, arm, w, 1);
    const bot = chevron(x, y, S / 2, S * 0.60, arm, w, -1);
    if (top || bot) {
      // Неоновый циан/фуксия
      const k = top ? 1 : 0.85;
      r = 40 + 180 * (1 - k);
      g = 211 * k + 60 * (1 - k);
      b = 238;
    } else {
      // Ореол вокруг шевронов
      for (const [cy, dir] of [
        [S * 0.4, 1],
        [S * 0.6, -1],
      ]) {
        const dx = Math.abs(x - S / 2);
        if (dx <= arm * 1.15) {
          const yLine = cy - dir * dx * 0.55;
          const dist = Math.abs(y - yLine);
          if (dist < w * 3) {
            const a = (1 - dist / (w * 3)) ** 2 * 0.5;
            r = lerp(r, 34, a);
            g = lerp(g, 211, a);
            b = lerp(b, 238, a);
          }
        }
      }
    }

    px[i] = Math.min(255, r | 0);
    px[i + 1] = Math.min(255, g | 0);
    px[i + 2] = Math.min(255, b | 0);
    px[i + 3] = 255;
  }
}

// --- Минимальный PNG-энкодер ---
const crcTable = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});
const crc32 = (buf) => {
  let c = -1;
  for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
};

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(S, 0);
ihdr.writeUInt32BE(S, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA

const raw = Buffer.alloc(S * (S * 4 + 1));
for (let y = 0; y < S; y++) {
  raw[y * (S * 4 + 1)] = 0; // filter: none
  Buffer.from(px.buffer, y * S * 4, S * 4).copy(raw, y * (S * 4 + 1) + 1);
}

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

writeFileSync(new URL("./icon-source.png", import.meta.url), png);
console.log("icon-source.png written:", png.length, "bytes");
