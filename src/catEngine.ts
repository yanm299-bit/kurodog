import {
  KEYBOARD_KEYS_65,
  KEYBOARD_KEYS_108,
  getKeyboardLayout,
  type KeySpec,
  type KeyboardLayoutId,
  formatBubbleForVk,
  resolveKeyByVk,
} from './keyboardLayout';
import { BlackDogArtwork } from './blackDogArtwork';

export const LOGICAL_W = 420;
export const LOGICAL_H = 440;
export type KeyboardOrientation = 'cat' | 'viewer';

interface Vec2 {
  x: number;
  y: number;
}

type PawMode = 'idle' | 'curled' | 'poke' | 'mouse';

interface PawState {
  side: 'left' | 'right';
  shoulderNormY: number; // vertical fraction along spine (0=ear tips, 1=base)
  shoulderHalfX: number;
  pos: Vec2;
  vel: Vec2;
  target: Vec2;
  mode: PawMode;
  idleWeight: number;
  curlWeight: number;
  pokeTime: number;
  impactSquash: number;
  lastActiveAt: number;
}

interface KeyBubble {
  id: number;
  label: string;
  isSpace: boolean;
  x: number;
  targetX: number;
  y: number;
  targetY: number;
  rot: number;
  createdAt: number;
  lifetime: number;
}

interface KeyAnim {
  pressed: boolean;
  pressAmount: number;
  lastHitAt: number;
}

const DOG_FUR = '#050505';

export class KuroCatEngine {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private rafId = 0;
  private lastTime = 0;
  private elapsed = 0;

  public showKeyBubbles = true;
  public privacyMode = false;
  public keyboardLayout: KeyboardLayoutId = '65';
  public keyboardOrientation: KeyboardOrientation = 'cat';
  private renderScale = 1.0;
  private embedded = false;
  private dogArtwork = new BlackDogArtwork();
  private privacyClose = 0;
  private privacyTypingAt = -10000;
  private lastMouseNX = 0.5;

  public setPrivacyMode(active: boolean): void {
    this.privacyMode = active;
    if (active) {
      this.bubbles.length = 0;
      for (const anim of this.keyAnims.values()) {
        anim.pressed = false;
        anim.pressAmount = 0;
      }
    }
  }

  public onPrivacyTyping(): void {
    this.privacyTypingAt = performance.now();

  }

  public setRenderScale(scale: number): void {
    this.renderScale = Math.max(0.2, Math.min(1.5, scale));
  }

  public getLogicalSize(): { w: number; h: number } {
    return { w: this.logicalW, h: this.logicalH };
  }

  private syncLayoutPositions(): void {
    if (this.keyboardLayout === '108') {
      this.logicalW = 540;
      this.logicalH = 456;
      this.catCenterX = 278;
      this.catTopY = 66;
      this.catBaseY = 292;
      this.kbCenterX = 290;
      this.kbCenterY = 356;
      this.kbWidth = 398;
      this.kbHeight = 96;
      this.kbTiltRad = (2.4 * Math.PI) / 180;
      this.mouseBaseX = 46;
      this.mouseBaseY = 342;
    } else {
      this.logicalW = 420;
      this.logicalH = 440;
      this.catCenterX = 196;
      this.catTopY = 82;
      this.catBaseY = 308;
      this.kbCenterX = 214;
      this.kbCenterY = 353;
      this.kbWidth = 266;
      this.kbHeight = 74;
      this.kbTiltRad = (3.4 * Math.PI) / 180;
      this.mouseBaseX = 46;
      this.mouseBaseY = 338;
    }
  }

  public setKeyboardConfig(layout: KeyboardLayoutId, orientation: KeyboardOrientation): void {
    const nextLayout: KeyboardLayoutId = layout === '108' ? '108' : '65';
    const nextOrientation: KeyboardOrientation = orientation === 'viewer' ? 'viewer' : 'cat';
    const layoutChanged = this.keyboardLayout !== nextLayout;
    this.keyboardLayout = nextLayout;
    this.keyboardOrientation = nextOrientation;

    this.syncLayoutPositions();

    if (layoutChanged) {
      this.leftPaw.pos = { x: this.catCenterX - 26, y: this.catBaseY + 2 };
      this.leftPaw.target = { x: this.catCenterX - 26, y: this.catBaseY + 2 };
      this.rightPaw.pos = { x: this.catCenterX + 19, y: this.catBaseY + 2 };
      this.rightPaw.target = { x: this.catCenterX + 19, y: this.catBaseY + 2 };
    }
  }

  private logicalW = 420;
  private logicalH = 440;
  private catCenterX = 196;
  private catTopY = 82;
  private catBaseY = 308;
  private readonly catHeight = 226;

  // Mechanical keyboard deck below the sitting cat
  private kbCenterX = 214;
  private kbCenterY = 353;
  private kbWidth = 266;
  private kbHeight = 74;
  private kbTiltRad = (3.4 * Math.PI) / 180;

  // True-scale white oval mouse to the left of keyboard
  private mouseBaseX = 46;
  private mouseBaseY = 338;
  private mouseOffsetX = 0;
  private mouseOffsetY = 0;
  private mouseTargetOffsetX = 0;
  private mouseTargetOffsetY = 0;
  private mouseLeftDown = false;
  private mouseRightDown = false;
  private mouseMiddleDown = false;
  private mouseLeftPress = 0;
  private mouseRightPress = 0;
  private mouseMiddlePress = 0;
  private mouseScrollFlash = 0;
  private lastMouseMoveAt = -10000;
  private mouseBadges: Array<{
    id: number;
    label: string;
    color: string;
    x: number;
    y: number;
    createdAt: number;
    lifetime: number;
  }> = [];

  // Cursor & 100% live eye tracking + 2.5D head turn
  private cursorRelX = 0.5;
  private cursorRelY = 0.5;
  private cursorOverCat = false;
  private eyeLookX = 0;
  private eyeLookY = 0;

  // Spine-shear deformation state (exact measured flexible cat spine + playful typing body/head sway!)
  private spineLean = 0;    // -1 (mouse lean: head shifts -46px left while hips stay anchored!) .. +1
  private headTilt = 0;
  private typingHunch = 0;  // 0..1 lowers head & rounds posture slightly when typing
  private typingSway = 0;   // -1..+1 lively left-right torso & head sway while typing!
  private typingSwayVel = 0;
  private typingGroovePhase = 0;
  private lastKeyBias = 0;
  private strikeRock = 0;
  private strikeRockVel = 0;
  private bodyBob = 0;
  private kbBounce = 0;

  // Blinking, petting, ear micro-twitch, typing rhythm
  private nextBlinkAt = 2.8;
  private blinkProgress = 0;
  private isBlinking = false;

  private lastPetCursorX: number | null = null;
  private lastPetCursorY: number | null = null;
  private petEnergy = 0;
  private squintAmount = 0;
  private recentKeyTimes: number[] = [];
  private typingEnergy = 0;
  private lastAnyKeyAt = -10000;

  // Paws
  private leftPaw: PawState;
  private rightPaw: PawState;
  private lastSpacePaw: 'left' | 'right' = 'left';

  // Keys & bubbles
  private keyAnims = new Map<string, KeyAnim>();
  private bubbles: KeyBubble[] = [];
  private nextBubbleId = 1;

  constructor(canvas: HTMLCanvasElement, options?: { embedded?: boolean }) {
    this.canvas = canvas;
    this.embedded = Boolean(options?.embedded);
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Could not initialize 2D canvas context');
    this.ctx = ctx;

    for (const k of KEYBOARD_KEYS_65) {
      this.keyAnims.set(k.id, { pressed: false, pressAmount: 0, lastHitAt: -10000 });
    }
    for (const k of KEYBOARD_KEYS_108) {
      if (!this.keyAnims.has(k.id)) {
        this.keyAnims.set(k.id, { pressed: false, pressAmount: 0, lastHitAt: -10000 });
      }
    }

    this.leftPaw = {
      side: 'left',
      shoulderNormY: 0.50,
      shoulderHalfX: -32,
      pos: { x: this.catCenterX - 26, y: this.catBaseY + 2 },
      vel: { x: 0, y: 0 },
      target: { x: this.catCenterX - 26, y: this.catBaseY + 2 },
      mode: 'idle',
      idleWeight: 1,
      curlWeight: 0,
      pokeTime: 1,
      impactSquash: 0,
      lastActiveAt: -10000,
    };

    this.rightPaw = {
      side: 'right',
      shoulderNormY: 0.50,
      shoulderHalfX: 32,
      pos: { x: this.catCenterX + 19, y: this.catBaseY + 2 },
      vel: { x: 0, y: 0 },
      target: { x: this.catCenterX + 19, y: this.catBaseY + 2 },
      mode: 'idle',
      idleWeight: 1,
      curlWeight: 0,
      pokeTime: 1,
      impactSquash: 0,
      lastActiveAt: -10000,
    };

    this.resize();
    window.addEventListener('resize', () => this.resize());
  }

  public resize(): void {
    const dpr = Math.max(window.devicePixelRatio || 1, 1);
    const cssW = Math.max(1, this.embedded ? (this.canvas.clientWidth || 420) : window.innerWidth);
    const cssH = Math.max(1, this.embedded ? (this.canvas.clientHeight || 440) : window.innerHeight);
    this.canvas.width = Math.round(cssW * dpr);
    this.canvas.height = Math.round(cssH * dpr);
  }

  public start(): void {
    this.lastTime = performance.now();
    const tick = (now: number) => {
      const dt = Math.min(0.05, Math.max(0.001, (now - this.lastTime) / 1000));
      this.lastTime = now;
      this.update(dt, now);
      this.render();
      this.rafId = requestAnimationFrame(tick);
    };
    this.rafId = requestAnimationFrame(tick);
  }

  public stop(): void {
    cancelAnimationFrame(this.rafId);
  }

  // ---- Flexible spine coordinate system (exact match to measured 0.3s -> 2.2s shear!) ----

  private spinePoint(t: number, breath: number): Vec2 {
    // t in [0..1]: 0 = ear tips, 1 = seated base on desk
    const u = Math.max(0, Math.min(1, 1 - t)); // 0 at hips, 1 at head
    const shearCurve = u * u * (3 - 2 * u); // smoothstep from hips up to head!
    // Measured mouse-lean shift (-46px at head) + lively typing left-right body/head sway (+-25px)!
    const leanAmp = this.keyboardLayout === '108' ? 76 : 46;
    const leanX = (this.spineLean * leanAmp + this.typingSway * 25 + this.strikeRock * 10) * shearCurve;
    const swayDipY = Math.abs(this.typingSway) * 4.0 * shearCurve;
    const hunchY = (this.typingHunch * 13 + this.bodyBob * 4.8 + breath) * shearCurve + swayDipY;
    // Slight forward drop when leaning deeply to the mouse
    const leanDropY = Math.abs(this.spineLean) * 6 * shearCurve;
    return {
      x: this.catCenterX + leanX,
      y: this.catTopY + t * this.catHeight + hunchY + leanDropY,
    };
  }

  private getLiveShoulder(paw: PawState, breath: number): Vec2 {
    const sp = this.spinePoint(paw.shoulderNormY, breath);
    return {
      x: sp.x + paw.shoulderHalfX,
      y: sp.y,
    };
  }

  private getLiveCurledPawPos(paw: PawState, breath: number): Vec2 {
    // Bent forearm elbow -> cute curled paw sitting near lower belly (matches 8.3s / 10.6s!)
    const sp = this.spinePoint(0.84, breath);
    const bob = Math.sin(this.elapsed * 12 + (paw.side === 'left' ? 0 : Math.PI)) * (1.5 + this.typingEnergy * 2.0);
    const xOff = paw.side === 'left' ? -24 : 24;
    return {
      x: sp.x + xOff,
      y: sp.y + bob,
    };
  }

  // ---- Input event handlers ----

  public onKeyDown(vk: number, repeat: boolean): void {
    if (this.privacyMode) {
      if (!repeat) this.onPrivacyTyping();
      return;
    }
    const now = performance.now();
    const spec = resolveKeyByVk(vk, this.keyboardLayout);
    if (!spec) return;

    const anim = this.keyAnims.get(spec.id);
    if (anim) {
      anim.pressed = true;
      anim.pressAmount = 1;
      anim.lastHitAt = now;
    }

    if (repeat) return;

    this.lastAnyKeyAt = now;
    this.recentKeyTimes.push(now);
    if (this.recentKeyTimes.length > 26) this.recentKeyTimes.shift();

    this.bodyBob = Math.min(1.15, this.bodyBob + 0.65);
    this.kbBounce = Math.min(1, this.kbBounce + 0.45);

    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const keyColCenter = spec.uStart + spec.uWidth * 0.5;
    const screenKeyCol = isCatFacing ? layout.totalUnits - keyColCenter : keyColCenter;
    const midCol = layout.totalUnits * 0.5;
    const keyBias = Math.max(-1, Math.min(1, (screenKeyCol - midCol) / (layout.totalUnits * 0.34)));
    this.lastKeyBias = keyBias;
    this.typingGroovePhase += 1.22;

    let targetPoint = this.getKeyWorldCenter(spec);
    let chosen: PawState;

    const activelyMousing = this.mouseLeftDown || this.mouseRightDown || now - this.lastMouseMoveAt < 320;
    const splitCol =
      this.keyboardLayout === '108'
        ? isCatFacing
          ? 12.2
          : 8.4
        : 7.5;

    if (spec.id === 'space') {
      if (activelyMousing) {
        chosen = this.rightPaw;
      } else {
        this.lastSpacePaw = this.lastSpacePaw === 'left' ? 'right' : 'left';
        chosen = this.lastSpacePaw === 'left' ? this.leftPaw : this.rightPaw;
      }
      const spaceU = isCatFacing
        ? chosen.side === 'left'
          ? spec.uStart + spec.uWidth * 0.68
          : spec.uStart + spec.uWidth * 0.32
        : chosen.side === 'left'
          ? spec.uStart + spec.uWidth * 0.32
          : spec.uStart + spec.uWidth * 0.68;
      const rY = spec.rowY ?? spec.row;
      targetPoint = this.projectKeyboardPoint(spaceU, rY + 0.50);
    } else if (activelyMousing) {
      chosen = this.rightPaw;
    } else if (screenKeyCol < splitCol) {
      chosen = this.leftPaw;
    } else {
      chosen = this.rightPaw;
    }

    const sideSign = chosen.side === 'left' ? -1 : 1;
    this.strikeRockVel += sideSign * (1.75 + this.typingEnergy * 1.25);
    if (!activelyMousing) {
      // Lively groove impulse: rocks head & upper body back and forth as the cat types!
      this.typingSwayVel += (keyBias * 2.1 + Math.cos(this.typingGroovePhase) * 1.65) * (0.9 + this.typingEnergy * 0.45);
    }

    chosen.mode = 'poke';
    chosen.target = this.clampToKeyboardSurface(targetPoint);
    chosen.pokeTime = 0;
    chosen.impactSquash = 0.85;
    chosen.lastActiveAt = now;

    const other = chosen.side === 'left' ? this.rightPaw : this.leftPaw;
    if (other.mode === 'idle' && !activelyMousing) {
      other.mode = 'curled';
      other.lastActiveAt = now;
    }

    if (this.showKeyBubbles) {
      const label = formatBubbleForVk(vk, spec);
      if (label) {
        this.spawnBubble(label, spec.id === 'space', targetPoint.x, now);
      }
    }
  }

  public onKeyUp(vk: number): void {
    if (this.privacyMode) return;
    const spec = resolveKeyByVk(vk, this.keyboardLayout);
    if (!spec) return;
    const anim = this.keyAnims.get(spec.id);
    if (anim) anim.pressed = false;
  }

  public onGlobalMouseMove(nx: number, ny: number): void {
    const now = performance.now();
    this.lastMouseNX = nx;
    this.mouseTargetOffsetX = (nx - 0.5) * 28;
    this.mouseTargetOffsetY = (ny - 0.5) * 20;
    this.lastMouseMoveAt = now;

    this.leftPaw.mode = 'mouse';
    this.leftPaw.lastActiveAt = now;
  }

  private getMouseButtonColors(): { left: string; right: string; middle: string } {
    return { left: '#d4f044', right: '#ff7b3b', middle: '#92ba12' };
  }

  private spawnMouseBadge(button: 'left' | 'right' | 'middle', now: number): void {
    if (!this.showKeyBubbles) return;
    const colors = this.getMouseButtonColors();
    const label = button === 'left' ? 'LMB' : button === 'right' ? 'RMB' : 'MMB';
    const color = button === 'left' ? colors.left : button === 'right' ? colors.right : colors.middle;
    const mx = this.mouseBaseX + this.mouseOffsetX;
    const my = this.mouseBaseY + this.mouseOffsetY;
    const isCatFacing = this.keyboardOrientation === 'cat';
    const sideShift =
      button === 'left'
        ? isCatFacing
          ? 10
          : -10
        : button === 'right'
          ? isCatFacing
            ? -10
            : 10
          : 0;

    if (this.mouseBadges.length >= 2) {
      this.mouseBadges.shift();
    }
    this.mouseBadges.push({
      id: this.nextBubbleId++,
      label,
      color,
      x: Math.max(24, mx + sideShift),
      y: my - 30,
      createdAt: now,
      lifetime: 560,
    });
  }

  public onMouseDown(button: string): void {
    const now = performance.now();
    if (button === 'left') {
      this.mouseLeftDown = true;
      this.mouseLeftPress = 1;
      this.spawnMouseBadge('left', now);
    } else if (button === 'right') {
      this.mouseRightDown = true;
      this.mouseRightPress = 1;
      this.spawnMouseBadge('right', now);
    } else if (button === 'middle') {
      this.mouseMiddleDown = true;
      this.mouseMiddlePress = 1;
      this.spawnMouseBadge('middle', now);
    }

    this.lastMouseMoveAt = now;
    this.leftPaw.mode = 'mouse';
    this.leftPaw.impactSquash = 0.72;
    this.leftPaw.lastActiveAt = now;
  }

  public onMouseUp(button: string): void {
    if (button === 'left') this.mouseLeftDown = false;
    else if (button === 'right') this.mouseRightDown = false;
    else if (button === 'middle') this.mouseMiddleDown = false;
  }

  public onMouseWheel(): void {
    const now = performance.now();
    this.mouseScrollFlash = 1;
    this.lastMouseMoveAt = now;
    this.leftPaw.mode = 'mouse';
    this.leftPaw.lastActiveAt = now;
  }

  public onWindowCursor(relX: number, relY: number, overCat: boolean): void {
    this.cursorRelX = relX;
    this.cursorRelY = relY;
    const px = relX * this.logicalW;
    const py = relY * this.logicalH;
    const head = this.spinePoint(0.24, 0);
    const overPettingArea = overCat && ((px - head.x) / 53) ** 2 + ((py - head.y + 5) / 54) ** 2 <= 1;
    this.cursorOverCat = overPettingArea;

    if (overPettingArea && !this.mouseLeftDown && !this.mouseRightDown) {
      if (this.lastPetCursorX !== null && this.lastPetCursorY !== null) {
        const dist = Math.hypot(px - this.lastPetCursorX, py - this.lastPetCursorY);
        if (dist > 1.5 && dist < 80) {
          this.petEnergy = Math.min(1, this.petEnergy + dist * 0.011);
        }
      }
      this.lastPetCursorX = px;
      this.lastPetCursorY = py;
    } else {
      this.lastPetCursorX = null;
      this.lastPetCursorY = null;
    }
  }

  // ---- Keyboard projection ----

  private projectKeyboardPoint(uCol: number, rowPos: number): Vec2 {
    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const screenU = isCatFacing ? layout.totalUnits - uCol : uCol;
    const screenRow = isCatFacing ? layout.totalRows - rowPos : rowPos;

    const padX = 7;
    const padY = 6;
    const innerW = this.kbWidth - padX * 2;
    const innerH = this.kbHeight - padY * 2;
    const localX = -innerW / 2 + (screenU / layout.totalUnits) * innerW;
    const localY = -innerH / 2 + (screenRow / layout.totalRows) * innerH;

    const depthFactor = 0.94 + (screenRow / layout.totalRows) * 0.09;
    const px = localX * depthFactor;
    const py = localY;

    const cos = Math.cos(this.kbTiltRad);
    const sin = Math.sin(this.kbTiltRad);
    return {
      x: this.kbCenterX + px * cos - py * sin,
      y: this.kbCenterY + px * sin + py * cos,
    };
  }

  private getKeyWorldCenter(spec: KeySpec): Vec2 {
    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const uClamped = Math.max(0.65, Math.min(layout.totalUnits - 0.65, spec.uStart + spec.uWidth * 0.5));
    const rY = spec.rowY ?? spec.row;
    const rSpan = spec.rowSpan ?? 1;
    let rowPos = rY + rSpan * 0.5;
    if (isCatFacing && rY <= 0.1) {
      rowPos = rY + 0.68;
    } else if (!isCatFacing && rY + rSpan >= layout.totalRows - 0.1) {
      rowPos = rY + rSpan - 0.68;
    }
    return this.projectKeyboardPoint(uClamped, rowPos);
  }

  private clampToKeyboardSurface(pt: Vec2): Vec2 {
    // Transform into keyboard-local space and clamp so the plump paw pad NEVER extends past the keyboard bezel!
    const dx = pt.x - this.kbCenterX;
    const dy = pt.y - this.kbCenterY;
    const cos = Math.cos(-this.kbTiltRad);
    const sin = Math.sin(-this.kbTiltRad);
    const lx = dx * cos - dy * sin;
    const ly = dx * sin + dy * cos;

    const maxX = this.kbWidth * 0.5 - 15;
    const minY = -this.kbHeight * 0.5 + 8;
    const maxY = this.kbHeight * 0.5 - 14;
    const cx = Math.max(-maxX, Math.min(maxX, lx));
    const cy = Math.max(minY, Math.min(maxY, ly));

    const rcos = Math.cos(this.kbTiltRad);
    const rsin = Math.sin(this.kbTiltRad);
    return {
      x: this.kbCenterX + cx * rcos - cy * rsin,
      y: this.kbCenterY + cx * rsin + cy * rcos,
    };
  }

  private spawnBubble(label: string, isSpace: boolean, keyX: number, now: number): void {
    if (this.bubbles.length >= 6) {
      this.bubbles.shift();
    }
    const minX = this.keyboardLayout === '108' ? 145 : 125;
    const maxX = this.logicalW - 105;
    const baseX = Math.max(minX, Math.min(maxX, keyX * 0.45 + this.kbCenterX * 0.55));
    const staggerY = 258 + ((this.nextBubbleId % 3) - 1) * 10 + (Math.random() - 0.5) * 6;
    this.bubbles.push({
      id: this.nextBubbleId++,
      label,
      isSpace,
      x: baseX,
      targetX: baseX,
      y: 292,
      targetY: staggerY,
      rot: ((Math.random() - 0.5) * 4.5 * Math.PI) / 180,
      createdAt: now,
      lifetime: 940,
    });
    this.layoutBubbles();
  }

  private layoutBubbles(): void {
    const count = this.bubbles.length;
    if (count === 0) return;

    const widths = this.bubbles.map((b) => (b.isSpace ? 40 : Math.max(25, b.label.length * 7.5 + 14)));
    const gap = 3.5;
    const totalW = widths.reduce((a, b) => a + b, 0) + gap * (count - 1);
    const anchor = this.kbCenterX + 12;
    const leftBound = this.keyboardLayout === '108' ? 140 : 126;
    const rightBound = this.logicalW - 82;
    let cursor = Math.max(leftBound, Math.min(rightBound - totalW, anchor - totalW * 0.5));

    for (let i = 0; i < count; i++) {
      const w = widths[i];
      this.bubbles[i].targetX = cursor + w * 0.5;
      cursor += w + gap;
    }
  }

  // ---- Simulation update ----

  private update(dt: number, now: number): void {
    this.elapsed += dt;

    const mLerp = 1 - Math.exp(-28 * dt);
    this.mouseOffsetX += (this.mouseTargetOffsetX - this.mouseOffsetX) * mLerp;
    this.mouseOffsetY += (this.mouseTargetOffsetY - this.mouseOffsetY) * mLerp;
    if (!this.mouseLeftDown && this.mouseLeftPress > 0) {
      this.mouseLeftPress = Math.max(0, this.mouseLeftPress - dt * 5.8);
    }
    if (!this.mouseRightDown && this.mouseRightPress > 0) {
      this.mouseRightPress = Math.max(0, this.mouseRightPress - dt * 5.8);
    }
    if (!this.mouseMiddleDown && this.mouseMiddlePress > 0) {
      this.mouseMiddlePress = Math.max(0, this.mouseMiddlePress - dt * 5.8);
    }
    this.mouseScrollFlash = Math.max(0, this.mouseScrollFlash - dt * 4.5);
    this.mouseBadges = this.mouseBadges.filter((b) => now - b.createdAt < b.lifetime);

    while (this.recentKeyTimes.length > 0 && now - this.recentKeyTimes[0] > 950) {
      this.recentKeyTimes.shift();
    }
    const targetTypingEnergy = Math.min(1, this.recentKeyTimes.length / 7);
    this.typingEnergy += (targetTypingEnergy - this.typingEnergy) * (1 - Math.exp(-9 * dt));

    const isTypingRecently = now - this.lastAnyKeyAt < 650;
    const targetHunch = isTypingRecently ? 0.45 + this.typingEnergy * 0.55 : 0;
    this.typingHunch += (targetHunch - this.typingHunch) * (1 - Math.exp(-10 * dt));

    const rockAcc = -this.strikeRock * 140 - this.strikeRockVel * 14;
    this.strikeRockVel += rockAcc * dt;
    this.strikeRock = Math.max(-1, Math.min(1, this.strikeRock + this.strikeRockVel * dt));

    this.bodyBob = Math.max(0, this.bodyBob - dt * 11.5);
    this.kbBounce = Math.max(0, this.kbBounce - dt * 15);

    // Playful left-right body & head groove sway during pure typing (matches original video first half!)
    const pureTypingActive = isTypingRecently && this.leftPaw.mode !== 'mouse';
    if (pureTypingActive) {
      this.typingGroovePhase += dt * (4.8 + this.typingEnergy * 5.2);
    }
    const grooveWave = Math.sin(this.typingGroovePhase) * (0.58 + this.typingEnergy * 0.44);
    const targetTypingSway = pureTypingActive
      ? Math.max(-1.1, Math.min(1.1, grooveWave * 0.72 + this.lastKeyBias * 0.55))
      : this.leftPaw.mode === 'mouse' && isTypingRecently
        ? Math.sin(this.typingGroovePhase) * 0.16
        : 0;
    const swayAcc = (targetTypingSway - this.typingSway) * 118 - this.typingSwayVel * 12.5;
    this.typingSwayVel += swayAcc * dt;
    this.typingSway = Math.max(-1.2, Math.min(1.2, this.typingSway + this.typingSwayVel * dt));

    // Measured spine-shear lean: shifts head & chest cleanly forward-left toward mouse while hips stay anchored!
    let targetLean = 0;
    if (this.leftPaw.mode === 'mouse') {
      const mouseSweepBias = (this.mouseOffsetX / 18) * 0.22;
      targetLean = this.rightPaw.mode === 'poke'
        ? -0.52 + mouseSweepBias
        : -0.96 + mouseSweepBias;
    }
    this.spineLean += (targetLean - this.spineLean) * (1 - Math.exp(-10 * dt));

    const desiredHeadTilt =
      this.typingSway * 0.18 +
      this.typingHunch * (this.strikeRock * 0.065 - 0.02) +
      this.spineLean * 0.085;
    this.headTilt += (desiredHeadTilt - this.headTilt) * (1 - Math.exp(-12 * dt));

    this.petEnergy = Math.max(0, this.petEnergy - dt * (this.cursorOverCat ? 0.22 : 0.65));
    const wantSquint = this.cursorOverCat && this.petEnergy > 0.15 ? 1 : 0;
    const earResponse = wantSquint ? 12 : 6;
    this.squintAmount += (wantSquint - this.squintAmount) * (1 - Math.exp(-earResponse * dt));
    const wantPrivacy = this.privacyMode ? 1 : 0;
    this.privacyClose += (wantPrivacy - this.privacyClose) * (1 - Math.exp(-14 * dt));

    if (!this.isBlinking && this.elapsed >= this.nextBlinkAt) {
      this.isBlinking = true;
      this.blinkProgress = 0;
    }
    if (this.isBlinking) {
      this.blinkProgress += dt * 12;
      if (this.blinkProgress >= 2) {
        this.isBlinking = false;
        this.blinkProgress = 0;
        this.nextBlinkAt = this.elapsed + 2.8 + Math.random() * 3.6;
      }
    }

    // 100% live eye tracking toward cursor, OR cozy bottom-edge glance at keys during pure typing!
    const eyePos = this.spinePoint(0.24, 0);
    const eyeCenterRelX = eyePos.x / this.logicalW;
    const eyeCenterRelY = eyePos.y / this.logicalH;
    let dx = this.cursorRelX - eyeCenterRelX;
    let dy = this.cursorRelY - eyeCenterRelY;
    const dist = Math.hypot(dx, dy);
    if (dist > 0.001) {
      const mag = Math.min(1, dist * 1.9);
      dx = (dx / dist) * mag;
      dy = (dy / dist) * mag;
    }
    const mouseRecentlyMoved = now - this.lastMouseMoveAt < 2400;
    const pureTypingGlance = mouseRecentlyMoved ? 0 : this.typingHunch;
    const targetEyeX =
      dx * 7.0 * (1 - pureTypingGlance * 0.45) +
      pureTypingGlance * (this.typingSway * 3.8 + this.lastKeyBias * 2.2);
    const targetEyeY = dy * 5.2 * (1 - pureTypingGlance * 0.45) + pureTypingGlance * 5.8;
    const eyeLerp = 1 - Math.exp(-20 * dt);
    this.eyeLookX += (targetEyeX - this.eyeLookX) * eyeLerp;
    this.eyeLookY += (targetEyeY - this.eyeLookY) * eyeLerp;

    for (const anim of this.keyAnims.values()) {
      if (!anim.pressed && anim.pressAmount > 0) {
        anim.pressAmount = Math.max(0, anim.pressAmount - dt * 5.0);
      }
    }

    this.updatePaw(this.leftPaw, dt, now);
    this.updatePaw(this.rightPaw, dt, now);

    const beforeLen = this.bubbles.length;
    this.bubbles = this.bubbles.filter((b) => now - b.createdAt < b.lifetime);
    if (this.bubbles.length !== beforeLen) {
      this.layoutBubbles();
    }
    for (const b of this.bubbles) {
      const age = (now - b.createdAt) / b.lifetime;
      b.x += (b.targetX - b.x) * (1 - Math.exp(-18 * dt));
      const desiredY = b.targetY - age * 10;
      b.y += (desiredY - b.y) * (1 - Math.exp(-16 * dt));
    }
  }

  private updatePaw(paw: PawState, dt: number, now: number): void {
    const liveMouseX = this.mouseBaseX + this.mouseOffsetX;
    const liveMouseY = this.mouseBaseY + this.mouseOffsetY;

    if (paw.mode === 'mouse') {
      const isCatFacing = this.keyboardOrientation === 'cat';
      const lrBias = this.mouseLeftPress - this.mouseRightPress;
      const anyClickPress = Math.max(this.mouseLeftPress, this.mouseRightPress, this.mouseMiddlePress);
      if (isCatFacing) {
        // 180° cat-facing: palm heel at TOP, LMB = inner/+X, RMB = outer/-X
        paw.target = {
          x: liveMouseX + 1.2 + lrBias * 6.4,
          y: liveMouseY - 11.5 + anyClickPress * 3.6,
        };
      } else {
        // 0° viewer-facing: palm heel at BOTTOM, LMB = left/-X, RMB = right/+X
        paw.target = {
          x: liveMouseX + 0.5 - lrBias * 6.4,
          y: liveMouseY + 10.0 - anyClickPress * 3.4,
        };
      }
      const stillHolding =
        this.mouseLeftDown ||
        this.mouseRightDown ||
        this.mouseMiddleDown ||
        now - this.lastMouseMoveAt < 2200;
      if (!stillHolding) {
        paw.mode = 'idle';
      }
    } else if (paw.mode === 'poke') {
      paw.pokeTime += dt;
      if (paw.pokeTime > 0.13) {
        if (paw.side === 'left' && (this.mouseLeftDown || this.mouseRightDown || now - this.lastMouseMoveAt < 450)) {
          paw.mode = 'mouse';
        } else if (now - this.lastAnyKeyAt < 650) {
          paw.mode = 'curled';
        } else {
          paw.mode = 'idle';
        }
      }
    } else if (paw.mode === 'curled') {
      if (now - this.lastAnyKeyAt > 680) {
        paw.mode = 'idle';
      }
    }

    if (paw.mode === 'curled') {
      paw.target = this.getLiveCurledPawPos(paw, 0);
    } else if (paw.mode === 'idle') {
      const baseSp = this.spinePoint(1.0, 0);
      paw.target = { x: baseSp.x + (paw.side === 'left' ? -26 : 19), y: baseSp.y + 2 };
    }

    const wantIdle = paw.mode === 'idle' ? 1 : 0;
    const wantCurl = paw.mode === 'curled' ? 1 : 0;
    const blendRate = 1 - Math.exp(-20 * dt);
    paw.idleWeight += (wantIdle - paw.idleWeight) * blendRate;
    paw.curlWeight += (wantCurl - paw.curlWeight) * blendRate;

    // Unconditionally stable, framerate-independent tracking (ZERO Euler spring overshoot at any FPS!)
    const followRate = paw.mode === 'poke' ? 42 : paw.mode === 'mouse' ? 32 : 22;
    const follow = 1 - Math.exp(-followRate * dt);
    paw.pos.x += (paw.target.x - paw.pos.x) * follow;
    paw.pos.y += (paw.target.y - paw.pos.y) * follow;
    paw.vel.x = 0;
    paw.vel.y = 0;

    // Hard guarantee: while poking a key, paw tip stays strictly on top of the keyboard surface
    if (paw.mode === 'poke') {
      const clamped = this.clampToKeyboardSurface(paw.pos);
      if (clamped.y < paw.pos.y || Math.abs(clamped.x - paw.pos.x) > 0.01) {
        paw.pos.x = clamped.x;
        paw.pos.y = Math.min(paw.pos.y, clamped.y);
      }
    }

    paw.impactSquash = Math.max(0, paw.impactSquash - dt * 7.5);
  }

  // ---- High-fidelity true-scale rendering ----

  private render(): void {
    const ctx = this.ctx;
    const cw = this.canvas.width;
    const ch = this.canvas.height;
    ctx.clearRect(0, 0, cw, ch);

    ctx.save();
    if (this.embedded) {
      const baseFit = Math.min((cw * 0.86) / this.logicalW, (ch * 0.86) / this.logicalH);
      const effectiveMul = this.renderScale <= 1.0 ? this.renderScale : 1.0 + (this.renderScale - 1.0) * 0.30;
      const uniformScale = baseFit * effectiveMul;
      const drawW = this.logicalW * uniformScale;
      const drawH = this.logicalH * uniformScale;
      const offsetX = (cw - drawW) * 0.5;
      const offsetY = (ch - drawH) * 0.52;
      ctx.translate(offsetX, offsetY);
      ctx.scale(uniformScale, uniformScale);
    } else {
      const cssW = Math.max(1, window.innerWidth);
      const dpr = cw / cssW;
      const fitScale = Math.min(cw / this.logicalW, ch / this.logicalH);
      const uniformScale = Math.min(fitScale, this.renderScale * dpr);
      const drawW = this.logicalW * uniformScale;
      const drawH = this.logicalH * uniformScale;
      const offsetX = cw - drawW;
      const offsetY = ch - drawH;
      ctx.translate(offsetX, offsetY);
      ctx.scale(uniformScale, uniformScale);
    }

    const breath = Math.sin(this.elapsed * 2.1) * 1.0;
    this.drawTail(ctx);

    // Layer 2: Mini mouse & 65% mechanical keyboard
    this.drawMiniMouse(ctx);
    this.drawKeyboard(ctx);

    // Layer 3: Exact measured spine-deformable cat silhouette (ears, cheeks, neck, pear belly, grounded hind feet, eyes & tiny whiskers)
    this.drawBlackDog(ctx, breath);

    // Moving front paws replace their resting artwork.
    this.drawPaw(ctx, this.leftPaw, breath);
    this.drawPaw(ctx, this.rightPaw, breath);

    // Layer 5: Organic bouncing 3D keycap bubbles & crisp mouse click badges
    if (this.showKeyBubbles && !this.privacyMode) {
      this.drawKeyBubbles(ctx);
      this.drawMouseBadges(ctx);
    }

    ctx.restore();
  }

  private drawTail(ctx: CanvasRenderingContext2D): void {
    const wag = Math.sin(this.elapsed * 4.2) * (0.045 + this.typingEnergy * 0.10);
    const seat = this.spinePoint(1, 0);
    const hip = this.spinePoint(0.80, 0);
    this.dogArtwork.drawTail(ctx, { x: seat.x + (hip.x - seat.x) * 1.6, y: seat.y }, wag + this.strikeRock * 0.06);
  }

  private drawBlackDog(ctx: CanvasRenderingContext2D, breath: number): void {
    const blink = this.isBlinking ? 1 - Math.abs(this.blinkProgress - 1) : 0;
    this.dogArtwork.draw(
      ctx,
      this.spinePoint(1, breath),
      this.spinePoint(0.24, breath),
      this.headTilt,
      this.squintAmount,
      Math.min(1, Math.max(blink, this.privacyClose, this.squintAmount * 0.44)),
      { x: Math.max(-1, Math.min(1, this.eyeLookX / 6)), y: Math.max(-1, Math.min(1, this.eyeLookY / 4.8)) },
      this.isPawAtRest(this.leftPaw),
      this.isPawAtRest(this.rightPaw),
    );
  }

  private drawMiniMouse(ctx: CanvasRenderingContext2D): void {
    const mx = this.mouseBaseX + this.mouseOffsetX;
    const my = this.mouseBaseY + this.mouseOffsetY;
    const rx = 18.5;
    const ry = 23.0;
    const lPress = this.mouseLeftPress;
    const rPress = this.mouseRightPress;
    const mPress = Math.max(this.mouseMiddlePress, this.mouseScrollFlash);
    const colors = this.getMouseButtonColors();

    ctx.save();
    ctx.translate(mx, my);
    ctx.rotate(-0.06);

    // Soft 3D base rim / drop shadow
    ctx.save();
    ctx.shadowColor = 'rgba(0, 0, 0, 0.28)';
    ctx.shadowBlur = 7;
    ctx.shadowOffsetY = 3.5;
    ctx.fillStyle = '#c8cac4';
    ctx.beginPath();
    ctx.ellipse(0, 1.8, rx, ry, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    // Main mouse palm shell (rear/palm at TOP closest to cat; split L/R buttons at BOTTOM pointing away from cat)
    const palmGrad = ctx.createLinearGradient(0, -ry, 0, 2);
    palmGrad.addColorStop(0.0, '#ecece7');
    palmGrad.addColorStop(1.0, '#f4f4f0');
    ctx.fillStyle = palmGrad;
    ctx.beginPath();
    ctx.ellipse(0, 0, rx, ry, 0, 0, Math.PI * 2);
    ctx.fill();

    // Clip inside mouse silhouette to render strictly separated Left / Right front button plates!
    const isCatFacing = this.keyboardOrientation === 'cat';
    ctx.save();
    ctx.beginPath();
    ctx.ellipse(0, 0, rx - 0.5, ry - 0.5, 0, 0, Math.PI * 2);
    ctx.clip();

    // Orientation-aware button/wheel geometry:
    // - 'cat' (180°): palm at TOP, buttons at BOTTOM, L = +X (inner), R = -X (outer)
    // - 'viewer' (0°): buttons at TOP, palm at BOTTOM, L = -X (left), R = +X (right)
    ctx.save();
    if (!isCatFacing) {
      ctx.scale(-1, -1);
    }

    const traceSeam = () => {
      ctx.moveTo(-rx - 2, -0.8);
      ctx.quadraticCurveTo(0, 1.8, rx + 2, -0.8);
    };

    // 1) Left Mouse Button plate (+X half in local coords -> inner-bottom in cat mode, top-left in viewer mode)
    ctx.beginPath();
    ctx.moveTo(0, 0.5);
    ctx.quadraticCurveTo(rx * 0.52, 0.4, rx + 2, -0.8);
    ctx.lineTo(rx + 2, ry + 2);
    ctx.lineTo(0, ry + 2);
    ctx.closePath();
    ctx.fillStyle = lPress > 0.08 ? colors.left : '#fafaf7';
    ctx.fill();
    if (lPress > 0.08) {
      ctx.fillStyle = `rgba(18, 22, 28, ${(0.18 * lPress).toFixed(3)})`;
      ctx.fillRect(0, -1, rx + 2, 3.8);
      ctx.fillRect(0, -1, 2.2, ry + 2);
    }

    // 2) Right Mouse Button plate (-X half in local coords -> outer-bottom in cat mode, top-right in viewer mode)
    ctx.beginPath();
    ctx.moveTo(-rx - 2, -0.8);
    ctx.quadraticCurveTo(-rx * 0.52, 0.4, 0, 0.5);
    ctx.lineTo(0, ry + 2);
    ctx.lineTo(-rx - 2, ry + 2);
    ctx.closePath();
    ctx.fillStyle = rPress > 0.08 ? colors.right : '#fafaf7';
    ctx.fill();
    if (rPress > 0.08) {
      ctx.fillStyle = `rgba(18, 22, 28, ${(0.18 * rPress).toFixed(3)})`;
      ctx.fillRect(-rx - 2, -1, rx + 2, 3.8);
      ctx.fillRect(-2.2, -1, 2.2, ry + 2);
    }

    // Crisp horizontal palm-to-button seam & vertical center L/R split line
    ctx.strokeStyle = 'rgba(42, 45, 52, 0.56)';
    ctx.lineWidth = 1.25;
    ctx.beginPath();
    traceSeam();
    ctx.stroke();

    ctx.strokeStyle = 'rgba(42, 45, 52, 0.66)';
    ctx.lineWidth = 1.3;
    ctx.beginPath();
    ctx.moveTo(0, 0.6);
    ctx.lineTo(0, ry + 1);
    ctx.stroke();

    // Recessed scroll wheel housing + interactive wheel (middle click & scroll glow)
    ctx.fillStyle = '#2d2f36';
    this.roundRect(ctx, -2.5, 4.2, 5.0, 9.4, 2.5);
    ctx.fill();

    ctx.fillStyle = mPress > 0.12 ? colors.middle : '#6b6f78';
    this.roundRect(ctx, -1.7, 5.0 + this.mouseMiddlePress * 0.8, 3.4, 7.6, 1.7);
    ctx.fill();

    ctx.strokeStyle = 'rgba(255, 255, 255, 0.45)';
    ctx.lineWidth = 0.8;
    ctx.beginPath();
    ctx.moveTo(-1.0, 8.8 + this.mouseMiddlePress * 0.8);
    ctx.lineTo(1.0, 8.8 + this.mouseMiddlePress * 0.8);
    ctx.stroke();

    ctx.restore(); // end local orientation transform

    // Always draw upright crisp L / R button legends in unobstructed front tips!
    ctx.font = '800 7.6px "Segoe UI", system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    const lx = isCatFacing ? 7.4 : -7.4;
    const rxLbl = isCatFacing ? -7.4 : 7.4;
    const lyBase = isCatFacing ? 14.0 : -14.0;
    const pressDir = isCatFacing ? 0.9 : -0.9;
    ctx.fillStyle = lPress > 0.12 ? '#14161a' : 'rgba(72, 76, 86, 0.56)';
    ctx.fillText('L', lx, lyBase + lPress * pressDir);
    ctx.fillStyle = rPress > 0.12 ? '#ffffff' : 'rgba(72, 76, 86, 0.56)';
    ctx.fillText('R', rxLbl, lyBase + rPress * pressDir);

    ctx.restore(); // end mouse clip

    // Crisp outer silhouette stroke
    ctx.strokeStyle = '#2b2d34';
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.ellipse(0, 0, rx, ry, 0, 0, Math.PI * 2);
    ctx.stroke();

    ctx.restore();
  }

  private drawKeyboard(ctx: CanvasRenderingContext2D): void {
    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const is108 = this.keyboardLayout === '108';
    const padX = 7;
    const padY = 6;
    const innerW = this.kbWidth - padX * 2;
    const innerH = this.kbHeight - padY * 2;

    ctx.save();
    ctx.translate(this.kbCenterX, this.kbCenterY + this.kbBounce * 1.1);
    ctx.rotate(this.kbTiltRad);

    // True 3D desk trapezoid perspective: far edge (top) is narrower, near edge (bottom) is wider,
    // cleanly re-projected for BOTH cat-facing (Spacebar at narrow top) and viewer-facing (Esc/F-row at narrow top)!
    const topHalfW = (this.kbWidth * 0.955) / 2;
    const botHalfW = (this.kbWidth * 1.035) / 2;
    const halfH = this.kbHeight / 2;

    ctx.save();
    ctx.shadowColor = 'rgba(0, 0, 0, 0.34)';
    ctx.shadowBlur = 11;
    ctx.shadowOffsetY = 5;
    ctx.fillStyle = '#191a1e';
    this.trapezoidPath(ctx, topHalfW, botHalfW, -halfH + 3.5, halfH + 5.5, 6.5);
    ctx.fill();
    ctx.restore();

    ctx.fillStyle = '#24262c';
    ctx.strokeStyle = '#363840';
    ctx.lineWidth = 1.1;
    this.trapezoidPath(ctx, topHalfW, botHalfW, -halfH, halfH, 6.5);
    ctx.fill();
    ctx.stroke();

    // Bright clean light-grey inner keybed matching ORIG 0.3s bright airy white keyboard!
    ctx.fillStyle = '#d4d6d1';
    this.trapezoidPath(ctx, topHalfW - 5, botHalfW - 5, -halfH + 4, halfH - 4, 3.5);
    ctx.fill();

    const rowH = innerH / layout.totalRows;
    const unitW = innerW / layout.totalUnits;
    const gap = is108 ? 0.85 : 0.9;

    for (const key of layout.keys) {
      const anim = this.keyAnims.get(key.id);
      const press = anim ? anim.pressAmount : 0;

      const rY = key.rowY ?? key.row;
      const rSpan = key.rowSpan ?? 1;
      const screenRowTop = isCatFacing ? layout.totalRows - (rY + rSpan) : rY;
      const screenUStart = isCatFacing
        ? layout.totalUnits - (key.uStart + key.uWidth)
        : key.uStart;

      const rowCenterFrac = (screenRowTop + rSpan * 0.5) / layout.totalRows;
      const depthScale = 0.94 + rowCenterFrac * 0.09;

      const rawX = -innerW / 2 + screenUStart * unitW + gap * 0.5;
      const rawW = key.uWidth * unitW - gap;
      const x = rawX * depthScale;
      const w = rawW * depthScale;
      const y = -innerH / 2 + screenRowTop * rowH + gap * 0.5 + press * 2.0;
      const h = rSpan * rowH - gap - 0.8;

      let topColor = '#f8f8f5';
      let sideColor = '#d0d2cc';
      if (press > 0.14) {
        topColor = '#d2f244';
        sideColor = '#8cb014';
      } else if (key.accent === 'orange') {
        topColor = '#f4a524';
        sideColor = '#c27a10';
      } else if (key.accent === 'mod') {
        topColor = '#efefeb';
        sideColor = '#c7c9c3';
      }

      ctx.fillStyle = sideColor;
      this.roundRect(ctx, x, y + 1.4, w, h, 1.8);
      ctx.fill();

      ctx.fillStyle = topColor;
      this.roundRect(ctx, x, y, w, h - 0.9, 1.8);
      ctx.fill();

      // Keycap legends: rotated 180° in cat-facing mode, upright 0° in viewer-facing mode!
      const legend = key.id === 'space' ? '' : key.id === 'esc' ? 'Esc' : key.label;
      if (legend) {
        const isWideWord = legend.length >= 3;
        const fontSize = is108
          ? isWideWord
            ? 5.0
            : legend.length === 2
              ? 5.8
              : 7.2
          : isWideWord
            ? 5.6
            : legend.length === 2
              ? 6.4
              : 7.8;
        ctx.save();
        ctx.translate(x + w * 0.5, y + (h - 0.9) * 0.50);
        if (isCatFacing) {
          ctx.rotate(Math.PI);
        }
        ctx.font = `700 ${fontSize}px "Segoe UI", system-ui, sans-serif`;
        ctx.fillStyle = press > 0.14
          ? '#111214'
          : key.accent === 'orange'
            ? '#ffffff'
            : '#2c2f36';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(legend, 0, 0);
        ctx.restore();
      }
    }

    if (this.privacyClose > 0.08) {
      ctx.save();
      ctx.globalAlpha = this.privacyClose;
      ctx.fillStyle = 'rgba(15, 20, 32, 0.74)';
      this.trapezoidPath(ctx, topHalfW - 2, botHalfW - 2, -halfH + 1.5, halfH - 1.5, 5.5);
      ctx.fill();
      ctx.strokeStyle = 'rgba(244, 114, 182, 0.45)';
      ctx.lineWidth = 1.4;
      ctx.stroke();
      ctx.font = '700 11.5px "Segoe UI", "PingFang SC", sans-serif';
      ctx.fillStyle = '#fbcfe8';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      const dots = performance.now() - this.privacyTypingAt < 350 ? ' • • •' : '';
      const label = '🔒 密码输入中 · 已隐藏按键';
      ctx.fillText(label + dots, 0, 0);
      ctx.restore();
    }

    ctx.restore();
  }

  /**
   * True-scale plush velvety cat arm & chubby rounded paw ball (1:1 match to reference photo & 8.3s / 13.2s / 31.0s!)
   */
  private drawMovingPaw(
    ctx: CanvasRenderingContext2D,
    side: 'left' | 'right',
    sx: number,
    sy: number,
    px: number,
    py: number,
    squash: number,
    curled: number,
    withCrease: boolean,
  ): void {
    const dx = px - sx;
    const dy = py - sy;
    const len = Math.max(1, Math.hypot(dx, dy));
    const ux = dx / len;
    const uy = dy / len;
    const nx = -uy;
    const ny = ux;

    const wristDist = Math.max(6, len - 8);
    const wx = sx + ux * wristDist;
    const wy = sy + uy * wristDist;

    // True-scale plush proportions for 248px tall cat!
    const reachingForMouse = !withCrease;
    const rShoulder = reachingForMouse ? 18 : 14.2;
    const rWrist = reachingForMouse ? 14.2 : 9.4;
    const pawRx = (reachingForMouse ? 17 : 14.5) * (1 + squash * 0.14);
    const pawRy = (reachingForMouse ? 13.2 : 11.8) * (1 - squash * 0.12);

    ctx.save();
    const elbowFlex = Math.min(len * (0.14 + curled * 0.22), 22) * (side === 'left' ? 1 : -1);
    const elbowX = (sx + wx) * 0.5 + nx * elbowFlex;
    const elbowY = (sy + wy) * 0.5 + ny * elbowFlex;
    ctx.shadowColor = 'rgba(18, 17, 18, 0.42)';
    ctx.shadowBlur = 1.6;
    ctx.fillStyle = DOG_FUR;

    // Arm silhouette tapering gently from shoulder to wrist
    ctx.beginPath();
    ctx.moveTo(sx + nx * rShoulder, sy + ny * rShoulder);
    ctx.quadraticCurveTo(
      elbowX + nx * (rShoulder * 0.84),
      elbowY + ny * (rShoulder * 0.84),
      wx + nx * rWrist,
      wy + ny * rWrist,
    );
    ctx.lineTo(wx - nx * rWrist, wy - ny * rWrist);
    ctx.quadraticCurveTo(
      elbowX - nx * (rShoulder * 0.84),
      elbowY - ny * (rShoulder * 0.84),
      sx - nx * rShoulder,
      sy - ny * rShoulder,
    );
    ctx.closePath();
    ctx.fill();

    // Seamless shoulder merge
    ctx.beginPath();
    ctx.arc(sx, sy, rShoulder, 0, Math.PI * 2);
    ctx.fill();

    // The pad keeps the original strike squash and contact point.
    ctx.beginPath();
    ctx.ellipse(px, py, pawRx, pawRy, 0, 0, Math.PI * 2);
    ctx.fill();

    ctx.shadowBlur = 0;

    ctx.strokeStyle = 'rgba(255,255,255,0.75)';
    ctx.lineWidth = 0.9;
    ctx.beginPath();
    ctx.ellipse(px, py, pawRx, pawRy, 0, Math.PI * 0.97, Math.PI * 1.76);
    ctx.stroke();

    // Clean crisp subtle light-grey side separation lines ONLY where the arm crosses the black body (exact 2.2s / 8.3s / 13.2s!)
    if (withCrease && len > 28) {
      const startFrac = 0.18;
      const endFrac = 0.72;
      for (const sgn of [-1, 1]) {
        const csx = sx + ux * (len * startFrac) + nx * sgn * (rShoulder * 0.86);
        const csy = sy + uy * (len * startFrac) + ny * sgn * (rShoulder * 0.86);
        const cex = sx + ux * (len * endFrac) + nx * sgn * (rWrist * 1.02);
        const cey = sy + uy * (len * endFrac) + ny * sgn * (rWrist * 1.02);

        const creaseGrad = ctx.createLinearGradient(csx, csy, cex, cey);
        creaseGrad.addColorStop(0.0, 'rgba(216, 218, 222, 0.0)');
        creaseGrad.addColorStop(0.22, 'rgba(216, 218, 222, 0.65)');
        creaseGrad.addColorStop(0.84, 'rgba(216, 218, 222, 0.56)');
        creaseGrad.addColorStop(1.0, 'rgba(216, 218, 222, 0.0)');

        ctx.strokeStyle = creaseGrad;
        ctx.lineWidth = 1.3;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(csx, csy);
        ctx.quadraticCurveTo(elbowX + nx * sgn * 11, elbowY + ny * sgn * 11, cex, cey);
        ctx.stroke();
      }
    }

    ctx.restore();
  }

  private isPawAtRest(paw: PawState): boolean {
    return paw.mode === 'idle' && paw.idleWeight > 0.96;
  }

  private drawPaw(ctx: CanvasRenderingContext2D, paw: PawState, breath: number): void {
    if (this.isPawAtRest(paw)) return;
    const shoulder = this.getLiveShoulder(paw, breath);
    const clickDrop = paw.mode === 'mouse'
      ? Math.max(this.mouseLeftPress, this.mouseRightPress, this.mouseMiddlePress) * 1.8
      : 0;
    this.drawMovingPaw(
      ctx, paw.side, shoulder.x, shoulder.y, paw.pos.x, paw.pos.y + clickDrop,
      paw.impactSquash, paw.curlWeight, paw.mode !== 'mouse',
    );
  }

  private drawKeyBubbles(ctx: CanvasRenderingContext2D): void {
    const now = performance.now();

    for (const b of this.bubbles) {
      const age = now - b.createdAt;
      const t = Math.min(1, age / b.lifetime);

      let scale = 1;
      if (t < 0.11) {
        const p = t / 0.11;
        scale = 0.62 + Math.sin(p * Math.PI * 0.65) * 0.44;
      } else if (t > 0.76) {
        scale = 1 - ((t - 0.76) / 0.24) * 0.1;
      }
      const alpha = t < 0.72 ? 1 : Math.max(0, 1 - (t - 0.72) / 0.28);

      ctx.save();
      ctx.globalAlpha = alpha;
      ctx.translate(b.x, b.y);
      ctx.rotate(b.rot);
      ctx.scale(scale, scale);

      ctx.font = '700 12px "Segoe UI", system-ui, sans-serif';
      const textW = ctx.measureText(b.label).width;
      const boxW = b.isSpace ? 40 : Math.max(24, textW + 13);
      const boxH = 24;
      const bx = -boxW / 2;
      const by = -boxH / 2;

      ctx.shadowColor = 'rgba(0, 0, 0, 0.22)';
      ctx.shadowBlur = 5;
      ctx.shadowOffsetY = 2;

      ctx.fillStyle = '#c8cac6';
      this.roundRect(ctx, bx, by + 2.2, boxW, boxH, 5.5);
      ctx.fill();

      ctx.shadowBlur = 0;
      ctx.fillStyle = '#ffffff';
      ctx.strokeStyle = 'rgba(38, 40, 46, 0.42)';
      ctx.lineWidth = 1.05;
      this.roundRect(ctx, bx, by, boxW, boxH - 1.3, 5.5);
      ctx.fill();
      ctx.stroke();

      if (b.isSpace) {
        ctx.strokeStyle = '#222429';
        ctx.lineWidth = 1.55;
        ctx.lineCap = 'round';
        ctx.lineJoin = 'round';
        ctx.beginPath();
        ctx.moveTo(-9, -1);
        ctx.lineTo(-9, 2.0);
        ctx.lineTo(9, 2.0);
        ctx.lineTo(9, -1);
        ctx.stroke();
      } else {
        ctx.fillStyle = '#1e2025';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(b.label, 0, -0.5);
      }

      ctx.restore();
    }
  }

  private drawMouseBadges(ctx: CanvasRenderingContext2D): void {
    const now = performance.now();
    for (let i = 0; i < this.mouseBadges.length; i++) {
      const b = this.mouseBadges[i];
      const t = Math.min(1, (now - b.createdAt) / b.lifetime);
      let scale = 1;
      if (t < 0.14) {
        scale = 0.65 + Math.sin((t / 0.14) * Math.PI * 0.65) * 0.42;
      } else if (t > 0.72) {
        scale = 1 - ((t - 0.72) / 0.28) * 0.12;
      }
      const alpha = t < 0.68 ? 1 : Math.max(0, 1 - (t - 0.68) / 0.32);
      const liftY = t * 14 + (this.mouseBadges.length - 1 - i) * 18;

      ctx.save();
      ctx.globalAlpha = alpha;
      ctx.translate(b.x, b.y - liftY);
      ctx.scale(scale, scale);

      const boxW = 34;
      const boxH = 19;
      const bx = -boxW / 2;
      const by = -boxH / 2;

      ctx.shadowColor = 'rgba(0, 0, 0, 0.24)';
      ctx.shadowBlur = 4;
      ctx.shadowOffsetY = 1.5;

      ctx.fillStyle = '#22242a';
      this.roundRect(ctx, bx, by + 1.6, boxW, boxH, 5);
      ctx.fill();

      ctx.shadowBlur = 0;
      ctx.fillStyle = b.color;
      ctx.strokeStyle = 'rgba(20, 22, 28, 0.45)';
      ctx.lineWidth = 1.0;
      this.roundRect(ctx, bx, by, boxW, boxH - 1.0, 5);
      ctx.fill();
      ctx.stroke();

      ctx.font = '800 9.5px "Segoe UI", system-ui, sans-serif';
      ctx.fillStyle = b.label === 'RMB' && b.color === '#ff7b3b' ? '#ffffff' : '#14161a';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(b.label, 0, -0.3);

      ctx.restore();
    }
  }

  private roundRect(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
  ): void {
    const rad = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + rad, y);
    ctx.arcTo(x + w, y, x + w, y + h, rad);
    ctx.arcTo(x + w, y + h, x, y + h, rad);
    ctx.arcTo(x, y + h, x, y, rad);
    ctx.arcTo(x, y, x + w, y, rad);
    ctx.closePath();
  }

  private trapezoidPath(
    ctx: CanvasRenderingContext2D,
    topHalfW: number,
    botHalfW: number,
    topY: number,
    botY: number,
    r: number,
  ): void {
    ctx.beginPath();
    ctx.moveTo(-topHalfW + r, topY);
    ctx.lineTo(topHalfW - r, topY);
    ctx.quadraticCurveTo(topHalfW, topY, topHalfW + 1, topY + r);
    ctx.lineTo(botHalfW, botY - r);
    ctx.quadraticCurveTo(botHalfW, botY, botHalfW - r, botY);
    ctx.lineTo(-botHalfW + r, botY);
    ctx.quadraticCurveTo(-botHalfW, botY, -botHalfW, botY - r);
    ctx.lineTo(-topHalfW - 1, topY + r);
    ctx.quadraticCurveTo(-topHalfW, topY, -topHalfW + r, topY);
    ctx.closePath();
  }
}
