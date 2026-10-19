/*
 * Riptide website. Vanilla JS, no dependencies, no network.
 *
 * 1. Theme toggle (saved choice, otherwise follows the system).
 * 2. Mobile navigation.
 * 3. The hero "current": segment stripes flow and assemble into a finished download bar.
 * 4. "How it works" animations run only while their panel is on screen.
 * 5. Screenshot lightbox on the native <dialog>.
 *
 * Every animation respects prefers-reduced-motion: the canvas draws one still frame and
 * the CSS animations are never started.
 */
(function () {
  'use strict';

  const root = document.documentElement;
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const isRtl = root.dir === 'rtl';

  // ---- 1. Theme (dark first; the visitor can switch to light and the choice is remembered) --
  function applyTheme(theme, persist) {
    root.dataset.theme = theme;
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

  // ---- 3. The current ---------------------------------------------------------------------
  function hexToRgb(hex) {
    const h = hex.trim().replace('#', '');
    const n = parseInt(h.length === 3 ? h.split('').map((c) => c + c).join('') : h, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  function rgba(rgb, a) { return `rgba(${rgb[0]},${rgb[1]},${rgb[2]},${a})`; }
  function cssVar(name) { return getComputedStyle(root).getPropertyValue(name).trim(); }
  function roundRect(ctx, x, y, w, h, r) {
    r = Math.min(r, h / 2, Math.abs(w) / 2);
    if (w <= 0) return;
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.lineTo(x + w - r, y);
    ctx.arcTo(x + w, y, x + w, y + r, r);
    ctx.lineTo(x + w, y + h - r);
    ctx.arcTo(x + w, y + h, x + w - r, y + h, r);
    ctx.lineTo(x + r, y + h);
    ctx.arcTo(x, y + h, x, y + h - r, r);
    ctx.lineTo(x, y + r);
    ctx.arcTo(x, y, x + r, y, r);
    ctx.closePath();
  }
  const easeOut = (t) => 1 - Math.pow(1 - t, 3);
  const clamp01 = (t) => Math.max(0, Math.min(1, t));

  function startCurrent(canvas) {
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const dir = isRtl ? -1 : 1;
    const label = canvas.dataset.label || '';
    const doneText = canvas.dataset.done || '';
    const fontFamily = getComputedStyle(document.body).fontFamily;

    let w = 0, h = 0;
    let colors = {};
    let lanes = [];
    let bar = null;
    let drops = [];
    let raf = 0, last = 0, inView = true, pageVisible = !document.hidden;
    let dropClock = 0;

    function readColors() {
      const light = root.dataset.theme !== 'dark';
      colors = {
        accent: hexToRgb(cssVar('--color-accent')),
        accentLight: hexToRgb(cssVar(light ? '--color-accent-dark' : '--color-accent-light')),
        done: hexToRgb(cssVar('--fill-done')),
        track: cssVar('--track'),
        text: cssVar('--color-text-secondary'),
        textStrong: cssVar('--color-text'),
        success: cssVar('--color-success'),
        light,
      };
    }

    // One flowing lane of stripes. Stripes enter at the start edge and leave at the end edge.
    function makeLane(index, count) {
      const t = count === 1 ? 0.5 : index / (count - 1);
      const lane = {
        y: 0,
        speed: 14 + 22 * (0.35 + 0.65 * Math.abs(Math.sin(index * 1.7 + 0.4))),
        amp: 3 + 4 * (1 - Math.abs(t - 0.5) * 2),
        wave: 0.7 + index * 0.17,
        phase: index * 1.3,
        alpha: 0.3 + 0.5 * (1 - Math.abs(t - 0.5) * 1.4),
        stripes: [],
      };
      return lane;
    }
    function seedStripes(lane, random) {
      lane.stripes = [];
      let x = random ? -Math.random() * w * 0.2 : 0;
      while (x < w * 1.1) {
        const len = w * (0.06 + Math.random() * 0.16);
        lane.stripes.push({ x, len });
        x += len + w * (0.03 + Math.random() * 0.08);
      }
    }

    function makeBar() {
      const x0 = w * 0.08, x1 = w * 0.92;
      const n = 6, gap = 3;
      const segLen = (x1 - x0 - gap * (n - 1)) / n;
      const segs = [];
      for (let i = 0; i < n; i++) {
        const s = x0 + i * (segLen + gap);
        segs.push({ x0: s, x1: s + segLen, filled: 0, rate: 0, done: false, flash: 0 });
      }
      const base = segLen / 7; // px per second: a lone segment takes ~7 s
      segs.forEach((s, i) => { s.rate = base * (0.55 + 0.9 * ((i * 7) % 5) / 4); });
      return { x0, x1, gap, segs, phase: 'filling', t: 0, fade: 1, percent: 0 };
    }

    function layout() {
      const laneCount = w < 420 ? 5 : 6;
      const top = h * 0.1, bottom = h * 0.6;
      lanes = [];
      for (let i = 0; i < laneCount; i++) {
        const lane = makeLane(i, laneCount);
        lane.y = top + (bottom - top) * (laneCount === 1 ? 0.5 : i / (laneCount - 1));
        seedStripes(lane, true);
        lanes.push(lane);
      }
      bar = makeBar();
      drops = [];
    }

    function resize() {
      const rect = canvas.getBoundingClientRect();
      if (!rect.width || !rect.height) return;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      w = rect.width; h = rect.height;
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      layout();
      if (reduceMotion.matches) drawStill();
    }

    function stepBar(dt) {
      const b = bar;
      b.t += dt;
      if (b.phase === 'filling') {
        let allDone = true;
        for (const s of b.segs) {
          if (s.done) continue;
          s.filled = Math.min(s.x1 - s.x0, s.filled + s.rate * dt);
          if (s.filled >= s.x1 - s.x0 - 0.01) {
            s.done = true;
            s.flash = 1;
            // Work stealing: take half of the largest remaining segment.
            let victim = null, best = 0;
            for (const o of b.segs) {
              if (o.done) continue;
              const remain = (o.x1 - o.x0) - o.filled;
              if (remain > best) { best = remain; victim = o; }
            }
            if (victim && best > (b.x1 - b.x0) * 0.08 && b.segs.length < 14) {
              const mid = victim.x0 + victim.filled + best / 2;
              const child = { x0: mid, x1: victim.x1, filled: 0, rate: s.rate, done: false, flash: 0 };
              victim.x1 = mid;
              b.segs.splice(b.segs.indexOf(victim) + 1, 0, child);
            }
          } else {
            allDone = false;
          }
        }
        let total = 0, filled = 0;
        for (const s of b.segs) { total += s.x1 - s.x0; filled += s.filled; }
        b.percent = total ? filled / total : 0;
        if (allDone) { b.phase = 'verifying'; b.t = 0; }
      } else if (b.phase === 'verifying') {
        if (b.t > 1.1) { b.phase = 'done'; b.t = 0; }
      } else if (b.phase === 'done') {
        if (b.t > 2.6) { b.phase = 'reset'; b.t = 0; }
      } else if (b.phase === 'reset') {
        b.fade = 1 - clamp01(b.t / 0.5);
        if (b.t > 0.7) { const nb = makeBar(); nb.fade = 0; bar = nb; bar.phase = 'filling'; bar.fadeIn = true; }
      }
      if (bar.fadeIn) { bar.fade = Math.min(1, bar.fade + dt * 2.5); if (bar.fade >= 1) bar.fadeIn = false; }
      for (const s of bar.segs) if (s.flash > 0) s.flash = Math.max(0, s.flash - dt * 1.6);
    }

    function stepLanes(dt, time) {
      for (const lane of lanes) {
        for (const s of lane.stripes) s.x += lane.speed * dt;
        lane.stripes = lane.stripes.filter((s) => s.x < w * 1.15);
        const firstX = lane.stripes.length ? Math.min(...lane.stripes.map((s) => s.x)) : w * 0.1;
        if (firstX > w * 0.02) {
          const len = w * (0.06 + Math.random() * 0.16);
          lane.stripes.push({ x: firstX - len - w * (0.03 + Math.random() * 0.08), len });
        }
      }
      // Occasionally a stripe leaves its lane and drops into the segment it is feeding.
      dropClock += dt;
      if (bar.phase === 'filling' && dropClock > 0.7 && drops.length < 3) {
        dropClock = 0;
        const active = bar.segs.filter((s) => !s.done);
        if (active.length) {
          const seg = active[Math.floor(Math.random() * active.length)];
          const lane = lanes[Math.floor(Math.random() * lanes.length)];
          drops.push({ x: seg.x0 + seg.filled, y0: lane.y, len: w * 0.05 + Math.random() * w * 0.05, t: 0, seg });
        }
      }
      for (const d of drops) d.t += dt / 0.9;
      drops = drops.filter((d) => d.t < 1);
    }

    function laneX(x) { return dir === 1 ? x : w - x; }

    function drawLanes(time, still) {
      const stripeH = Math.max(7, Math.min(12, w * 0.022));
      for (const lane of lanes) {
        for (const s of lane.stripes) {
          const cx = s.x + s.len / 2;
          const edge = clamp01(cx / (w * 0.18)) * clamp01((w - cx) / (w * 0.18));
          if (edge <= 0) continue;
          const wobble = still ? 0 : Math.sin((s.x / w) * Math.PI * 2 * lane.wave + time * 0.9 + lane.phase) * lane.amp;
          const px = dir === 1 ? s.x : w - s.x - s.len;
          ctx.fillStyle = rgba(colors.accent, lane.alpha * edge);
          roundRect(ctx, px, lane.y + wobble - stripeH / 2, s.len, stripeH, stripeH / 2);
          ctx.fill();
        }
      }
      for (const d of drops) {
        const e = easeOut(d.t);
        const y = d.y0 + (bar ? (h * 0.78 - d.y0) * e : 0);
        const x = d.seg.x0 + d.seg.filled;
        const px = dir === 1 ? x - d.len * (1 - e) : w - x - d.len * e;
        ctx.fillStyle = rgba(colors.accentLight, 0.75 * (1 - d.t * d.t));
        roundRect(ctx, px, y - 5, d.len, 10, 5);
        ctx.fill();
      }
    }

    function drawBar() {
      const b = bar;
      const y = h * 0.78;
      const barH = Math.max(14, Math.min(22, w * 0.04));
      const r = barH / 2;
      ctx.save();
      ctx.globalAlpha = b.fade;

      // track
      ctx.fillStyle = colors.track;
      roundRect(ctx, laneX(b.x0) - (dir === 1 ? 0 : (b.x1 - b.x0)), y - r, b.x1 - b.x0, barH, r);
      ctx.fill();

      const doneLook = b.phase === 'done' || b.phase === 'reset';
      const verifying = b.phase === 'verifying';
      for (const s of b.segs) {
        const len = s.x1 - s.x0;
        const sx = dir === 1 ? s.x0 : w - s.x1;
        if (doneLook) {
          ctx.fillStyle = rgba(colors.done, 1);
          roundRect(ctx, sx, y - r + 1, len, barH - 2, r - 1);
          ctx.fill();
          continue;
        }
        // segment boundary
        ctx.fillStyle = rgba(colors.accent, 0.1);
        roundRect(ctx, sx, y - r + 1, len, barH - 2, r - 1);
        ctx.fill();
        // fill
        const fx = dir === 1 ? s.x0 : w - s.x0 - s.filled;
        ctx.fillStyle = verifying
          ? rgba(colors.done, 0.6 + 0.4 * Math.abs(Math.sin(b.t * 6)))
          : rgba(s.done ? colors.done : colors.accent, s.done ? 1 : 0.92);
        roundRect(ctx, fx, y - r + 1, s.filled, barH - 2, r - 1);
        ctx.fill();
        if (s.flash > 0) {
          ctx.fillStyle = rgba(colors.accentLight, 0.5 * s.flash);
          roundRect(ctx, sx, y - r + 1, len, barH - 2, r - 1);
          ctx.fill();
        }
      }

      // labels
      const fontSize = Math.max(12, Math.min(14, w * 0.028));
      ctx.font = `500 ${fontSize}px ${fontFamily}`;
      ctx.textBaseline = 'top';
      const ty = y + r + 12;
      const startX = laneX(b.x0), endX = laneX(b.x1);
      ctx.textAlign = dir === 1 ? 'left' : 'right';
      if (doneLook) {
        ctx.fillStyle = colors.success;
        // check mark
        ctx.strokeStyle = colors.success;
        ctx.lineWidth = 2;
        ctx.lineCap = 'round';
        ctx.lineJoin = 'round';
        const cx = startX + dir * 7, cy = ty + fontSize * 0.55;
        ctx.beginPath();
        ctx.moveTo(cx - dir * 5, cy);
        ctx.lineTo(cx - dir * 1.5, cy + 3.5);
        ctx.lineTo(cx + dir * 5.5, cy - 4);
        ctx.stroke();
        ctx.fillText(doneText, startX + dir * 20, ty);
      } else {
        ctx.fillStyle = colors.text;
        ctx.fillText(label, startX, ty);
      }
      ctx.textAlign = dir === 1 ? 'right' : 'left';
      ctx.fillStyle = doneLook ? colors.success : colors.textStrong;
      ctx.font = `600 ${fontSize}px ${fontFamily}`;
      const pct = doneLook ? 100 : Math.floor(b.percent * 100);
      ctx.fillText(`${pct}%`, endX, ty);
      ctx.restore();
    }

    function drawStill() {
      ctx.clearRect(0, 0, w, h);
      for (const lane of lanes) seedStripes(lane, true);
      drawLanes(0, true);
      bar = makeBar();
      for (const s of bar.segs) { s.filled = s.x1 - s.x0; s.done = true; }
      bar.phase = 'done';
      bar.percent = 1;
      drawBar();
    }

    function frame(now) {
      raf = 0;
      if (!inView || !pageVisible || reduceMotion.matches) return;
      const dt = Math.min(0.05, last ? (now - last) / 1000 : 0.016);
      last = now;
      stepLanes(dt, now / 1000);
      stepBar(dt);
      ctx.clearRect(0, 0, w, h);
      drawLanes(now / 1000, false);
      drawBar();
      raf = requestAnimationFrame(frame);
    }
    function schedule() {
      if (reduceMotion.matches) { drawStill(); return; }
      if (!raf && inView && pageVisible) { last = 0; raf = requestAnimationFrame(frame); }
    }

    readColors();
    resize();
    schedule();

    new ResizeObserver(() => { resize(); schedule(); }).observe(canvas);
    if ('IntersectionObserver' in window) {
      new IntersectionObserver((entries) => { inView = entries[0].isIntersecting; schedule(); }, { threshold: 0.05 }).observe(canvas);
    }
    document.addEventListener('visibilitychange', () => { pageVisible = !document.hidden; schedule(); });
    document.addEventListener('riptide:theme', () => { readColors(); if (reduceMotion.matches) drawStill(); });
    reduceMotion.addEventListener('change', () => { if (raf) cancelAnimationFrame(raf); raf = 0; schedule(); });
  }

  const canvas = document.getElementById('current');
  if (canvas) startCurrent(canvas);

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
