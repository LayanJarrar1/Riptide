/*
 * Riptide website. Vanilla JS, no dependencies, no network.
 *
 * 1. Theme toggle (light by default; a saved choice wins) and the mobile menu.
 * 2. One scroll loop: a passive scroll listener schedules at most one requestAnimationFrame,
 *    which computes a progress value per pinned scene from cached offsets and writes it as
 *    --p (plus a few derived values for the hero). CSS turns those into transforms, opacity,
 *    stroke-dashoffset and clip-path. Nothing reads layout inside the frame.
 * 3. Scenes: hero logo (streams draw the R, the wordmark rises, the logo lands in the header),
 *    the eight-connection race, the power cut and resume, the SHA-256 check.
 * 4. Reveals and counters for the unpinned blocks (IntersectionObserver).
 *
 * html.anim is set in <head> unless the visitor prefers reduced motion. Without it this file
 * only wires the theme and menu; the CSS shows every end state and nothing is pinned.
 */
(function () {
  'use strict';

  const root = document.documentElement;
  const anim = root.classList.contains('anim');
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
      root.dataset.theme = theme;
      const meta = document.querySelector('meta[name="theme-color"]');
      if (meta) meta.setAttribute('content', theme === 'dark' ? '#071a26' : '#ffffff');
      try { localStorage.setItem('riptide-theme', theme); } catch (e) { /* storage blocked */ }
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
  if (!header || !main || !document.querySelector('.hero')) {
    if (header) header.classList.add('is-solid');
    return;
  }

  // ---- 3. Scenes --------------------------------------------------------------------------
  const scenes = {};

  // Hero: the three streams of the real mark are revealed through masks (never recoloured),
  // spiral into place, the wordmark rises, then the whole logo flies into the header slot.
  (function () {
    const pin = document.querySelector('.hero-pin');
    const fly = pin.querySelector('.hl-fly');
    const hl = pin.querySelector('.hl');
    const brand = header.querySelector('.brand');
    let msd = 1.8;
    const set = (k, v) => pin.style.setProperty(k, v);
    scenes.hero = {
      measure() {
        msd = parseFloat(getComputedStyle(pin).getPropertyValue('--msd')) || 1.8;
        const b = brand.getBoundingClientRect();
        const w = hl.offsetWidth;
        set('--fx', r4(b.left + b.width / 2 - pin.clientWidth / 2));
        set('--fy', r4(b.top + b.height / 2 - pin.clientHeight / 2));
        set('--fs', r4(b.width / w));
      },
      update(p) {
        const q = clamp(p);
        const spin = easeIO(clamp(q / 0.5));
        for (let i = 0; i < 3; i++) {
          const base = [0.3, 0.33, 0.37][i];
          set(`--s${i + 1}`, r4(base + (1 - base) * easeOut(clamp((q - 0.02 - i * 0.05) / 0.42))));
          set(`--r${i + 1}`, r4(-Math.sin(Math.PI * clamp(q / 0.52)) * (22 + 16 * i)));
        }
        const lg = easeIO(clamp((q - 0.56) / 0.16));
        const draw = msd * (1.12 - 0.12 * spin);
        const f = easeIO(clamp((q - 0.8) / 0.2));
        const hd = clamp((q - 0.975) / 0.025);
        set('--q', r4(q));
        set('--h', r4(clamp(q / 0.14)));
        set('--c', r4(easeIO(clamp((q - 0.04) / 0.3))));
        set('--ms', r4(draw + (1 - draw) * lg));
        set('--mx', r4(265.5 * (1 - lg)));
        set('--my', r4(22.7 * (1 - lg)));
        set('--lg', r4(clamp((q - 0.6) / 0.16)));
        set('--ta', r4(clamp((q - 0.68) / 0.06) * (1 - clamp((q - 0.79) / 0.04))));
        set('--f', r4(f));
        fly.style.opacity = String(1 - hd);
        header.style.setProperty('--hd', String(hd));
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
    // Build the DOM: one element per piece in its lane, and a copy in the file bar.
    const make = (pc) => {
      const el = document.createElement('span');
      el.className = pc.t0 > 0 ? 'pc stolen' : 'pc';
      el.style.setProperty('--a', String(pc.a));
      el.style.setProperty('--b', String(pc.b));
      el.appendChild(document.createElement('i'));
      return el;
    };
    lanes.forEach((lane) => lane.querySelectorAll('.pc').forEach((n) => n.remove()));
    filebar.textContent = '';
    pieces.forEach((pc) => {
      const a = make(pc), b = make(pc);
      lanes[pc.c].insertBefore(a, lanes[pc.c].querySelector('.hd'));
      filebar.appendChild(b);
      pc.els = [a.firstChild, b.firstChild];
      pc.last = -1;
    });
    const heads = lanes.map((lane) => lane.querySelector('.hd'));
    scenes.speed = {
      update(p) {
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

  // Resume: download, power cut, resume from the same byte, complete.
  (function () {
    const section = document.getElementById('resume');
    if (!section) return;
    const rs = section.querySelector('.rs');
    const fill = rs.querySelector('.rs-fill');
    const bytes = rs.querySelector('[data-o="bytes"]');
    const TOTAL = 5046586572, CUT = 0.62;
    const nf = new Intl.NumberFormat('en-US');
    rs.querySelector('[data-o="at"]').textContent = nf.format(Math.round(CUT * TOTAL));
    scenes.resume = {
      update(p) {
        let f, phase, cut = 0;
        if (p < 0.4) { f = CUT * easeIO(clamp(p / 0.4)); phase = 'run'; }
        else if (p < 0.52) { f = CUT; phase = 'cut'; cut = clamp((p - 0.4) / 0.025) * (1 - clamp((p - 0.45) / 0.05)) * 0.9; }
        else if (p < 0.92) { f = CUT + (1 - CUT) * easeIO(clamp((p - 0.58) / 0.34)); phase = 'res'; }
        else { f = 1; phase = 'done'; }
        if (rs.dataset.phase !== phase) rs.dataset.phase = phase;
        rs.style.setProperty('--cut', String(r4(cut)));
        fill.style.transform = `scaleX(${r4(f)})`;
        bytes.textContent = nf.format(Math.round(f * TOTAL));
      },
    };
  })();

  // Verify: the digest resolves left to right out of scrambled hex.
  (function () {
    const section = document.getElementById('verified');
    if (!section) return;
    const ok = section.querySelector('[data-o="ok"]');
    const scr = section.querySelector('[data-o="scr"]');
    const hash = ok.textContent.trim();
    const hex = '0123456789abcdef';
    scenes.verify = {
      update(p) {
        const n = Math.round(clamp((p + 0.1) / 0.5) * hash.length);
        const seed = Math.floor(p * 120);
        let s = '';
        for (let i = n; i < hash.length; i++) s += hex[(i * 7 + seed * 13 + ((i * i) % 11)) & 15];
        ok.textContent = hash.slice(0, n);
        scr.textContent = s;
      },
    };
  })();

  // ---- 2. The scroll loop -----------------------------------------------------------------
  const sections = Array.from(main.children).filter((el) => el.tagName === 'SECTION');
  let metrics = [];
  let vh = window.innerHeight;
  let heroEnd = 0;

  function measure() {
    vh = window.innerHeight;
    const y = window.scrollY;
    metrics = sections.map((el) => {
      const r = el.getBoundingClientRect();
      const name = el.dataset.scene;
      return { el, top: r.top + y, h: r.height, tone: el.dataset.tone || 'light', name, scene: scenes[name], pin: el.querySelector('.pin'), p: NaN };
    });
    heroEnd = metrics.length ? metrics[0].top + metrics[0].h - vh : 0;
    if (anim) scenes.hero.measure();
  }

  let ticking = false;
  let tone = '';
  function frame() {
    ticking = false;
    const y = window.scrollY;
    if (anim) {
      for (const m of metrics) {
        if (!m.name || y + vh <= m.top || y >= m.top + m.h) continue;
        const len = m.h - vh;
        let p = y < m.top ? (y - m.top) / vh : len > 0 ? Math.min(1, (y - m.top) / len) : 1;
        p = r4(p);
        if (p === m.p) continue;
        m.p = p;
        if (m.pin && m.name !== 'hero') m.pin.style.setProperty('--p', String(p));
        if (m.scene) m.scene.update(p);
      }
    }
    const probe = y + 34;
    let t = 'light';
    for (const m of metrics) if (probe >= m.top && probe < m.top + m.h) { t = m.tone; break; }
    if (t !== tone) { tone = t; header.dataset.tone = t; }
    header.classList.toggle('is-solid', anim ? y > heroEnd - 2 : y > 8);
  }
  const schedule = () => { if (!ticking) { ticking = true; requestAnimationFrame(frame); } };
  const remeasure = () => { measure(); metrics.forEach((m) => { m.p = NaN; }); schedule(); };

  measure();
  frame();
  window.addEventListener('scroll', schedule, { passive: true });
  window.addEventListener('resize', remeasure);
  window.addEventListener('load', remeasure);
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(remeasure);
  if ('ResizeObserver' in window) {
    let last = 0;
    new ResizeObserver(() => { const h = document.body.scrollHeight; if (h !== last) { last = h; remeasure(); } }).observe(document.body);
  }

  // ---- 4. Reveals and counters ------------------------------------------------------------
  if (!anim || !('IntersectionObserver' in window)) return;
  const count = (el) => {
    const to = Number(el.dataset.count) || 0;
    const from = to === 0 ? 9 : 0;
    const t0 = performance.now();
    const step = (now) => {
      const k = easeOut(clamp((now - t0) / 1300));
      el.textContent = String(Math.round(from + (to - from) * k));
      if (k < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  };
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => {
      if (!e.isIntersecting) return;
      e.target.classList.add('in');
      e.target.querySelectorAll('[data-count]').forEach(count);
      io.unobserve(e.target);
    });
  }, { rootMargin: '0px 0px -12% 0px', threshold: 0.2 });
  document.querySelectorAll('.rv, .cta').forEach((el) => io.observe(el));
})();
