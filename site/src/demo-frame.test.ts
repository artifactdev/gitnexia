// @vitest-environment happy-dom
// @vitest-environment-options {"settings":{"enableIframePageLoading":true}}
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { initFullscreen, initHeroDemo, type WideQuery } from './demo-frame.ts';

const ORIGIN = 'https://site.example';

function hero(): HTMLElement {
  document.body.innerHTML = '<div class="hero-demo" data-state="still"><div class="window"><div class="window-in"><div class="chrome"><span class="demo-badge" role="status">Screenshot</span></div><div class="stage"><img class="still" alt="" /><button type="button" class="btn demo-show" data-action="show" hidden><span class="btn-stack"><span class="demo-show-idle">Show demo</span><span class="demo-show-retry">Try again</span></span></button></div></div></div></div>';
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
const report = (frame: HTMLIFrameElement | null, data: unknown, origin = ORIGIN, source: unknown = frame?.contentWindow) =>
  window.dispatchEvent(new MessageEvent('message', { data, origin, source: source as Window }));
const badge = () => document.querySelector('.demo-badge')!.textContent;
const showButton = () => document.querySelector<HTMLButtonElement>('[data-action="show"]')!;

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

test('a narrow window keeps the still: no frame, nothing loaded, even if show is called', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(false) });
  expect(demo.state).toBe('still');
  expect(root.querySelector('iframe')).toBeNull();
  expect(badge()).toBe('Screenshot');
  demo.show();
  expect(root.querySelector('iframe')).toBeNull();
});

test('a wide window keeps the still until Show demo is pressed', () => {
  const root = hero();
  initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  expect(root.querySelector('iframe')).toBeNull();
  expect(badge()).toBe('Screenshot');
  expect(showButton().hidden).toBe(false);
});

test('Show demo starts the demo in a titled frame over the still, loading', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  demo.show();
  const frame = root.querySelector<HTMLIFrameElement>('.stage > iframe.demo-live')!;
  expect(frame.title).toBe('GitNexia live demo');
  expect(demo.frame).toBe(frame);
  expect(demo.state).toBe('loading');
  expect(badge()).toBe('Loading the live demo…');
});

test('the Show demo button starts the demo', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  showButton().click();
  expect(demo.frame).not.toBeNull();
  expect(demo.state).toBe('loading');
});

test('"ready" from the frame shows the demo', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  demo.show();
  report(demo.frame, { source: 'gitnexia-demo', type: 'ready' });
  expect(demo.state).toBe('ready');
  expect(badge()).toBe('Live demo');
});

test('a message from another window or origin, or of another shape, changes nothing', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  demo.show();
  report(demo.frame, { source: 'gitnexia-demo', type: 'ready' }, 'https://elsewhere.example');
  report(demo.frame, { source: 'gitnexia-demo', type: 'ready' }, ORIGIN, window);
  report(demo.frame, { type: 'ready' });
  report(demo.frame, 'ready');
  expect(demo.state).toBe('loading');
});

test("a failed report keeps the still and says why", () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  demo.show();
  report(demo.frame, { source: 'gitnexia-demo', type: 'failed', message: "The demo's data is missing: orbital.gbdemo (404)" });
  expect(demo.state).toBe('failed');
  expect(badge()).toBe("The live demo didn't start: The demo's data is missing: orbital.gbdemo (404)");
});

test('no report in time: failed, not an endless "Loading"', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true), timeoutMs: 1000 });
  demo.show();
  vi.advanceTimersByTime(999);
  expect(demo.state).toBe('loading');
  vi.advanceTimersByTime(1);
  expect(demo.state).toBe('failed');
  expect(badge()).toBe("The live demo didn't start: it took too long to load");
});

test('show does nothing while it is already loading or ready', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  demo.show();
  const frame = demo.frame;
  demo.show();
  expect(demo.frame).toBe(frame);
  expect(root.querySelectorAll('iframe')).toHaveLength(1);
  report(frame, { source: 'gitnexia-demo', type: 'ready' });
  demo.show();
  expect(demo.frame).toBe(frame);
  expect(root.querySelectorAll('iframe')).toHaveLength(1);
});

test('a failed demo can be tried again with a fresh frame', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  demo.show();
  const old = demo.frame;
  report(old, { source: 'gitnexia-demo', type: 'failed', message: 'missing' });
  demo.show();
  expect(demo.frame).not.toBe(old);
  expect(demo.state).toBe('loading');
  expect(root.querySelectorAll('iframe')).toHaveLength(1);
});

test('reset replaces the frame with a fresh one, loading again; the old one\'s word no longer counts', () => {
  const root = hero();
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: wide(true) });
  demo.show();
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
  const demo = initHeroDemo(root, { src: 'about:blank', origin: ORIGIN, wide: q });
  demo.show();
  report(demo.frame, { source: 'gitnexia-demo', type: 'ready' });
  expect(badge()).toBe('Live demo');
  q.set(false);
  expect(badge()).toBe('Screenshot');
  q.set(true);
  expect(badge()).toBe('Live demo');
});
