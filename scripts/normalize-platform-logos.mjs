#!/usr/bin/env node
/**
 * 一次性脚本：把三家外部平台的官方图标归一化为本仓产品图标的统一参数。
 *
 * 目标参数（与 `src/components/product-marks.tsx` 里 WorkBuddy / Trae / Qoder 一致）：
 *   画布 256×256 透明，内容占画布 80.4%，严格居中。
 * 渲染侧统一用「放大 118% 居中裁掉透明圈 + 容器 rounded-[22%]」。
 *
 * 输入来自各官网公开的 favicon / apple-touch-icon：
 *   doubao  https://lf-flow-web-cdn.doubao.com/obj/flow-doubao/doubao/doubao_avatar_red_favicon.png
 *   loomy   https://loomy.xunfei.cn/icon.png
 *   raccoon https://sta-volc.xiaohuanxiong.com/raccoon-website/prod/mime-v1/brand/apple-touch-icon.png
 */
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { fileURLToPath } from "node:url";

const CANVAS = 256;
const CONTENT_RATIO = 0.804;

function decodePng(file) {
  const buf = fs.readFileSync(file);
  let pos = 8, width = 0, height = 0, colorType = 0;
  const idat = [];
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.toString("ascii", pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      colorType = data[9];
    }
    if (type === "IDAT") idat.push(data);
    pos += 12 + len;
  }
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const bpp = colorType === 6 ? 4 : colorType === 2 ? 3 : 1;
  const stride = width * bpp;
  const out = Buffer.alloc(width * height * bpp);
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const cur = Buffer.alloc(stride);
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? cur[i - bpp] : 0;
      const b = prev[i];
      const c = i >= bpp ? prev[i - bpp] : 0;
      const x = line[i];
      let v;
      switch (filter) {
        case 0: v = x; break;
        case 1: v = x + a; break;
        case 2: v = x + b; break;
        case 3: v = x + ((a + b) >> 1); break;
        case 4: {
          const pa = Math.abs(b - c), pb = Math.abs(a - c), pc = Math.abs(a + b - 2 * c);
          v = x + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c);
          break;
        }
        default: v = x;
      }
      cur[i] = v & 255;
    }
    cur.copy(out, y * stride);
    prev = cur;
  }
  return { width, height, bpp, data: out };
}

function cropToContent({ width, height, bpp, data }) {
  let minX = width, maxX = -1, minY = height, maxY = -1;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const o = (y * width + x) * bpp;
      const a = bpp === 4 ? data[o + 3] : 255;
      if (a > 8) {
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
    }
  }
  return { minX, minY, w: maxX - minX + 1, h: maxY - minY + 1 };
}

/** 双线性采样到目标画布，内容按 CONTENT_RATIO 缩放并居中。 */
function render(src, box, target) {
  const scale = (target * CONTENT_RATIO) / Math.max(box.w, box.h);
  const drawW = box.w * scale;
  const drawH = box.h * scale;
  const offX = (target - drawW) / 2;
  const offY = (target - drawH) / 2;

  const out = Buffer.alloc(target * target * 4); // RGBA
  const sample = (sx, sy) => {
    const x0 = Math.max(0, Math.min(src.width - 1, Math.floor(sx)));
    const y0 = Math.max(0, Math.min(src.height - 1, Math.floor(sy)));
    const o = (y0 * src.width + x0) * src.bpp;
    const a = src.bpp === 4 ? src.data[o + 3] : 255;
    // 透明像素的 RGB 无意义，置 0 避免采样时串色
    if (a === 0) return [0, 0, 0, 0];
    return [src.data[o], src.data[o + 1], src.data[o + 2], a];
  };

  for (let y = 0; y < target; y++) {
    for (let x = 0; x < target; x++) {
      const dx = x + 0.5 - offX;
      const dy = y + 0.5 - offY;
      const o = (y * target + x) * 4;
      if (dx < 0 || dy < 0 || dx >= drawW || dy >= drawH) continue;
      const sx = box.minX + dx / scale;
      const sy = box.minY + dy / scale;
      // 2×2 超采样，压掉缩小时的锯齿
      let r = 0, g = 0, b = 0, a = 0, n = 0;
      for (const ox of [-0.25, 0.25]) {
        for (const oy of [-0.25, 0.25]) {
          const [pr, pg, pb, pa] = sample(sx + ox, sy + oy);
          r += pr * pa; g += pg * pa; b += pb * pa; a += pa; n++;
        }
      }
      if (a > 0) {
        out[o] = Math.round(r / a);
        out[o + 1] = Math.round(g / a);
        out[o + 2] = Math.round(b / a);
        out[o + 3] = Math.round(a / n);
      }
    }
  }
  return out;
}

function encodePng(rgba, size) {
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0; // filter: none
    rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body) >>> 0);
    return Buffer.concat([len, body, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;   // bit depth
  ihdr[9] = 6;   // RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

let table = null;
function crc32(buf) {
  if (!table) {
    table = new Int32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      table[n] = c;
    }
  }
  let crc = -1;
  for (const byte of buf) crc = table[(crc ^ byte) & 255] ^ (crc >>> 8);
  return crc ^ -1;
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const jobs = [
  ["doubao.png", "doubao-official-icon.png"],
  ["loomy.png", "loomy-official-icon.png"],
  ["raccoon-touch.png", "raccoon-official-icon.png"],
];

for (const [input, output] of jobs) {
  const src = decodePng(path.join(root, ".tmp-logo", input));
  const box = cropToContent(src);
  const rgba = render(src, box, CANVAS);
  const png = encodePng(rgba, CANVAS);
  fs.writeFileSync(path.join(root, "src/assets", output), png);
  console.log(
    `${input} ${src.width}x${src.height} 内容 ${box.w}x${box.h} -> src/assets/${output} (${png.length}b)`,
  );
}
