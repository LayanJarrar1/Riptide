/*
 * Riptide website. Vanilla JS, no dependencies, no network.
 *
 * 1. Theme toggle (light by default; a saved choice wins) and the mobile menu.
 * 2. One scroll loop: a passive scroll listener schedules at most one requestAnimationFrame, which turns the
 *    scroll position into a progress value per pinned chapter (written as --p on its stage) and the hero's
 *    parallax value (--hp). CSS derives every transform, opacity, stroke-dashoffset and clip-path from those.
 *    Offsets are measured on load, resize and font load, never inside the frame.
 * 3. Chapters: the stream-to-file logo draw, the eight-connection race, the power cut and resume, the
 *    SHA-256 check. All deterministic, so scrolling back up plays everything in reverse.
 * 4. The hero download simulation (labelled "Simulation" on the page), paused when off-screen.
 * 5. Reveals, current-section highlight in the nav, and pausing of endless loops off-screen.
 *
 * html.anim (and data-motion="full") is set in <head> unless the OS asks for reduced motion and the visitor has not switched animations on. Without it this file only wires
 * the theme and menu; the CSS shows every end state and nothing is pinned.
 */
(function () {
  'use strict';

  const root = document.documentElement;
  // The one place that decides whether scroll motion runs: the head script resolves the OS setting and the
  // visitor's own Animations choice into html[data-motion] before first paint.
  const motion = { enabled: () => root.dataset.motion === 'full' };
  const anim = motion.enabled();
  const dir = root.dir === 'rtl' ? -1 : 1;
  const clamp = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
  const easeIO = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
  const easeOut = (t) => 1 - Math.pow(1 - t, 3);
  const r4 = (v) => Math.round(v * 10000) / 10000;

  // ---- 1. Theme and menu ------------------------------------------------------------------
  const themeButton = document.querySelector('.theme-toggle');
  if (themeButton) {
    themeButton.addEventListener('click', () => {
      const theme = root.dataset.theme === 'dark' ? 'light' : 'dark';
      root.classList.add('theme-anim');
      root.dataset.theme = theme;
      setTimeout(() => root.classList.remove('theme-anim'), 520);
      const meta = document.querySelector('meta[name="theme-color"]');
      if (meta) meta.setAttribute('content', theme === 'dark' ? '#071a26' : '#ffffff');
      try { localStorage.setItem('riptide-theme', theme); } catch (e) { /* storage blocked */ }
    });
  }
  const motionButton = document.querySelector('.motion-toggle');
  if (motionButton) {
    motionButton.setAttribute('aria-pressed', String(motion.enabled()));
    motionButton.addEventListener('click', () => {
      const next = motion.enabled() ? 'off' : 'on';
      try { localStorage.setItem('riptide-motion', next); } catch (e) { location.search = '?motion=' + next; return; }
      location.reload(); // pinned chapters and listeners are set up once, so a clean reload is the robust switch
    });
  }
  const header = document.querySelector('.site-header');
  const navToggle = document.querySelector('.nav-toggle');
  if (header && navToggle) {
    const setOpen = (open) => {
      header.classList.toggle('is-open', open);
      navToggle.setAttribute('aria-expanded', String(open));
    };
    navToggle.addEventListener('click', () => setOpen(navToggle.getAttribute('aria-expanded') !== 'true'));
    header.querySelectorAll('.nav-menu a').forEach((a) => a.addEventListener('click', () => setOpen(false)));
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape') setOpen(false); });
  }

  const main = document.getElementById('main');
  const hero = document.getElementById('hero');
  if (!header || !main || !hero) {
    if (header) header.classList.add('is-solid');
    return;
  }

  // ---- 4. Hero simulation -----------------------------------------------------------------
  // A pretend download that loops: runs, verifies, completes, starts over. Static "completed" markup
  // is what visitors without motion see.
  (function () {
    const sim = hero.querySelector('[data-sim]');
    const spd = hero.querySelector('[data-o="speed"]');
    const sha = hero.querySelector('[data-o="sha"]');
    if (!sim || !anim) return;
    const pct = sim.querySelector('[data-o="pct"]');
    const state = sim.querySelector('[data-o="state"]');
    const fills = Array.from(sim.querySelectorAll('.sim-bar b'));
    const doneText = state.textContent;
    const stateText = { run: '', ver: '' };
    const lang = root.lang === 'ar';
    stateText.run = lang ? 'جارٍ التحميل' : 'Downloading';
    stateText.ver = lang ? 'جارٍ التحقق' : 'Verifying';
    // Eight connections with different speeds; the file is done when the slowest one finishes.
    const rates = [1, 0.78, 0.9, 0.66, 0.97, 0.72, 0.84, 0.6];
    const DL = 9, VERIFY = 1.1, HOLD = 4.2;
    let t0 = performance.now() + 1500;
    let first = true;
    let running = false;
    let timer = 0;
    let last = '';
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
      const key = phase + p + (spd ? speed.toFixed(1) : '');
      if (key !== last) {
        last = key;
        pct.textContent = `${phase === 'run' ? p : 100}%`;
        state.textContent = phase === 'run' ? stateText.run : phase === 'ver' ? stateText.ver : doneText;
        spd.textContent = phase === 'run' ? speed.toFixed(1) : '0.0';
        sha.classList.toggle('is-on', phase === 'done');
      }
      if (t > DL + VERIFY + HOLD) { first = false; t0 = now + 600; last = ''; sha.classList.remove('is-on'); fills.forEach((el) => { el.style.transform = 'scaleX(0)'; el.__s = ''; }); }
    };
    const tick = () => { draw(performance.now()); };
    const start = () => { if (!running && !document.hidden) { running = true; timer = setInterval(tick, 100); } };
    const stop = () => { running = false; clearInterval(timer); };
    if ('IntersectionObserver' in window) {
      new IntersectionObserver((es) => { es.forEach((e) => (e.isIntersecting ? start() : stop())); }).observe(sim);
    } else start();
    document.addEventListener('visibilitychange', () => { if (document.hidden) stop(); });
  })();

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
        set('--mx', r4(265.5 * (1 - lg)));
        set('--my', r4(22.7 * (1 - lg)));
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

  // ---- 2. The scroll loop -----------------------------------------------------------------
  const sections = Array.from(main.children).filter((el) => el.tagName === 'SECTION');
  const navLinks = Array.from(header.querySelectorAll('.nav-links a'));
  const navTargets = navLinks.map((a) => document.getElementById(a.getAttribute('href').slice(1)));
  let metrics = [];
  let vh = window.innerHeight;
  let heroH = 1;

  function measure() {
    vh = window.innerHeight;
    const y = window.scrollY;
    heroH = hero.offsetHeight || 1;
    metrics = sections.map((el) => {
      const r = el.getBoundingClientRect();
      const name = el.dataset.scene;
      const pin = el.querySelector('.pin');
      const pinned = !!pin && getComputedStyle(pin).position === 'sticky';
      return { el, top: r.top + y, h: r.height, tone: el.dataset.tone || 'light', name, scene: scenes[name], pin, pinned, p: NaN };
    });
  }

  let ticking = false;
  let tone = '';
  let current = -2;
  function frame() {
    ticking = false;
    const y = window.scrollY;
    if (y <= heroH) hero.style.setProperty('--hp', String(r4(clamp(y / heroH))));
    for (const m of metrics) {
      if (!m.name || m.name === 'hero') continue;
      if (!anim) continue;
      const off = y + vh <= m.top || y >= m.top + m.h;
      // Off-screen chapters are skipped, but parked at their end state once passed, so a jump past one
      // (anchor link, reload mid-page) shows it finished.
      if (off && m.p === (y >= m.top ? 1 : 0)) continue;
      const len = m.h - vh;
      let p;
      if (!m.pinned) p = 1;
      else if (off) p = y >= m.top ? 1 : -1;
      else p = y < m.top ? (y - m.top) / vh : len > 0 ? Math.min(1, (y - m.top) / len) : 1;
      p = r4(p);
      if (p === m.p) continue;
      m.p = p;
      if (m.pin) m.pin.style.setProperty('--p', String(p));
      if (m.scene && !(off && p === -1)) m.scene.update(clamp(p));
      else if (m.scene) m.scene.update(0);
    }
    const probe = y + 34;
    let t = 'light';
    for (const m of metrics) if (probe >= m.top && probe < m.top + m.h) { t = m.tone; break; }
    if (t !== tone) { tone = t; header.dataset.tone = t; }
    header.classList.toggle('is-solid', y > 8);

    // Current section in the nav.
    let idx = -1;
    navTargets.forEach((el, i) => { if (el) { const top = el.getBoundingClientRect().top; if (top <= vh * 0.4) idx = i; } });
    const last = navTargets[idx];
    if (last && last.getBoundingClientRect().bottom < vh * 0.4) idx = -1;
    if (idx !== current) { current = idx; navLinks.forEach((a, i) => { if (i === idx) a.setAttribute('aria-current', 'true'); else a.removeAttribute('aria-current'); }); }
  }
  const schedule = () => { if (!ticking) { ticking = true; requestAnimationFrame(frame); } };
  const remeasure = () => { if (!ready) return; measure(); metrics.forEach((m) => { m.p = NaN; }); schedule(); };

  // The first measurement waits for the first paint (the CSS already shows every start state), so loading
  // never forces an extra layout of the whole page.
  let ready = false;
  const start = () => { if (!ready) { ready = true; measure(); frame(); } };
  // Nothing needs measuring until the visitor scrolls (the CSS already shows every start state), so the first
  // layout read waits for the first scroll event; a reload mid-page or an anchor jump fires one immediately.
  // If nobody has scrolled a few seconds in, do it in idle time so the first scroll is not the one that pays.
  window.addEventListener('load', () => setTimeout(() => (window.requestIdleCallback ? requestIdleCallback(start) : start()), 4000));
  window.addEventListener('scroll', () => { if (ready) schedule(); else start(); }, { passive: true });
  window.addEventListener('resize', remeasure);
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(remeasure);
  if ('ResizeObserver' in window) {
    let lastH = 0;
    new ResizeObserver(() => { const h = document.body.scrollHeight; if (h !== lastH) { lastH = h; remeasure(); } }).observe(document.body);
  }

  // ---- 5. Reveals, and pausing of endless loops off-screen ---------------------------------
  if (!('IntersectionObserver' in window)) return;
  const loops = new IntersectionObserver((entries) => {
    entries.forEach((e) => e.target.classList.toggle('off', !e.isIntersecting));
  }, { rootMargin: '120px 0px' });
  document.querySelectorAll('.ch, .bp-video, .hero').forEach((el) => loops.observe(el));
  if (!anim) return;
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => {
      if (!e.isIntersecting) return;
      e.target.classList.add('in');
      io.unobserve(e.target);
    });
  }, { rootMargin: '0px 0px -10% 0px', threshold: 0.15 });
  document.querySelectorAll('.rv, .cta').forEach((el) => io.observe(el));
})();
