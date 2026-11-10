// @vitest-environment happy-dom
// @vitest-environment-options {"settings":{"enableIframePageLoading":true}}
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { initFullscreen, initHeroDemo, type WideQuery } from './demo-frame.ts';

const ORIGIN = 'https://site.example';

function hero(): HTMLElement {
  document.body.innerHTML = '<div class="hero-demo" data-state="still"><div class="window"><div class="window-in"><div class="chrome"><span class="demo-badge" role="status">Screenshot</span></div><div class="stage"><img class="still" alt="" /><button data-action="load" hidden>Load Demo</button></div></div></div><button data-action="reset" disabled>Reset demo</button><button data-action="fullscreen" disabled>Full screen</button></div>';
  return document.querySelector<HTMLElement>('.hero-demo')!;
}
function wide(matches: boolean) {
  const listeners: Array<() => void> = [];
  const q: WideQuery & { set(m: boolean): void } = {
    matches,
    addEventListener: (_type, l) => { listeners.push(l); },
    set(m) { (q as { matches: boolean }).matches = m; for (const l of listeners) l(); },
  };
  return q;
}
const load = () => document.querySelector<HTMLButtonElement>('[data-action="load"]')!;
function start(root: HTMLElement, options: Parameters<typeof initHeroDemo>[1] = {}) {
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true), ...options });
  load().click();
  return demo;
}
const report = (frame: HTMLIFrameElement | null, data: unknown, origin = ORIGIN, source: unknown = frame?.contentWindow) =>
  window.dispatchEvent(new MessageEvent('message', { data, origin, source: source as Window }));
const badge = () => document.querySelector('.demo-badge')!.textContent;

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

test('a narrow window keeps the still: no frame, nothing loaded', () => {
  const root = hero();
  const demo = start(root, { wide: wide(false) });
  expect(demo.state).toBe('still');
  expect(root.querySelector('iframe')).toBeNull();
  expect(badge()).toBe('Screenshot');
  expect(load().hidden).toBe(false);
  expect(load().disabled).toBe(true);
});

test('a wide window stays a screenshot through idle time until Load Demo is clicked', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  vi.advanceTimersByTime(120_000);
  expect(demo.state).toBe('still');
  expect(demo.frame).toBeNull();
  expect(load().hidden).toBe(false);
  expect(document.querySelector<HTMLButtonElement>('[data-action="reset"]')!.disabled).toBe(true);
  expect(document.querySelector<HTMLButtonElement>('[data-action="fullscreen"]')!.disabled).toBe(true);
  load().click();
  const frame = root.querySelector<HTMLIFrameElement>('.stage > iframe.demo-live')!;
  expect(frame.title).toBe('GitNexia live demo');
  expect(demo.frame).toBe(frame);
  expect(demo.state).toBe('loading');
  expect(badge()).toBe('Loading the live demo…');
  expect(load().hidden).toBe(true);
  load().click();
  expect(demo.frame).toBe(frame);
});

test('"ready" from the frame shows the demo', () => {
  const root = hero();
  const demo = start(root);
  report(demo.frame, { source: 'gitnexia-demo', type: 'ready' });
  expect(demo.state).toBe('ready');
  expect(badge()).toBe('Live demo');
  expect(load().hidden).toBe(true);
  expect(document.querySelector<HTMLButtonElement>('[data-action="reset"]')!.disabled).toBe(false);
  expect(document.querySelector<HTMLButtonElement>('[data-action="fullscreen"]')!.disabled).toBe(false);
});

test('a message from another window or origin, or of another shape, changes nothing', () => {
  const root = hero();
  const demo = start(root);
  report(demo.frame, { source: 'gitnexia-demo', type: 'ready' }, 'https://elsewhere.example');
  report(demo.frame, { source: 'gitnexia-demo', type: 'ready' }, ORIGIN, window);
  report(demo.frame, { type: 'ready' });
  report(demo.frame, 'ready');
  expect(demo.state).toBe('loading');
});

test("a failed report keeps the still and says why", () => {
  const root = hero();
  const demo = start(root);
  report(demo.frame, { source: 'gitnexia-demo', type: 'failed', message: "The demo's data is missing: orbital.gbdemo (404)" });
  expect(demo.state).toBe('failed');
  expect(badge()).toBe("The live demo didn't start: The demo's data is missing: orbital.gbdemo (404)");
  expect(load().hidden).toBe(false);
  const old = demo.frame;
  load().click();
  expect(demo.state).toBe('loading');
  expect(demo.frame).not.toBe(old);
  expect(root.querySelectorAll('iframe')).toHaveLength(1);
});

test('no report in time: failed, not an endless "Loading"', () => {
  const root = hero();
  const demo = start(root, { timeoutMs: 1000 });
  vi.advanceTimersByTime(999);
  expect(demo.state).toBe('loading');
  vi.advanceTimersByTime(1);
  expect(demo.state).toBe('failed');
  expect(badge()).toBe("The live demo didn't start: it took too long to load");
});

test('resizing never starts the demo; an explicit click starts it once when wide', () => {
  const root = hero();
  const q = wide(false);
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: q });
  q.set(true);
  expect(load().disabled).toBe(false);
  q.set(false);
  expect(load().disabled).toBe(true);
  q.set(true);
  vi.advanceTimersByTime(120_000);
  expect(root.querySelector('iframe')).toBeNull();
  expect(demo.state).toBe('still');
  load().click();
  q.set(false);
  q.set(true);
  expect(root.querySelectorAll('iframe')).toHaveLength(1);
  expect(demo.state).toBe('loading');
});

test('reset replaces the frame with a fresh one, loading again; the old one\'s word no longer counts', () => {
  const root = hero();
  const demo = start(root);
  const old = demo.frame;
  const oldWindow = old!.contentWindow;
  expect(oldWindow).not.toBeNull();
  report(old, { source: 'gitnexia-demo', type: 'ready' });
  demo.reset();
  expect(root.querySelectorAll('iframe')).toHaveLength(1);
  expect(demo.frame).not.toBe(old);
  expect(demo.state).toBe('loading');
  report(old, { source: 'gitnexia-demo', type: 'ready' }, ORIGIN, oldWindow);
  expect(demo.state).toBe('loading');
});

test('full screen: hidden where the browser has none; otherwise the window goes full screen', async () => {
  const button = document.createElement('button');
  initFullscreen(button, document.createElement('div'), { fullscreenEnabled: false } as Document, () => null);
  expect(button.hidden).toBe(true);

  const target = document.createElement('div');
  const requestFullscreen = vi.fn(() => Promise.resolve());
  target.requestFullscreen = requestFullscreen;
  const shown = document.createElement('button');
  initFullscreen(shown, target, { fullscreenEnabled: true, fullscreenElement: null, exitFullscreen: vi.fn(), addEventListener: vi.fn() } as unknown as Document, () => null);
  shown.click();
  expect(requestFullscreen).toHaveBeenCalled();
  expect(shown.hidden).toBe(false);
});

test('the badge says Screenshot while the window is too narrow for the demo, and Live demo again once wide', () => {
  const root = hero();
  const q = wide(true);
  const demo = start(root, { wide: q });
  report(demo.frame, { source: 'gitnexia-demo', type: 'ready' });
  expect(badge()).toBe('Live demo');
  q.set(false);
  expect(badge()).toBe('Screenshot');
  q.set(true);
  expect(badge()).toBe('Live demo');
});
