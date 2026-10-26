/*
 * Riptide website. Vanilla JS, no dependencies, no network.
 *
 * 1. Theme toggle (light by default; a saved choice wins), the mobile menu, the Animations switch and its one-time hint.
 * 2. One engine loop (a single requestAnimationFrame that sleeps when nothing moves). Pinned chapters (logo, app; desktop, full
 *    motion) turn the scroll position into a progress value written as --p; the value is eased toward the scroll position
 *    (a light lerp on the VALUE only: native scrolling is untouched) and reaches its end state at 85 % of the stage, which leaves
 *    a calm resting moment. Every other chapter (phones, calm motion, speed, integrity) plays once, by time, when it comes
 *    into view. CSS derives every transform, opacity, stroke-dashoffset and clip-path from --p.
 * 3. Chapters: the stream-to-file logo draw, the eight-connection race, the power cut and resume, the SHA-256 check.
 * 4. The hero download simulation (labelled "Simulation" on the page), paused when off-screen or in a hidden tab.
 * 5. Reveals, current-section highlight in the nav, pausing of endless loops off-screen.
 * 6. Robustness: re-measure on resize / orientation / fonts / image load, restore on bfcache and hash jumps, hidden-tab pause,
 *    a frame-time monitor that drops to the "lite" level on sustained slow frames, and a failsafe that shows final states
 *    if anything throws. html.js-ready is added only after a successful init; every hidden start state in the CSS depends on it.
 *
 * html[data-motion]: "full" (class anim) or "reduced" (class calm) is decided in <head>; html[data-level] is "full" or "lite".
 */
(function () {
  'use strict';

  const root = document.documentElement;
  const full = root.dataset.motion === 'full';
  const dir = root.dir === 'rtl' ? -1 : 1;
  const clamp = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
  const easeIO = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
  const easeOut = (t) => 1 - Math.pow(1 - t, 3);
  const r4 = (v) => Math.round(v * 10000) / 10000;
  const store = {
    get(k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
    set(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* storage blocked */ } },
  };
  const isLite = () => root.dataset.level === 'lite';
  const safe = (name, fn) => { try { fn(); } catch (e) { console.warn('[riptide] ' + name + ' skipped:', e); } };

  // If this script ever throws, fall back to the plain final states (the no-JS styles).
  window.addEventListener('error', (e) => {
    if (e.filename && e.filename.indexOf('site.js') < 0) return;
    root.classList.remove('js-ready');
    root.classList.add('rv-all');
  });

  // ---- 1. Theme, menu, Animations switch and hint ------------------------------------------
  safe('theme', () => {
    const themeButton = document.querySelector('.theme-toggle');
    if (!themeButton) return;
    themeButton.addEventListener('click', () => {
      const theme = root.dataset.theme === 'dark' ? 'light' : 'dark';
      root.classList.add('theme-anim');
      root.dataset.theme = theme;
      setTimeout(() => root.classList.remove('theme-anim'), 520);
      const meta = document.querySelector('meta[name="theme-color"]');
      if (meta) meta.setAttribute('content', theme === 'dark' ? '#071a26' : '#ffffff');
      store.set('riptide-theme', theme);
    });
  });
  const setMotion = (next) => {
    try { localStorage.setItem('riptide-motion', next); } catch (e) { location.search = '?motion=' + next; return; }
    location.reload(); // pinned chapters and listeners are set up once, so a clean reload is the robust switch
  };
  safe('motion switch', () => {
    const button = document.querySelector('.motion-toggle');
    if (!button) return;
    button.setAttribute('aria-pressed', String(full));
    button.addEventListener('click', () => setMotion(full ? 'off' : 'on'));
  });
  safe('motion hint', () => {
    const hint = document.querySelector('.mo-hint');
    if (!hint || full) return;
    const osReduces = window.matchMedia && matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (!osReduces || store.get('riptide-motion') !== null || store.get('riptide-motion-hint') !== null) return;
    hint.hidden = false;
    const close = () => { hint.hidden = true; store.set('riptide-motion-hint', '1'); };
    hint.querySelector('.mo-on').addEventListener('click', () => { store.set('riptide-motion-hint', '1'); setMotion('on'); });
    hint.querySelector('.mo-x').addEventListener('click', close);
    hint.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
  });
  safe('release pill', () => {
    // Shown once per version: a dismissal is remembered for that version only. A row of its own above the
    // hero (never over the headline). The motion hint, when open, pushes it down.
    const pill = document.querySelector('.rel-pill');
    if (!pill) return;
    const version = pill.dataset.version;
    const hint = document.querySelector('.mo-hint');
    if (!version || store.get('riptide-release-seen') === version) return;
    // When the motion hint is open (reduced-motion visitors, first visit) the pill sits below it instead of beside it.
    const place = () => { pill.style.marginBlockStart = hint && !hint.hidden ? 'calc(var(--hh) + 16px + ' + hint.offsetHeight + 'px)' : ''; };
    place();
    if (hint && window.MutationObserver) new MutationObserver(place).observe(hint, { attributes: true, attributeFilter: ['hidden'] });
    pill.hidden = false;
    pill.querySelector('.rel-x').addEventListener('click', () => { pill.hidden = true; store.set('riptide-release-seen', version); });
    pill.addEventListener('keydown', (e) => { if (e.key === 'Escape') { pill.hidden = true; store.set('riptide-release-seen', version); } });
  });
  const header = document.querySelector('.site-header');
  safe('menu', () => {
    const navToggle = document.querySelector('.nav-toggle');
    if (!header || !navToggle) return;
    const setOpen = (open) => {
      header.classList.toggle('is-open', open);
      navToggle.setAttribute('aria-expanded', String(open));
    };
    navToggle.addEventListener('click', () => setOpen(navToggle.getAttribute('aria-expanded') !== 'true'));
    header.querySelectorAll('.nav-menu a').forEach((a) => a.addEventListener('click', () => setOpen(false)));
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape') setOpen(false); });
  });

  const main = document.getElementById('main');
  const hero = document.getElementById('hero');
  if (!header || !main || !hero) {
    if (header) header.classList.add('is-solid');
    return;
  }

  // Hidden tab: pause CSS loops (html.tab-hidden) and the simulation; resume cleanly.
  const visibility = [];
  document.addEventListener('visibilitychange', () => {
    root.classList.toggle('tab-hidden', document.hidden);
    visibility.forEach((fn) => { try { fn(document.hidden); } catch (e) { /* keep going */ } });
  });

  // ---- 4. Hero simulation -----------------------------------------------------------------
  // A pretend download that loops: runs, verifies, completes, starts over. Static "completed" markup
  // is what visitors without JS see. It runs in calm motion too (progress and counters are not spatial).
  safe('simulation', () => {
    const sim = hero.querySelector('[data-sim]');
    const spd = hero.querySelector('[data-o="speed"]');
    const sha = hero.querySelector('[data-o="sha"]');
    if (!sim || !spd || !sha) return;
    const pct = sim.querySelector('[data-o="pct"]');
    const state = sim.querySelector('[data-o="state"]');
    const fills = Array.from(sim.querySelectorAll('.sim-bar b'));
    const doneText = state.textContent;
    const ar = root.lang === 'ar';
    const stateText = { run: ar ? 'جارٍ التحميل' : 'Downloading', ver: ar ? 'جارٍ التحقق' : 'Verifying' };
    // Eight connections with different speeds; the file is done when the slowest one finishes.
    const rates = [1, 0.78, 0.9, 0.66, 0.97, 0.72, 0.84, 0.6];
    const DL = isLite() ? 5.5 : 9, VERIFY = 1.1, HOLD = isLite() ? 2.4 : 4.2;
    let t0 = performance.now() + 1500;
    let first = true;
    let running = false;
    let raf = 0;
    let textAt = 0;
    let last = '';
    let pausedAt = 0;
    sha.classList.remove('is-on');
    const draw = (now) => {
      let t = (now - t0) / 1000;
      if (t < 0) t = 0;
      const start = first ? 0.4 : 0; // the first loop is already underway when the page opens
      const dl = clamp(start + (1 - start) * (t / DL));
      let phase = 'run';
      if (t >= DL + VERIFY) phase = 'done'; else if (t >= DL) phase = 'ver';
      let sum = 0;
      fills.forEach((el, i) => {
        const x = phase === 'run' ? clamp(dl * (1 + (rates[i] - 0.6) * 0.7)) : 1;
        sum += x;
        const s = `scaleX(${r4(x)})`;
        if (el.__s !== s) { el.style.transform = s; el.__s = s; }
      });
      const p = Math.min(100, Math.floor((sum / fills.length) * 100));
      const speed = phase === 'run' ? 21.4 + 2.4 * Math.sin(t * 2.3) + 0.9 * Math.sin(t * 7.1) : 0;
      const key = phase + p + speed.toFixed(1);
      if (key !== last && (phase !== 'run' || now - textAt > 120 || last.slice(0, 3) !== 'run')) {
        textAt = now;
        last = key;
        pct.textContent = `${phase === 'run' ? p : 100}%`;
        state.textContent = phase === 'run' ? stateText.run : phase === 'ver' ? stateText.ver : doneText;
        spd.textContent = phase === 'run' ? speed.toFixed(1) : '0.0';
        sha.classList.toggle('is-on', phase === 'done');
      }
      if (t > DL + VERIFY + HOLD) { first = false; t0 = now + 600; last = ''; sha.classList.remove('is-on'); fills.forEach((el) => { el.style.transform = 'scaleX(0)'; el.__s = ''; }); }
    };
    const tick = (now) => { if (!running) return; draw(now); raf = requestAnimationFrame(tick); };
    const start = () => {
      if (running || document.hidden) return;
      running = true;
      if (pausedAt) { t0 += performance.now() - pausedAt; pausedAt = 0; } // resume exactly where it stopped
      raf = requestAnimationFrame(tick);
    };
    const stop = () => { if (!running) return; running = false; cancelAnimationFrame(raf); pausedAt = performance.now(); };
    let visible = true;
    if ('IntersectionObserver' in window) {
      new IntersectionObserver((es) => { es.forEach((e) => { visible = e.isIntersecting; if (visible) start(); else stop(); }); }).observe(sim);
    } else start();
    visibility.push((hidden) => { if (hidden) stop(); else if (visible) start(); });
  });

  // ---- 3. Chapters ------------------------------------------------------------------------
  const scenes = {};

  // Streams: the three streams draw the real mark (revealed through masks, never recoloured), the wordmark
  // rises, then the same three lanes run down out of the R's stem into one file.
  (function () {
    const pin = document.querySelector('#streams .pin');
    if (!pin) return;
    const set = (k, v) => pin.style.setProperty(k, v);
    const msd = parseFloat(getComputedStyle(pin).getPropertyValue('--msd')) || 1.45;
    let lastMsd = msd;
    scenes.streams = {
      update(p) {
        const q = clamp(p / 0.56) * 0.8;
        const spin = easeIO(clamp(q / 0.5));
        const m = parseFloat(getComputedStyle(pin).getPropertyValue('--msd')) || lastMsd;
        lastMsd = m;
        for (let i = 0; i < 3; i++) {
          const base = [0.3, 0.33, 0.37][i];
          set(`--s${i + 1}`, r4(base + (1 - base) * easeOut(clamp((q - 0.02 - i * 0.05) / 0.42))));
          set(`--r${i + 1}`, r4(-Math.sin(Math.PI * clamp(q / 0.52)) * (22 + 16 * i)));
        }
        const lg = easeIO(clamp((q - 0.56) / 0.16));
        const draw = m * (1.12 - 0.12 * spin);
        set('--ms', r4(draw + (1 - draw) * lg));
        set('--mx', full ? r4(265.5 * (1 - lg)) : 0);
        set('--my', full ? r4(22.7 * (1 - lg)) : 0);
        set('--lg', r4(clamp((q - 0.6) / 0.16)));
        for (let i = 0; i < 3; i++) set(`--l${i}`, r4(easeIO(clamp((p - 0.58 - i * 0.05) / 0.3))));
        set('--fc', r4(easeOut(clamp((p - 0.88) / 0.1))));
      },
    };
  })();

  // Speed: a deterministic work-stealing schedule, computed once, scrubbed by scroll.
  (function () {
    const section = document.getElementById('speed');
    if (!section) return;
    const lanes = Array.from(section.querySelectorAll('.lane-track'));
    const filebar = section.querySelector('.filebar');
    const out = {};
    section.querySelectorAll('[data-o]').forEach((n) => { out[n.dataset.o] = n; });
    const rates = [1, 0.62, 0.86, 0.5, 0.94, 0.7, 0.56, 0.8];
    const conns = rates.map((rate, c) => ({ rate, pos: c / 8, end: (c + 1) / 8, done: Infinity, pieces: [] }));
    const pieces = [];
    const addPiece = (k, a, b, t0) => { const pc = { c: k, a, b, t0, rate: conns[k].rate }; conns[k].pieces.push(pc); pieces.push(pc); return pc; };
    conns.forEach((k, c) => { k.cur = addPiece(c, k.pos, k.end, 0); });
    let t = 0;
    for (let guard = 0; guard < 200; guard++) {
      let next = -1, dt = Infinity;
      conns.forEach((k, c) => { if (k.done === Infinity) { const d = (k.end - k.pos) / k.rate; if (d < dt) { dt = d; next = c; } } });
      if (next < 0) break;
      t += dt;
      conns.forEach((k) => { if (k.done === Infinity) k.pos = Math.min(k.end, k.pos + k.rate * dt); });
      const k = conns[next];
      let victim = -1, most = 0;
      conns.forEach((v, c) => { const rem = v.end - v.pos; if (c !== next && v.done === Infinity && rem > most) { most = rem; victim = c; } });
      if (victim >= 0 && most > 0.03) {
        const v = conns[victim];
        const mid = v.pos + most / 2;
        k.cur = addPiece(next, mid, v.end, t);
        k.pos = mid; k.end = v.end;
        v.end = mid; v.cur.b = mid;
      } else {
        k.done = t;
      }
    }
    const tEnd = t;
    const make = (pc) => {
      const el = document.createElement('span');
      el.className = pc.t0 > 0 ? 'pc stolen' : 'pc';
      el.style.setProperty('--a', String(pc.a));
      el.style.setProperty('--b', String(pc.b));
      el.appendChild(document.createElement('i'));
      return el;
    };
    let built = false;
    const build = () => {
      built = true;
      lanes.forEach((lane) => lane.querySelectorAll('.pc').forEach((n) => n.remove()));
      filebar.textContent = '';
      pieces.forEach((pc) => {
        const a = make(pc), b = make(pc);
        lanes[pc.c].insertBefore(a, lanes[pc.c].querySelector('.hd'));
        filebar.appendChild(b);
        pc.els = [a.firstChild, b.firstChild];
        pc.last = -1;
      });
    };
    const heads = lanes.map((lane) => lane.querySelector('.hd'));
    scenes.speed = {
      update(p) {
        if (!built) build();
        const T = clamp(p / 0.82) * tEnd;
        let sum = 0, active = 0;
        pieces.forEach((pc) => {
          const f = r4(clamp(((T - pc.t0) * pc.rate) / (pc.b - pc.a)));
          sum += f * (pc.b - pc.a);
          if (f !== pc.last) { pc.last = f; pc.els[0].style.transform = pc.els[1].style.transform = `scaleX(${f})`; }
        });
        conns.forEach((k, c) => {
          const on = T > 0 && T < k.done;
          if (k.done > T) active++;
          let x = k.pieces[0].a;
          for (const pc of k.pieces) if (pc.t0 <= T) x = Math.min(pc.b, pc.a + (T - pc.t0) * pc.rate);
          heads[c].style.opacity = on ? '1' : '0';
          heads[c].style.transform = `translate3d(${r4(x * 100 * dir)}%,0,0)`;
        });
        const pct = Math.round(sum * 100);
        out.pct.textContent = `${pct}%`;
        out.gb.textContent = `${(4.7 * sum).toFixed(2)} / 4.70 GB`;
        out.active.textContent = String(active);
      },
    };
  })();

  // Integrity: power cut and exact resume, then the digest resolves and the check draws.
  (function () {
    const section = document.getElementById('integrity');
    if (!section) return;
    const rs = section.querySelector('.rs');
    const vf = section.querySelector('.vf');
    const fill = rs.querySelector('.rs-fill');
    const bytes = rs.querySelector('[data-o="bytes"]');
    const ok = vf.querySelector('[data-o="ok"]');
    const scr = vf.querySelector('[data-o="scr"]');
    const hash = ok.textContent.trim();
    const hex = '0123456789abcdef';
    const TOTAL = 5046586572, CUT = 0.62;
    const nf = new Intl.NumberFormat('en-US');
    rs.querySelector('[data-o="at"]').textContent = nf.format(Math.round(CUT * TOTAL));
    scenes.integrity = {
      update(p) {
        const a = clamp(p / 0.56);
        let f, phase, cut = 0;
        if (a < 0.4) { f = CUT * easeIO(clamp(a / 0.4)); phase = 'run'; }
        else if (a < 0.52) { f = CUT; phase = 'cut'; cut = clamp((a - 0.4) / 0.025) * (1 - clamp((a - 0.45) / 0.05)) * 0.9; }
        else if (a < 0.92) { f = CUT + (1 - CUT) * easeIO(clamp((a - 0.58) / 0.34)); phase = 'res'; }
        else { f = 1; phase = 'done'; }
        if (rs.dataset.phase !== phase) rs.dataset.phase = phase;
        rs.style.setProperty('--cut', String(r4(cut)));
        fill.style.transform = `scaleX(${r4(f)})`;
        bytes.textContent = nf.format(Math.round(f * TOTAL));

        const v = clamp((p - 0.5) / 0.46);
        vf.style.setProperty('--v', String(r4(v)));
        const n = Math.round(clamp(v / 0.5) * hash.length);
        const seed = Math.floor(v * 120);
        let s = '';
        for (let i = n; i < hash.length; i++) s += hex[(i * 7 + seed * 13 + ((i * i) % 11)) & 15];
        ok.textContent = hash.slice(0, n);
        scr.textContent = s;
      },
    };
  })();

  // ---- 2. The engine ----------------------------------------------------------------------
  const END = 0.85;          // a pinned stage reaches its end state at 85 % of its scroll, then rests
  const LERP = 0.15;         // per-frame (at 60 fps) easing of the progress VALUE toward the scroll position
  const sections = Array.from(main.children).filter((el) => el.tagName === 'SECTION');
  const navLinks = Array.from(header.querySelectorAll('.nav-links a'));
  const navTargets = navLinks.map((a) => document.getElementById(a.getAttribute('href').slice(1)));
  const hasIO = 'IntersectionObserver' in window;
  let chapters = [];
  let vh = window.innerHeight;
  let heroH = 1;
  let measured = false;
  let snap = true;           // jump straight to the target (first frame, resize, bfcache, hash jumps)
  let raf = 0;
  let lastT = 0;
  let lastScrollAt = 0;
  let tone = '';
  let current = -2;
  const samples = [];        // frame-time monitor

  function modeOf(pin) {
    return full && pin && getComputedStyle(pin).position === 'sticky' ? 'pin' : 'timed';
  }

  function measure() {
    vh = window.innerHeight;
    const y = window.scrollY;
    heroH = hero.offsetHeight || 1;
    const old = new Map(chapters.map((c) => [c.el, c]));
    chapters = sections.map((el) => {
      const r = el.getBoundingClientRect();
      const name = el.dataset.scene;
      const pin = el.querySelector('.pin');
      const prev = old.get(el);
      const c = prev || { el, name, pin, scene: scenes[name], cur: NaN, last: NaN, state: 'idle', t0: 0, D: 0, mode: '' };
      c.top = r.top + y;
      c.h = r.height;
      c.tone = el.dataset.tone || 'light';
      const mode = name && name !== 'hero' && pin ? modeOf(pin) : '';
      if (mode !== c.mode) {
        // Switched between pinned and one-shot (a resize across 900px): keep whatever was already shown.
        const seen = c.state === 'done' || (c.mode === 'pin' && c.cur > 0.02);
        c.mode = mode;
        c.state = seen || y > c.top + c.h ? 'done' : 'idle';
        c.last = NaN;
        c.cur = NaN;
        snap = true;
        el.dataset.mode = mode || 'static';
      }
      return c;
    });
    measured = true;
  }

  function apply(c, p) {
    if (p === c.last) return;
    c.last = p;
    c.pin.style.setProperty('--p', String(p));
    if (c.scene) c.scene.update(clamp(p));
  }

  const ensure = () => { if (!measured) { measure(); initIdle(); } };

  // One-shot chapters start hidden (p = -1: headline masked, scene at its start) until they come into view.
  function initIdle() {
    chapters.forEach((c) => { if (c.mode === 'timed' && c.state === 'idle') apply(c, -1); });
  }

  function play(c) {
    if (c.mode !== 'timed' || c.state !== 'idle') return;
    c.state = 'play';
    c.t0 = performance.now();
    c.D = (c.name === 'speed' ? 5200 : c.name === 'integrity' ? 5600 : c.name === 'streams' ? 4200 : 2200) * (isLite() ? 0.6 : 1);
    wake();
  }

  function frame(now) {
    raf = 0;
    const raw_dt = lastT ? now - lastT : 16.7;
    const dt = Math.max(1, Math.min(100, raw_dt));
    const continuous = lastT && now - lastScrollAt < 400 && raw_dt < 1000;
    lastT = now;
    ensure();
    const y = window.scrollY;
    let busy = false;

    // Frame-time monitor: sustained slow frames while something is moving switch to the lite level.
    if (continuous && !document.hidden && !isLite()) {
      samples.push(raw_dt);
      if (samples.length > 40) samples.shift();
      if (samples.length >= 30) {
        const sorted = samples.slice().sort((a, b) => a - b);
        if (sorted[sorted.length >> 1] > 32) {
          root.dataset.level = 'lite';
          try { sessionStorage.setItem('riptide-lite', '1'); } catch (e) { /* storage blocked */ }
          hero.style.setProperty('--hp', '0');
        }
      }
    } else if (!continuous) samples.length = 0;

    if (full && !isLite() && y <= heroH) hero.style.setProperty('--hp', String(r4(clamp(y / heroH))));

    const k = 1 - Math.pow(1 - (isLite() ? 0.3 : LERP), dt / 16.67);
    for (const c of chapters) {
      if (c.mode === 'pin') {
        const off = y + vh <= c.top || y >= c.top + c.h;
        const len = c.h - vh;
        let raw;
        if (off) raw = y >= c.top ? 1 : -1;
        else if (y < c.top) raw = (y - c.top) / vh;
        else raw = len > 0 ? Math.min(1, (y - c.top) / len) : 1;
        const target = raw >= 0 ? clamp(raw / END) : raw;
        if (off || snap || !(c.cur === c.cur)) c.cur = target;
        else {
          const d = target - c.cur;
          if (Math.abs(d) < 0.0006) c.cur = target;
          else { c.cur += d * k; busy = true; }
        }
        apply(c, r4(c.cur));
      } else if (c.mode === 'timed' && c.state === 'play') {
        const t = (now - c.t0) / c.D;
        if (t >= 1) { c.state = 'done'; apply(c, 1); }
        else { apply(c, r4(-0.3 + 1.3 * easeOut(t))); busy = true; }
      } else if (c.mode === 'timed' && c.state === 'done') apply(c, 1);
    }
    snap = false;

    const probe = y + 34;
    let t = 'light';
    for (const c of chapters) if (probe >= c.top && probe < c.top + c.h) { t = c.tone; break; }
    if (t !== tone) { tone = t; header.dataset.tone = t; }
    header.classList.toggle('is-solid', y > 8);
    let idx = -1;
    navTargets.forEach((el, i) => { if (el) { const top = el.getBoundingClientRect().top; if (top <= vh * 0.4) idx = i; } });
    const lastEl = navTargets[idx];
    if (lastEl && lastEl.getBoundingClientRect().bottom < vh * 0.4) idx = -1;
    if (idx !== current) { current = idx; navLinks.forEach((a, i) => { if (i === idx) a.setAttribute('aria-current', 'true'); else a.removeAttribute('aria-current'); }); }

    if (now - lastScrollAt < 120) busy = true;
    if (busy) raf = requestAnimationFrame(frame); else lastT = 0;
  }
  function wake() { if (!raf && !document.hidden) raf = requestAnimationFrame(frame); }

  // Re-measure (debounced to one frame) without jumping: values re-target and ease unless a snap is asked for.
  let measureTimer = 0;
  function remeasure(hard) {
    if (!measured) return;
    clearTimeout(measureTimer);
    measureTimer = setTimeout(() => { measure(); if (hard) snap = true; wake(); }, 60);
  }

  // Nothing is measured until the visitor scrolls (the CSS already shows every start state), so loading never forces layout;
  // a reload mid-page, an anchor jump or a back/forward restore fires a scroll event immediately. If nobody scrolled a few
  // seconds in, it happens in idle time, so the first scroll is not the one that pays. The IntersectionObserver below
  // starts the one-shot chapters, so no animation depends on this single event.
  const begin = () => { ensure(); wake(); };
  window.addEventListener('scroll', () => { lastScrollAt = performance.now(); if (!measured) ensure(); wake(); }, { passive: true });
  window.addEventListener('load', () => setTimeout(() => (window.requestIdleCallback ? requestIdleCallback(begin) : begin()), 4000));
  window.addEventListener('resize', () => remeasure(false));
  window.addEventListener('orientationchange', () => remeasure(true));
  window.addEventListener('hashchange', () => { snap = true; wake(); });
  window.addEventListener('popstate', () => { snap = true; wake(); });
  window.addEventListener('pageshow', (e) => {
    root.classList.remove('tab-hidden');
    if (e.persisted) { measured = true; measure(); snap = true; wake(); }
  });
  visibility.push((hidden) => { if (!hidden) { remeasure(true); lastT = 0; wake(); } });
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(() => remeasure(false));
  document.addEventListener('load', (e) => { if (e.target && e.target.tagName === 'IMG') remeasure(false); }, true);
  if ('ResizeObserver' in window) {
    let lastH = 0;
    new ResizeObserver(() => { const h = document.body.scrollHeight; if (h !== lastH) { lastH = h; remeasure(false); } }).observe(document.body);
  }

  // ---- 5. One-shot chapters, reveals, and pausing of endless loops off-screen -------------
  safe('observers', () => {
    if (!hasIO) {
      // No IntersectionObserver: show every final state.
      root.classList.add('rv-all');
      sections.forEach((el) => { const pin = el.querySelector('.pin'); if (pin && el.dataset.scene) pin.style.setProperty('--p', '1'); });
      return;
    }
    const chapterIO = new IntersectionObserver((entries) => {
      entries.forEach((e) => {
        if (!e.isIntersecting) return;
        ensure();
        const c = chapters.find((x) => x.el === e.target);
        if (c) play(c);
      });
    }, { rootMargin: '0px 0px -22% 0px', threshold: 0 });
    sections.forEach((el) => { if (el.dataset.scene && el.dataset.scene !== 'hero') chapterIO.observe(el); });

    const loops = new IntersectionObserver((entries) => {
      entries.forEach((e) => e.target.classList.toggle('off', !e.isIntersecting));
    }, { rootMargin: '120px 0px' });
    document.querySelectorAll('.ch, .bp-video, .hero').forEach((el) => loops.observe(el));

    const io = new IntersectionObserver((entries) => {
      entries.forEach((e) => {
        if (!e.isIntersecting) return;
        e.target.classList.add('in');
        io.unobserve(e.target);
      });
    }, { rootMargin: '0px 0px -10% 0px', threshold: 0.15 });
    document.querySelectorAll('.rv, .cta').forEach((el) => io.observe(el));
  });

  // Everything initialised: from here on the CSS may hide things for their entrance.
  root.classList.add('js-ready');
})();
