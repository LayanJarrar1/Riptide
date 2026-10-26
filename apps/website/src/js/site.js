/*
 * Riptide website. Vanilla JS, no dependencies, no network.
 *
 * 1. Theme toggle (light by default; a saved choice wins) and the mobile menu.
 * 2. Reveals: elements with .rv get .in once, when they come into view (CSS does the motion).
 * 3. The hero simulation (pure CSS) is paused while off-screen or in a hidden tab.
 * 4. The header gets a hairline once the page scrolls, and the menu marks the section in view.
 * html.js-ready is added only after a successful init; every hidden start state in the CSS depends on it.
 */
(function () {
  'use strict';

  const root = document.documentElement;
  const store = {
    get(k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
    set(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* storage blocked */ } },
  };
  const safe = (name, fn) => { try { fn(); } catch (e) { console.warn('[riptide] ' + name + ' skipped:', e); } };
  window.addEventListener('error', (e) => {
    if (e.filename && e.filename.indexOf('site.js') < 0) return;
    root.classList.remove('js-ready');
    root.classList.add('rv-all');
  });

  // ---- 1. Theme and menu ---------------------------------------------------------------------
  safe('theme', () => {
    const button = document.querySelector('.theme-toggle');
    if (!button) return;
    button.addEventListener('click', () => {
      const theme = root.dataset.theme === 'dark' ? 'light' : 'dark';
      root.dataset.theme = theme;
      const meta = document.querySelector('meta[name="theme-color"]');
      if (meta) meta.setAttribute('content', theme === 'dark' ? '#071a26' : '#ffffff');
      store.set('riptide-theme', theme);
    });
  });
  const header = document.querySelector('.site-header');
  safe('menu', () => {
    const toggle = document.querySelector('.nav-toggle');
    if (!header || !toggle) return;
    const setOpen = (open) => {
      header.classList.toggle('is-open', open);
      toggle.setAttribute('aria-expanded', String(open));
    };
    toggle.addEventListener('click', () => setOpen(toggle.getAttribute('aria-expanded') !== 'true'));
    header.querySelectorAll('.nav-menu a').forEach((a) => a.addEventListener('click', () => setOpen(false)));
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape') setOpen(false); });
  });

  const io = 'IntersectionObserver' in window;

  // ---- 2. Reveals ----------------------------------------------------------------------------
  safe('reveals', () => {
    const items = document.querySelectorAll('.rv');
    if (!io) { items.forEach((el) => el.classList.add('in')); return; }
    const seen = new IntersectionObserver((entries) => {
      entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add('in'); seen.unobserve(e.target); } });
    }, { rootMargin: '0px 0px -8% 0px', threshold: 0.12 });
    items.forEach((el) => seen.observe(el));
    // A jump to an anchor shows everything above it at once, so nothing pops in behind the reader.
    const showAbove = () => {
      const y = window.scrollY + window.innerHeight;
      items.forEach((el) => { if (el.getBoundingClientRect().top + window.scrollY < y) el.classList.add('in'); });
    };
    window.addEventListener('hashchange', showAbove);
    if (location.hash) showAbove();
  });

  // ---- 3. Hero simulation pause --------------------------------------------------------------
  safe('simulation', () => {
    const sim = document.querySelector('.sim');
    if (!sim) return;
    let visible = true;
    const apply = () => sim.classList.toggle('paused', !visible || document.hidden);
    if (io) new IntersectionObserver((es) => { visible = es[0].isIntersecting; apply(); }).observe(sim);
    document.addEventListener('visibilitychange', apply);
  });

  // ---- 4. Header hairline and current section -------------------------------------------------
  safe('header', () => {
    if (!header) return;
    const solid = () => header.classList.toggle('is-solid', window.scrollY > 8);
    solid();
    window.addEventListener('scroll', solid, { passive: true });
    if (!io) return;
    const links = new Map();
    header.querySelectorAll('.nav-links a[href^="#"]').forEach((a) => {
      const target = document.getElementById(a.getAttribute('href').slice(1));
      if (target) links.set(target, a);
    });
    const spy = new IntersectionObserver((entries) => {
      entries.forEach((e) => {
        const a = links.get(e.target);
        if (!a) return;
        if (e.isIntersecting) { links.forEach((l) => l.removeAttribute('aria-current')); a.setAttribute('aria-current', 'true'); }
        else if (a.getAttribute('aria-current')) a.removeAttribute('aria-current');
      });
    }, { rootMargin: '-45% 0px -50% 0px' });
    links.forEach((_, section) => spy.observe(section));
  });

  root.classList.add('js-ready');
})();
