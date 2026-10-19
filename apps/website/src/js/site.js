/*
 * Riptide website. Vanilla JS, no dependencies, no network.
 *
 * 1. Theme toggle (light by default; the saved choice wins).
 * 2. Mobile navigation.
 * 3. Simulated downloads (hero card and "How it works"), pausing animations off screen,
 *    staggered scroll reveals.
 * 4. "How it works" step animations run only while their panel is on screen.
 * 5. Screenshot lightbox on the native <dialog>.
 *
 * The hero R is pure SVG + CSS and needs no script. Every animation respects
 * prefers-reduced-motion: simulations show their end state and the CSS is never started.
 */
(function () {
  'use strict';

  const root = document.documentElement;
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const isRtl = root.dir === 'rtl';

  // ---- 1. Theme (light by default; the visitor can switch to dark and the choice is remembered)
  function applyTheme(theme, persist) {
    root.dataset.theme = theme;
    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute('content', theme === 'dark' ? '#071a26' : '#ffffff');
    if (persist) {
      try { localStorage.setItem('riptide-theme', theme); } catch (e) { /* storage blocked */ }
    }
    document.dispatchEvent(new CustomEvent('riptide:theme', { detail: theme }));
  }
  const themeButton = document.querySelector('.theme-toggle');
  if (themeButton) {
    themeButton.addEventListener('click', () => applyTheme(root.dataset.theme === 'dark' ? 'light' : 'dark', true));
  }

  // ---- 2. Mobile navigation ---------------------------------------------------------------
  const header = document.querySelector('.site-header');
  const navToggle = document.querySelector('.nav-toggle');
  if (header && navToggle) {
    const setOpen = (open) => {
      header.classList.toggle('is-open', open);
      navToggle.setAttribute('aria-expanded', String(open));
    };
    navToggle.addEventListener('click', () => setOpen(navToggle.getAttribute('aria-expanded') !== 'true'));
    header.querySelectorAll('.nav-links a').forEach((a) => a.addEventListener('click', () => setOpen(false)));
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape') setOpen(false); });
    window.matchMedia('(min-width: 1100px)').addEventListener('change', (e) => { if (e.matches) setOpen(false); });
  }

  // ---- 3. Simulated downloads (hero card, How it works) ----------------------------------
  // A pure simulation: N blocks stand for ranges of one file, C connections fill them, and a
  // connection that runs out takes half of the largest range still left (work stealing). It
  // runs only while on screen and the tab is visible; with reduced motion it shows the end state.
  const nf = new Intl.NumberFormat(root.lang === 'ar' ? 'ar-u-nu-latn' : 'en', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const pf = new Intl.NumberFormat(root.lang === 'ar' ? 'ar-u-nu-latn' : 'en', { style: 'percent', maximumFractionDigits: 0 });

  function startSim(el) {
    const blocks = Array.from(el.querySelectorAll('.blk'));
    const N = blocks.length;
    const C = Number(el.dataset.conns) || 8;
    const total = Number(el.dataset.size) || 4.7;
    const out = {};
    el.querySelectorAll('[data-o]').forEach((n) => { out[n.dataset.o] = n; });
    const stealText = el.dataset.steal || '';
    const text = { run: el.dataset.tRun, verify: el.dataset.tVerify, done: el.dataset.tDone, start: el.dataset.tStart };
    const set = (k, v) => { if (out[k] && out[k].textContent !== v) out[k].textContent = v; };

    let filled, conns, phase, clock, running = false, raf = 0, last = 0, stealAt = 0;

    function reset() {
      blocks.forEach((b) => { b.classList.remove('on', 'hd'); });
      filled = new Array(N).fill(false);
      const per = N / C;
      conns = [];
      for (let i = 0; i < C; i++) {
        conns.push({ pos: i * per, end: (i + 1) * per, rate: (N / C / 7) * (0.55 + 0.9 * (((i * 5) % 8) / 7)), idle: false });
      }
      phase = 'run'; clock = 0; stealAt = 0;
      el.classList.remove('is-verifying', 'is-done');
      render(true);
      set('event', text.start || ' ');
    }

    function render() {
      let n = 0;
      for (let i = 0; i < N; i++) if (filled[i]) n++;
      const f = n / N;
      set('bytes', `${nf.format(total * f)} / ${nf.format(total)} GB`);
      set('pct', pf.format(f));
      set('state', phase === 'run' ? text.run : phase === 'verify' ? text.verify : text.done);
    }

    function fill(i) {
      if (i >= 0 && i < N && !filled[i]) { filled[i] = true; blocks[i].classList.add('on'); }
    }

    function stepRun(dt) {
      const heads = new Set();
      for (let c = 0; c < conns.length; c++) {
        const k = conns[c];
        if (k.idle) continue;
        const wobble = 1 + 0.25 * Math.sin(clock * 1.7 + c * 2.1);
        const from = Math.floor(k.pos);
        k.pos = Math.min(k.end, k.pos + k.rate * wobble * dt);
        for (let i = from; i < Math.floor(k.pos); i++) fill(i);
        if (k.pos >= k.end - 1e-6) {
          // finished: steal half of the largest remaining range
          let victim = -1, most = 0;
          for (let v = 0; v < conns.length; v++) {
            const rem = conns[v].end - conns[v].pos;
            if (v !== c && !conns[v].idle && rem > most) { most = rem; victim = v; }
          }
          if (victim >= 0 && most >= 4) {
            const v = conns[victim];
            const mid = Math.ceil(v.pos + most / 2);
            k.pos = mid; k.end = v.end; v.end = mid;
            if (stealText && clock - stealAt > 0.8) {
              set('event', stealText.replace('%a', String(c + 1)).replace('%b', String(victim + 1)));
              stealAt = clock;
            }
          } else k.idle = true;
        }
        if (!k.idle) heads.add(Math.min(N - 1, Math.floor(k.pos)));
      }
      for (let i = 0; i < N; i++) blocks[i].classList.toggle('hd', heads.has(i) && !filled[i]);
      if (filled.every(Boolean)) { phase = 'verify'; clock = 0; el.classList.add('is-verifying'); blocks.forEach((b) => b.classList.remove('hd')); }
    }

    function frame(now) {
      raf = 0;
      if (!running) return;
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      clock += dt;
      if (phase === 'run') stepRun(dt);
      else if (phase === 'verify' && clock > 1.2) { phase = 'done'; clock = 0; el.classList.remove('is-verifying'); el.classList.add('is-done'); }
      else if (phase === 'done' && clock > 3.6) { reset(); }
      render();
      raf = requestAnimationFrame(frame);
    }
    function play() {
      if (running || reduceMotion.matches) return;
      running = true; last = performance.now();
      if (!raf) raf = requestAnimationFrame(frame);
    }
    function pause() { running = false; if (raf) { cancelAnimationFrame(raf); raf = 0; } }

    reset();
    if (reduceMotion.matches) {
      blocks.forEach((b, i) => { filled[i] = true; b.classList.add('on'); });
      phase = 'done'; el.classList.add('is-done'); render(); set('event', ' ');
      return;
    }
    let visible = true;
    const sync = () => (visible && !document.hidden ? play() : pause());
    if ('IntersectionObserver' in window) {
      new IntersectionObserver((entries) => { visible = entries[entries.length - 1].isIntersecting; sync(); }, { rootMargin: '80px' }).observe(el);
    }
    document.addEventListener('visibilitychange', sync);
    sync();
  }
  document.querySelectorAll('[data-sim]').forEach(startSim);

  // The hero stage and the band pause their CSS animations while off screen.
  if ('IntersectionObserver' in window) {
    const pausable = document.querySelectorAll('.stage, .band');
    const po = new IntersectionObserver((entries) => {
      for (const e of entries) e.target.classList.toggle('is-off', !e.isIntersecting);
    }, { rootMargin: '60px' });
    pausable.forEach((n) => po.observe(n));
  }

  // ---- 3b. Scroll reveals (staggered inside [data-stagger] groups) ------------------------
  const reveals = document.querySelectorAll('.reveal');
  if (reveals.length) {
    document.querySelectorAll('[data-stagger]').forEach((group) => {
      Array.from(group.children).forEach((child, i) => child.style.setProperty('--i', String(Math.min(i, 6))));
    });
    if ('IntersectionObserver' in window && !reduceMotion.matches) {
      const ro = new IntersectionObserver((entries) => {
        for (const e of entries) if (e.isIntersecting) { e.target.classList.add('is-in'); ro.unobserve(e.target); }
      }, { threshold: 0.12, rootMargin: '0px 0px -6% 0px' });
      reveals.forEach((n) => ro.observe(n));
    } else {
      reveals.forEach((n) => n.classList.add('is-in'));
    }
  }

  // ---- 4. Step animations only while visible ----------------------------------------------
  const visuals = document.querySelectorAll('.step-visual');
  if (visuals.length && 'IntersectionObserver' in window && !reduceMotion.matches) {
    const io = new IntersectionObserver((entries) => {
      for (const e of entries) e.target.classList.toggle('in-view', e.isIntersecting);
    }, { threshold: 0.3 });
    visuals.forEach((v) => io.observe(v));
  } else {
    visuals.forEach((v) => v.classList.add('in-view'));
  }

  // ---- 5. Lightbox ------------------------------------------------------------------------
  const lightbox = document.getElementById('lightbox');
  const triggers = Array.from(document.querySelectorAll('.shot-open'));
  if (lightbox && triggers.length && typeof lightbox.showModal === 'function') {
    const img = lightbox.querySelector('#lightbox-img');
    const cap = lightbox.querySelector('#lightbox-cap');
    let index = 0;
    let opener = null;

    function show(i) {
      index = (i + triggers.length) % triggers.length;
      const source = triggers[index].querySelector('img');
      const caption = triggers[index].closest('figure').querySelector('figcaption');
      img.src = source.dataset.full || source.currentSrc || source.src;
      img.alt = source.alt;
      cap.textContent = caption ? caption.textContent : '';
    }
    function open(i, button) {
      opener = button;
      show(i);
      lightbox.showModal();
      lightbox.querySelector('.lightbox-close').focus();
    }
    triggers.forEach((button, i) => button.addEventListener('click', () => open(i, button)));
    lightbox.querySelector('.lightbox-close').addEventListener('click', () => lightbox.close());
    lightbox.querySelector('.lightbox-prev').addEventListener('click', () => show(index - 1));
    lightbox.querySelector('.lightbox-next').addEventListener('click', () => show(index + 1));
    lightbox.addEventListener('click', (e) => { if (e.target === lightbox) lightbox.close(); });
    lightbox.addEventListener('keydown', (e) => {
      const forward = isRtl ? 'ArrowLeft' : 'ArrowRight';
      const back = isRtl ? 'ArrowRight' : 'ArrowLeft';
      if (e.key === forward) { e.preventDefault(); show(index + 1); }
      if (e.key === back) { e.preventDefault(); show(index - 1); }
    });
    lightbox.addEventListener('close', () => { img.removeAttribute('src'); if (opener) opener.focus(); });
  }
})();
