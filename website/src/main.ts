import { KuroCatEngine } from '../../src/catEngine';
import { keyboardEventToVk } from './webKeyMap';

const canvas = document.querySelector<HTMLCanvasElement>('#pet-preview')!;
const engine = new KuroCatEngine(canvas, { embedded: true });
const buttons = ['left', 'middle', 'right'];
const pressed = new Set<number>();

engine.start();
window.addEventListener('keydown', (event) => {
  const key = keyboardEventToVk(event);
  if (key === null) return;
  event.preventDefault();
  pressed.add(key);
  engine.onKeyDown(key, event.repeat);
});
window.addEventListener('keyup', (event) => {
  const key = keyboardEventToVk(event);
  if (key === null) return;
  pressed.delete(key);
  engine.onKeyUp(key);
});
canvas.addEventListener('pointermove', (event) => {
  const rect = canvas.getBoundingClientRect();
  const x = event.clientX - rect.left;
  const y = event.clientY - rect.top;
  const { w, h } = engine.getLogicalSize();
  const scale = Math.min(rect.width * 0.86 / w, rect.height * 0.86 / h);
  const px = (x - (rect.width - w * scale) * 0.5) / (w * scale);
  const py = (y - (rect.height - h * scale) * 0.52) / (h * scale);
  const overHead = px >= 0.30 && px <= 0.65 && py >= 0.13 && py <= 0.49;
  if (!overHead) engine.onGlobalMouseMove(x / rect.width, y / rect.height);
  engine.onWindowCursor(px, py, overHead);
});
canvas.addEventListener('pointerleave', () => engine.onWindowCursor(0.5, 0.5, false));
canvas.addEventListener('pointerdown', (event) => {
  const button = buttons[event.button];
  if (button) engine.onMouseDown(button);
});
window.addEventListener('pointerup', (event) => {
  const button = buttons[event.button];
  if (button) engine.onMouseUp(button);
});
canvas.addEventListener('contextmenu', (event) => event.preventDefault());
canvas.addEventListener('wheel', () => engine.onMouseWheel(), { passive: true });
window.addEventListener('blur', () => {
  for (const key of pressed) engine.onKeyUp(key);
  pressed.clear();
  for (const button of buttons) engine.onMouseUp(button);
});
