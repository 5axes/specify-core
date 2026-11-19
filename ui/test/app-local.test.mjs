import { test } from 'node:test';
import assert from 'node:assert/strict';
import { keepAlive } from '../app/local.ts';
import { createSession } from '../app/answers.ts';

const config = { name: 'part.step', saveTarget: '/tmp/part.specified.step' };
const flush = async () => { for (let n = 0; n < 20; n++) await Promise.resolve(); };

/** Browser timers run against wall time. Sleeping moves time without executing callbacks. */
function browser(t) {
  let now = 0;
  let id = 0;
  const timers = new Map();
  const cleanup = [];
  const window = new EventTarget();
  const document = Object.assign(new EventTarget(), { visibilityState: 'visible' });
  const schedule = (callback, delay, repeat = false) => {
    timers.set(++id, { callback, at: now + delay, delay, repeat });
    return id;
  };
  window.setTimeout = (callback, delay) => schedule(callback, delay);
  window.setInterval = (callback, delay) => schedule(callback, delay, true);
  window.clearTimeout = window.clearInterval = (id) => timers.delete(id);
  const original = new Map(['window', 'document', 'navigator'].map((key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: window },
    document: { configurable: true, value: document },
    navigator: { configurable: true, value: { sendBeacon: () => true } },
  });
  t.mock.method(Date, 'now', () => now);
  t.after(() => {
    for (const close of cleanup) close();
    for (const [key, descriptor] of original) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  });
  return {
    window, document,
    cleanup(close) { cleanup.push(close); },
    sleep(ms) { now += ms; },
    async advance(ms) {
      await flush();
      const end = now + ms;
      for (;;) {
        const due = [...timers].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        const [id, timer] = due;
        now = Math.max(now, timer.at);
        if (timer.repeat) timer.at = now + timer.delay;
        else timers.delete(id);
        timer.callback();
        await flush();
      }
      now = end;
      await flush();
    },
  };
}

function command(t) {
  const calls = [];
  const state = { mode: 'ok' };
  t.mock.method(globalThis, 'fetch', (_url, { signal }) => {
    const call = { signal, started: Date.now() };
    calls.push(call);
    if (state.mode === 'ok') return Promise.resolve(Response.json(config));
    if (state.mode === 'busy') return Promise.resolve(Response.json({ error: 'busy' }, { status: 503 }));
    if (state.mode === 'offline') return Promise.reject(new TypeError('connection refused'));
    return new Promise((resolve, reject) => {
      call.answer = () => resolve(Response.json(config));
      signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    });
  });
  return { state, calls };
}

test('a stopped command continues probing and a later success restores Save eligibility', async (t) => {
  const clock = browser(t);
  const { state } = command(t);
  const statuses = [];
  const configs = [];
  const heartbeat = keepAlive((c) => configs.push(c), (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  heartbeat.refresh();
  await flush();
  state.mode = 'offline';
  await clock.advance(1_000);
  heartbeat.refresh();
  await clock.advance(23_000);
  assert.deepEqual(statuses, [], '25 seconds since the last successful response have not elapsed');
  await clock.advance(1_000);
  assert.deepEqual(statuses, [true]);
  state.mode = 'ok';
  await clock.advance(4_000);
  assert.deepEqual(statuses, [true, false], 'recovery clears the stopped state');
  assert.equal(configs.length, 2);
});

test('sustained HTTP 503 responses keep the command alive', async (t) => {
  const clock = browser(t);
  const { state, calls } = command(t);
  const statuses = [];
  state.mode = 'busy';
  const heartbeat = keepAlive(() => {}, (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  heartbeat.refresh();
  await clock.advance(180_000);
  assert.ok(calls.length > 30, 'busy commands are retried');
  assert.deepEqual(statuses, []);
});

test('a fresh probe after sleep survives overdue timers and a healthy response', async (t) => {
  const clock = browser(t);
  const { state, calls } = command(t);
  const statuses = [];
  const heartbeat = keepAlive(() => {}, (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  state.mode = 'offline';
  heartbeat.refresh();
  await clock.advance(24_000);
  clock.sleep(600_000);
  state.mode = 'pending';
  // Start a healthy wake probe before the overdue interval callback gets its turn.
  heartbeat.refresh();
  const fresh = calls.at(-1);
  await clock.advance(4_000);
  assert.equal(fresh.signal.aborted, false);
  assert.deepEqual(statuses, []);
  fresh.answer();
  await flush();
  state.mode = 'offline';
  heartbeat.refresh();
  await clock.advance(24_000);
  assert.deepEqual(statuses, [], 'the healthy response restarted the observation window');
});

test('visibility resume resets the failure window and gives an in-flight probe time to answer', async (t) => {
  const clock = browser(t);
  const { state, calls } = command(t);
  const statuses = [];
  const heartbeat = keepAlive(() => {}, (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  state.mode = 'offline';
  heartbeat.refresh();
  await clock.advance(20_000);
  state.mode = 'pending';
  heartbeat.refresh();
  const fresh = calls.at(-1);
  await clock.advance(4_000);
  clock.document.dispatchEvent(new Event('visibilitychange'));
  await clock.advance(3_000);
  assert.equal(fresh.signal.aborted, false, 'the pre-resume response timeout no longer applies');
  fresh.answer();
  await flush();
  assert.deepEqual(statuses, []);
});

test('a probe hung before sleep is timed out only after a fresh response window on wake', async (t) => {
  const clock = browser(t);
  const { state, calls } = command(t);
  const heartbeat = keepAlive(() => {}, () => {}, () => {});
  clock.cleanup(heartbeat.stop);
  state.mode = 'pending';
  heartbeat.refresh();
  const old = calls.at(-1);
  clock.sleep(60_000);
  await clock.advance(0);
  assert.equal(old.signal.aborted, false);
  await clock.advance(5_000);
  assert.equal(old.signal.aborted, true);
  state.mode = 'ok';
  await clock.advance(4_000);
  assert.equal(calls.length, 2, 'a hung request does not prevent later recovery');
});

test('pagehide aborts PUTs across parts and cancels queued retries before the closed beacon', async (t) => {
  const clock = browser(t);
  const events = [];
  const signals = [];
  t.mock.method(globalThis, 'fetch', async (_url, { signal }) => new Promise((_resolve, reject) => {
    signals.push(signal);
    events.push('PUT');
    signal.addEventListener('abort', () => {
      events.push('abort');
      reject(signal.reason);
    }, { once: true });
  }));
  const session = createSession(0, { 0: {}, 1: {} });
  const a = session.store(0);
  const b = session.store(1);
  a.set({ material: 'Steel' }, {});
  b.set({ material: 'Brass' }, {});
  await flush();
  a.set({ material: 'Titanium' }, a.get());
  const heartbeat = keepAlive(() => {}, () => {}, () => session.close());
  clock.cleanup(heartbeat.stop);
  t.mock.method(navigator, 'sendBeacon', () => {
    assert.ok(signals.every((signal) => signal.aborted), 'every active PUT is aborted before closed');
    events.push('closed');
    return true;
  });
  clock.window.dispatchEvent(Object.assign(new Event('pagehide'), { persisted: true }));
  assert.deepEqual(events, ['PUT', 'PUT'], 'bfcache navigation keeps the session active');
  clock.window.dispatchEvent(Object.assign(new Event('pagehide'), { persisted: false }));
  await Promise.all([a.settled(), b.settled()]);
  a.set({ material: 'Copper' }, a.get());
  await clock.advance(60_000);
  assert.deepEqual(events, ['PUT', 'PUT', 'abort', 'abort', 'closed'], 'no queued PUT or heartbeat follows closed');
});
