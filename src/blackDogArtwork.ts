import artworkUrl from './assets/little-black-dog.png';

interface Point {
  x: number;
  y: number;
}

const SCALE = 0.22;
const SEAT_X = 654;
const SEAT_Y = 1196;
const HEAD_X = 615;
const HEAD_Y = 465;
const HEAD_OFFSET_Y = 11;

// Masks use the approved illustration's own coordinates.
const TORSO = new Path2D('M 405 606 L 851 569 Q 894 717 872 846 Q 925 882 943 1011 L 944 1218 H 316 V 865 Q 350 801 405 606 Z');
const TAIL = new Path2D('M 895 740 H 1072 V 1090 H 906 Q 940 1009 911 904 L 895 862 Z');
const FACE = new Path2D('M 369 509 Q 365 406 446 308 Q 500 263 555 241 Q 634 212 766 280 Q 874 305 894 463 L 900 569 Q 826 637 694 653 Q 565 685 423 654 L 352 615 Z');
const EARS = [
  {
    outer: new Path2D('M 384 519 C 339 496 281 417 275 351 C 267 292 287 268 332 274 Q 395 278 461 311 Q 487 318 489 359 C 496 420 455 500 384 519 Z'),
    inner: new Path2D('M 392 448 Q 341 419 327 354 Q 319 320 339 325 Q 390 329 426 349 Q 401 389 392 448 Z'),
    x: 406, y: 405, foldAngle: -0.88, lower: 13,
  },
  {
    outer: new Path2D('M 546 268 Q 551 237 552 187 C 555 136 568 91 605 94 C 669 97 749 194 781 292 L 720 338 L 573 315 Z'),
    inner: new Path2D('M 605 234 Q 608 204 612 178 Q 613 151 623 156 Q 660 174 700 243 Q 656 231 605 234 Z'),
    x: 665, y: 262, foldAngle: 1.62, lower: 73,
  },
];
const RESTING_PAWS = [
  new Path2D('M 425 821 H 570 Q 618 962 614 1111 L 621 1224 H 440 L 446 1160 Q 456 1130 478 1101 Q 466 971 425 821 Z'),
  new Path2D('M 716 818 H 836 Q 800 966 798 1103 Q 823 1145 821 1223 H 656 L 663 1127 Q 664 969 716 818 Z'),
];
const EYES = [
  { x: 526, y: 476, rx: 36, ry: 35 },
  { x: 705, y: 394, rx: 37, ry: 35 },
];

function makeTorso(texture: HTMLCanvasElement): HTMLCanvasElement {
  const torso = document.createElement('canvas');
  torso.width = texture.width;
  torso.height = texture.height;
  const ctx = torso.getContext('2d')!;
  ctx.clip(TORSO);
  ctx.drawImage(texture, 0, 0);
  ctx.globalCompositeOperation = 'source-atop';
  const fur = ctx.createLinearGradient(0, 808, 0, 1140);
  fur.addColorStop(0, '#070707');
  fur.addColorStop(1, '#020202');
  ctx.fillStyle = fur;
  for (const paw of RESTING_PAWS) ctx.fill(paw);
  ctx.globalCompositeOperation = 'destination-out';
  ctx.fillStyle = '#000';
  // The belly has one level edge between the hind feet. Only the resting paw
  // layers may extend below this edge.
  ctx.fillRect(443, 1139, 378, torso.height - 1139);
  return torso;
}

function isolateArtwork(image: HTMLImageElement): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = image.naturalWidth;
  canvas.height = image.naturalHeight;
  const ctx = canvas.getContext('2d')!;
  ctx.drawImage(image, 0, 0);
  const { width, height } = canvas;
  const pixels = ctx.getImageData(0, 0, width, height);
  const data = pixels.data;
  const visited = new Uint8Array(width * height);
  const queue = new Uint32Array(visited.length);
  let end = 0;

  // Only exterior light pixels are paper. Enclosed eye/arm highlights stay opaque.
  const visit = (i: number) => {
    if (visited[i]) return;
    visited[i] = 1;
    const p = i * 4;
    const light = Math.min(data[p], data[p + 1], data[p + 2]);
    if (light < 36) return;
    queue[end++] = i;
    data[p + 3] = light >= 224 ? 0 : Math.round(255 * (1 - light / 255));
    data[p] = data[p + 1] = data[p + 2] = 0;
  };
  visit(0);
  for (let i = 0; i < end; i++) {
    const pixel = queue[i];
    const x = pixel % width;
    if (x > 0) visit(pixel - 1);
    if (x < width - 1) visit(pixel + 1);
    if (pixel >= width) visit(pixel - width);
    if (pixel + width < visited.length) visit(pixel + width);
  }
  ctx.putImageData(pixels, 0, 0);
  return canvas;
}

export class BlackDogArtwork {
  private texture: HTMLCanvasElement | null = null;
  private torso: HTMLCanvasElement | null = null;

  constructor() {
    const image = new Image();
    image.onload = () => {
      this.texture = isolateArtwork(image);
      this.torso = makeTorso(this.texture);
    };
    image.src = artworkUrl;
  }

  private drawMasked(ctx: CanvasRenderingContext2D, mask: Path2D): void {
    ctx.save();
    ctx.clip(mask);
    ctx.drawImage(this.texture!, 0, 0);
    ctx.restore();
  }

  drawTail(ctx: CanvasRenderingContext2D, seat: Point, wag: number): void {
    if (!this.texture) return;
    ctx.save();
    ctx.translate(seat.x, seat.y);
    ctx.scale(SCALE, SCALE);
    ctx.translate(-SEAT_X, -SEAT_Y);
    ctx.translate(875, 985);
    ctx.rotate(wag);
    ctx.translate(-875, -985);
    ctx.fillStyle = '#040404';
    ctx.beginPath();
    ctx.moveTo(838, 954);
    ctx.quadraticCurveTo(900, 983, 949, 942);
    ctx.lineTo(965, 1017);
    ctx.quadraticCurveTo(931, 1070, 838, 1063);
    ctx.fill();
    this.drawMasked(ctx, TAIL);
    ctx.restore();
  }

  draw(
    ctx: CanvasRenderingContext2D,
    seat: Point,
    head: Point,
    tilt: number,
    folded: number,
    eyelid: number,
    gaze: Point,
    leftAtRest: boolean,
    rightAtRest: boolean,
  ): void {
    if (!this.texture || !this.torso) return;
    const headX = head.x + (HEAD_X - SEAT_X) * SCALE;
    const headY = head.y + HEAD_OFFSET_Y;
    ctx.save();
    ctx.translate(seat.x, seat.y);
    // Keep the seat planted while the upper body follows the existing typing and mouse lean.
    ctx.transform(1, 0, (head.x - seat.x) / -161, Math.max(0.84, (seat.y - headY) / 161), 0, 0);
    ctx.scale(SCALE, SCALE);
    ctx.translate(-SEAT_X, -SEAT_Y);
    ctx.drawImage(this.torso, 0, 0);
    if (leftAtRest) this.drawMasked(ctx, RESTING_PAWS[0]);
    if (rightAtRest) this.drawMasked(ctx, RESTING_PAWS[1]);
    ctx.restore();

    ctx.save();
    ctx.translate(headX, headY);
    ctx.rotate(tilt);
    ctx.scale(SCALE, SCALE);
    ctx.translate(-HEAD_X, -HEAD_Y);
    // Keep the left ear root tucked under the head through the full fold.
    ctx.fillStyle = '#050505';
    ctx.beginPath();
    ctx.moveTo(347, 378);
    ctx.bezierCurveTo(372, 366, 411, 332, 455, 347);
    ctx.bezierCurveTo(481, 369, 442, 438, 402, 456);
    ctx.bezierCurveTo(381, 462, 370, 443, 353, 424);
    ctx.quadraticCurveTo(333, 400, 347, 378);
    ctx.fill();
    for (const { outer, inner, x, y, foldAngle, lower } of EARS) {
      ctx.save();
      ctx.translate(x, y + folded * lower);
      ctx.rotate(foldAngle * folded);
      ctx.translate(-x, -y);
      const fur = ctx.createLinearGradient(x, y, x, y - 165);
      fur.addColorStop(0, '#030303');
      fur.addColorStop(1, '#080808');
      ctx.fillStyle = fur;
      ctx.fill(outer);
      ctx.fillStyle = '#0e0e0e';
      ctx.fill(inner);
      ctx.restore();
    }
    this.drawMasked(ctx, FACE);
    // Follow the pointer with the pupils. The white eye shapes stay in place.
    for (const { x, y, rx, ry } of EYES) {
      ctx.save();
      ctx.fillStyle = '#030303';
      ctx.beginPath();
      ctx.ellipse(x, y, rx + 5, ry + 6, 0, 0, Math.PI * 2);
      ctx.fill();
      const opening = ry * (1 - eyelid);
      if (opening < 2.5) {
        ctx.strokeStyle = '#343434';
        ctx.lineWidth = 4;
        ctx.beginPath();
        ctx.moveTo(x - rx * 0.65, y);
        ctx.quadraticCurveTo(x, y + 13, x + rx * 0.65, y);
        ctx.stroke();
        ctx.restore();
        continue;
      }
      ctx.fillStyle = '#f4f3f2';
      ctx.beginPath();
      ctx.ellipse(x, y, rx, opening, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.clip();
      const pupilX = x + gaze.x * 17;
      const pupilY = y - opening * 0.10 + gaze.y * opening * 0.43;
      ctx.fillStyle = '#050505';
      ctx.beginPath();
      ctx.ellipse(pupilX, pupilY, rx * 0.8, opening * 0.86, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = '#fff';
      ctx.beginPath();
      ctx.ellipse(pupilX - 9, pupilY - opening * 0.29, 9, Math.min(9, opening * 0.43), 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
    ctx.restore();
  }
}
