/* Vale site script. One plain deferred file, no framework, no library.
   Every part checks that its elements exist and does nothing otherwise, so
   the same file serves index, 404, privacy and terms, and every part runs in
   safe(): one failing must not stop the others. There is exactly one
   requestAnimationFrame loop (makeLoop): it reads first and then writes,
   runs continuously only while the page is visible, motion is welcome and
   something animates, and is otherwise asked for one frame at a time by
   scroll, resize, load and input. */
(function () {
  'use strict';

  var doc = document;
  var root = doc.documentElement;
  var reduce = window.matchMedia('(prefers-reduced-motion: reduce)');

  function $(sel, ctx) { return (ctx || doc).querySelector(sel); }
  function $$(sel, ctx) { return Array.prototype.slice.call((ctx || doc).querySelectorAll(sel)); }
  function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)); }
  function pad(n) { return (n < 10 ? '0' : '') + n; }
  function store(key, val) { try { localStorage.setItem(key, val); } catch (e) { /* storage blocked: the choice lasts for this visit */ } }
  function isLight() { return root.getAttribute('data-theme') === 'light'; }
  function rnd(a, b) { return a + Math.random() * (b - a); }

  /* Shared, written by the progress task at the start of every frame. */
  var S = { dirty: true, sy: 0, max: 0, p: 0, vw: 0, vh: 0, px: 0.5, py: 0.5, mx: -1, my: -1, moving: false, scrollAt: 0 };
  var loop = null;
  var notchApi = null;
  var coatApi = null;

  /* ---------- the one animation loop ----------
     Tasks have read(t, dt) and write(t, dt), and optionally on() and busy.
     A task runs in a frame when it is flushed, busy (it set busy in its last
     write: it still animates), or on() says so. All reads run before all
     writes. The loop asks for the next frame by itself only while some task
     is busy, the page is visible and motion is welcome; everything else
     (scroll, resize, load, pointer, a timer) calls request(). fx.near follows
     an element through an IntersectionObserver (gate). */
  function makeLoop() {
    var list = [];
    var raf = 0;
    var last = 0;
    function frame(t) {
      raf = 0;
      var dt = last ? Math.min(0.1, (t - last) / 1000) : 1 / 60;
      last = t;
      var run = [];
      var i;
      for (i = 0; i < list.length; i++) {
        var fx = list[i];
        if (fx.flush || fx.busy || (fx.on && fx.on())) { fx.flush = false; run.push(fx); }
      }
      for (i = 0; i < run.length; i++) run[i].read(t, dt);
      for (i = 0; i < run.length; i++) run[i].write(t, dt);
      var more = false;
      if (!doc.hidden && !reduce.matches) {
        for (i = 0; i < run.length; i++) if (run[i].busy) { more = true; break; }
      }
      S.dirty = false;
      if (more) request(); else last = 0;
    }
    function request() {
      if (raf) return;
      raf = requestAnimationFrame(frame);
    }
    /* Scroll-driven tasks run on a frame only when S.dirty says the page
       scrolled, resized or loaded since the last one (animation frames skip
       them); a task can still be flushed or busy. */
    window.addEventListener('scroll', function () { S.dirty = true; request(); }, { passive: true });
    window.addEventListener('resize', function () { S.dirty = true; list.forEach(function (fx) { if (fx.onResize) fx.flush = true; }); request(); });
    window.addEventListener('load', function () { S.dirty = true; request(); });
    window.addEventListener('pageshow', function () { S.dirty = true; request(); });
    doc.addEventListener('visibilitychange', function () {
      root.classList.toggle('is-hidden', doc.hidden);
      if (!doc.hidden) request();
    });
    if (reduce.addEventListener) reduce.addEventListener('change', function () { S.dirty = true; list.forEach(function (fx) { fx.flush = true; }); request(); });
    return {
      add: function (fx) { fx.flush = true; list.push(fx); request(); return fx; },
      kick: function (fx) { if (fx) fx.flush = true; request(); },
      request: request,
      /* fx.near follows el, with margin around the viewport. Without an
         observer the effect simply stays on. */
      gate: function (el, fx, margin) {
        if (!('IntersectionObserver' in window)) { fx.near = true; return; }
        fx.near = false;
        new IntersectionObserver(function (entries) {
          fx.near = entries[entries.length - 1].isIntersecting;
          fx.flush = true;
          S.dirty = true;
          request();
        }, { rootMargin: margin || '200px 0px' }).observe(el);
      }
    };
  }

  /* ---------- theme button ---------- */
  var ICON_SUN = '<svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2.2M12 19.3v2.2M2.5 12h2.2M19.3 12h2.2M5.3 5.3l1.6 1.6M17.1 17.1l1.6 1.6M5.3 18.7l1.6-1.6M17.1 6.9l1.6-1.6"/></svg>';
  var ICON_MOON = '<svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z"/></svg>';

  /* Two theme buttons: one in the right pill, one in the dropdown under the
     Menu button (only one of them is displayed at a time). */
  function initTheme() {
    var btns = $$('.theme-btn');
    if (!btns.length) return;
    /* Dark is the default whatever the system scheme says; only a stored or
       clicked choice of light changes it. */
    function current() { return isLight() ? 'light' : 'dark'; }
    function paint() {
      var isDark = current() === 'dark';
      btns.forEach(function (btn) {
        btn.setAttribute('aria-label', isDark ? 'Switch to light theme' : 'Switch to dark theme');
        btn.innerHTML = isDark ? ICON_SUN : ICON_MOON;
      });
    }
    btns.forEach(function (btn) {
      btn.addEventListener('click', function () {
        var next = current() === 'dark' ? 'light' : 'dark';
        root.setAttribute('data-theme', next);
        store('vale-theme', next);
        paint();
        if (loop) loop.request();
      });
      btn.hidden = false;
    });
    paint();
  }

  /* ---------- menu button (below the pill breakpoint, 49em) ----------
     The left pill becomes a Menu button; the nav list opens under it as a
     dropdown with Home, Features, Install, GitHub and the theme button. */
  function initMenu() {
    var btn = $('.menu-btn');
    var nav = $('#site-nav');
    if (!btn || !nav) return;
    function isOpen() { return nav.classList.contains('is-open'); }
    function setOpen(open, focusButton) {
      nav.classList.toggle('is-open', open);
      btn.setAttribute('aria-expanded', open ? 'true' : 'false');
      if (open) {
        var first = $('a', nav);
        if (first) first.focus();
      } else if (focusButton) {
        btn.focus();
      }
    }
    btn.addEventListener('click', function () { setOpen(!isOpen(), false); });
    nav.addEventListener('click', function (e) {
      if (e.target.closest && e.target.closest('a')) setOpen(false, false);
    });
    doc.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && isOpen()) setOpen(false, true);
    });
    doc.addEventListener('pointerdown', function (e) {
      if (isOpen() && !nav.contains(e.target) && !btn.contains(e.target)) setOpen(false, false);
    }, { passive: true });
    /* Tab out of the open dropdown closes it. */
    nav.addEventListener('focusout', function (e) {
      if (isOpen() && e.relatedTarget && !nav.contains(e.relatedTarget) && e.relatedTarget !== btn) setOpen(false, false);
    });
    var wide = window.matchMedia('(min-width: 49em)');
    if (wide.addEventListener) wide.addEventListener('change', function () { setOpen(false, false); });
  }

  /* ---------- copy buttons ---------- */
  /* True while the execCommand fallback runs: it fires a copy event too, and
     the page-wide copy toast must not show a second one. */
  var fallbackCopying = false;

  function copyText(text) {
    function fallback() {
      var ok = false;
      var area = doc.createElement('textarea');
      area.value = text;
      area.setAttribute('readonly', '');
      area.setAttribute('aria-hidden', 'true');
      area.style.cssText = 'position:fixed;top:0;left:-9999px;opacity:0';
      var keep = doc.activeElement;
      doc.body.appendChild(area);
      fallbackCopying = true;
      try { area.select(); area.setSelectionRange(0, text.length); ok = doc.execCommand('copy'); } catch (e) { ok = false; }
      fallbackCopying = false;
      doc.body.removeChild(area);
      if (keep && keep.focus) keep.focus();
      return ok;
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text).then(function () { return true; }, function () { return fallback(); });
    }
    return Promise.resolve(fallback());
  }

  /* The notch's copy preview: trimmed, whitespace collapsed, cut with an ellipsis. */
  function clipLine(text) {
    var t = String(text).replace(/\s+/g, ' ').trim();
    return t.length > 80 ? t.slice(0, 79).replace(/\s+$/, '') + '…' : t;
  }

  function initCopy() {
    $$('[data-copy]').forEach(function (btn) {
      var label = btn.textContent;
      var timer = 0;
      btn.addEventListener('click', function () {
        var target = $(btn.getAttribute('data-copy'));
        if (!target) return;
        var text = target.textContent.trim();
        var msg = btn.parentNode.querySelector('.copy-msg');
        clearTimeout(timer);
        copyText(text).then(function (ok) {
          if (msg) {
            msg.classList.toggle('is-error', !ok);
            msg.textContent = ok ? 'Copied to the clipboard.' : 'Copy failed. Select the text and press Ctrl+C.';
          }
          if (ok) {
            btn.textContent = 'Copied';
            btn.classList.add('is-copied');
            if (notchApi) notchApi.showCopy(clipLine(text));
          }
          timer = setTimeout(function () {
            btn.textContent = label;
            btn.classList.remove('is-copied');
            if (msg && ok) msg.textContent = '';
          }, ok ? 2000 : 6000);
        });
      });
    });
  }

  /* ---------- the cat's coat ----------
     The coat lives in six custom properties on <html>; the hero cat, the
     notch cats, the picture cats and the walking cat all read them. DECIDED
     for the site: Black has the #75675B rim and amber eyes (the app's own rim
     is #8E7CB8). The brand logo and the header brand stay terracotta. */
  var COATS = {
    terracotta: { coat: '#E2683C', l: '#E2683C', r: '#E2683C', eye: '#17120F', rim: 'transparent', pa: '0' },
    calico: { coat: '#F4EDE4', l: '#E2683C', r: '#2B2420', eye: '#17120F', rim: 'transparent', pa: '1' },
    white: { coat: '#F6F1EA', l: '#F6F1EA', r: '#F6F1EA', eye: '#17120F', rim: 'transparent', pa: '0' },
    black: { coat: '#1E1B22', l: '#1E1B22', r: '#1E1B22', eye: '#F2B33D', rim: '#75675B', pa: '0' }
  };

  function initCoats() {
    var chips = $$('.coat-chip');
    if (!chips.length) return null;
    function setCoat(name, pop) {
      var k = COATS[name];
      if (!k) return;
      var st = root.style;
      st.setProperty('--coat', k.coat);
      st.setProperty('--earl', k.l);
      st.setProperty('--earr', k.r);
      st.setProperty('--eye', k.eye);
      st.setProperty('--rim', k.rim);
      st.setProperty('--pa', k.pa);
      chips.forEach(function (c) { c.setAttribute('aria-pressed', c.getAttribute('data-coat') === name ? 'true' : 'false'); });
      if (pop && coatApi && coatApi.pop) coatApi.pop();
    }
    chips.forEach(function (c) {
      c.addEventListener('click', function () { setCoat(c.getAttribute('data-coat'), true); });
    });
    coatApi = { set: setCoat, pop: null };
    return coatApi;
  }

  /* ---------- progress, the sky and the stars ----------
     progress runs first every frame and fills S (scroll, page progress,
     viewport); it writes --p on <html> (the sky mixes dusk, night and dawn
     from it) only when it moved by .004. The star canvas exists on the home
     page only. Stars: count by area, DPR capped at 2, depth parallax on
     scroll, a small pointer parallax, an occasional shooting star, and the
     wish (meteors, then stars flying into the cat's head constellation).
     Dark: warm white stars. Light: faint warm specks, and meteors and the
     constellation in the ink colour. Reduced motion: drawn once, again on
     resize or scroll, and the wish shows only its bubble. */
  var CON_PTS = [[22, 157], [36, 33], [105, 106], [128, 97], [151, 106], [220, 33], [234, 157], [243, 190], [230, 235], [26, 235], [13, 190], [91, 152], [91, 190], [165, 152], [165, 190]];
  var CON_LINES = [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [5, 6], [6, 7], [7, 8], [8, 9], [9, 10], [10, 0], [11, 12], [13, 14]];
  var skyApi = null;

  function initSky() {
    var shown = -1;
    loop.add({
      on: function () { return S.dirty; },
      read: function () {
        S.sy = window.pageYOffset || 0;
        S.vw = root.clientWidth;
        S.vh = root.clientHeight;
        S.max = Math.max(0, root.scrollHeight - S.vh);
        S.p = S.max > 0 ? clamp(S.sy / S.max, 0, 1) : 0;
      },
      write: function () {
        if (shown < 0 || Math.abs(S.p - shown) >= 0.004 || ((S.p === 0 || S.p === 1) && S.p !== shown)) {
          shown = S.p;
          root.style.setProperty('--p', S.p.toFixed(3));
        }
      },
      busy: false,
      onResize: true
    });
    window.addEventListener('scroll', function () { S.scrollAt = performance.now(); }, { passive: true });
    doc.addEventListener('pointermove', function (e) {
      S.mx = e.clientX;
      S.my = e.clientY;
      S.px = e.clientX / Math.max(1, root.clientWidth) - 0.5;
      S.py = e.clientY / Math.max(1, root.clientHeight) - 0.5;
      loop.request();
    }, { passive: true });

    if (!$('#h-hero')) return;
    var cv = doc.createElement('canvas');
    cv.id = 'sky';
    cv.setAttribute('aria-hidden', 'true');
    doc.body.appendChild(cv);
    var ctx = cv.getContext('2d');
    if (!ctx) return;
    var W = 0;
    var H = 0;
    var dpr = 1;
    var pool = [];
    var count = 0;
    var shoot = null;
    var meteors = [];
    var con = null;
    var want = { w: 0, h: 0, r: 1 };
    var animated = false;

    function grow(n) {
      while (pool.length < n) pool.push({ x: Math.random(), y: Math.random() * 1.6, z: Math.random() * 0.8 + 0.2, ph: Math.random() * 6.28, sp: 0.5 + Math.random() * 2 });
    }
    function measure() {
      want.w = root.clientWidth;
      want.h = root.clientHeight;
      want.r = Math.min(2, window.devicePixelRatio || 1);
    }
    function size() {
      if (want.w === W && want.h === H && want.r === dpr) return;
      W = want.w; H = want.h; dpr = want.r;
      cv.width = Math.max(1, Math.round(W * dpr));
      cv.height = Math.max(1, Math.round(H * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      count = Math.round(W * H / 4200);
      grow(count);
    }

    function draw(t, dt) {
      var light = isLight();
      var anim = !reduce.matches;
      var sy = S.sy;
      /* Pointer parallax is motion: under reduced motion the stars ignore the pointer. */
      var px = anim ? S.px : 0;
      var py = anim ? S.py : 0;
      var span = H * 1.6;
      ctx.clearRect(0, 0, W, H);
      var step = light ? 3 : 1;
      var i;
      for (i = 0; i < count; i += step) {
        var s = pool[i];
        var y = (((s.y * H - sy * s.z * 0.25 - py * 14 * s.z) % span) + span) % span - H * 0.3;
        var x = s.x * W - px * 18 * s.z;
        var a = anim && !light ? (0.35 + 0.65 * Math.abs(Math.sin(t / 1000 * s.sp + s.ph))) * s.z : 0.7 * s.z;
        ctx.fillStyle = light ? 'rgba(168,67,28,' + (a * 0.3).toFixed(3) + ')' : 'rgba(244,237,228,' + a.toFixed(3) + ')';
        ctx.fillRect(x, y, s.z * 1.8, s.z * 1.8);
      }
      var inkMeteor = light ? '168,67,28' : '255,214,170';
      /* An occasional shooting star (dark, motion welcome). */
      if (anim && !light) {
        if (!shoot && Math.random() < 0.25 * dt) shoot = { x: rnd(0.2, 0.9) * W, y: Math.random() * H * 0.3, age: 0 };
        if (shoot) {
          var g0 = ctx.createLinearGradient(shoot.x, shoot.y, shoot.x - 120, shoot.y + 60);
          g0.addColorStop(0, 'rgba(244,237,228,.9)');
          g0.addColorStop(1, 'rgba(244,237,228,0)');
          ctx.strokeStyle = g0;
          ctx.lineWidth = 1.5;
          ctx.beginPath();
          ctx.moveTo(shoot.x, shoot.y);
          ctx.lineTo(shoot.x - 120, shoot.y + 60);
          ctx.stroke();
          shoot.x -= 720 * dt;
          shoot.y += 360 * dt;
          shoot.age += dt;
          if (shoot.age > 0.6) shoot = null;
        }
      }
      /* The wish: a meteor shower ... */
      var alive = [];
      meteors.forEach(function (m) {
        if (t < m.t0) { alive.push(m); return; }
        var k = (t - m.t0) / m.d;
        if (k > 1) return;
        alive.push(m);
        var mx = m.x - k * m.len;
        var my = m.y + k * m.len * 0.5;
        var g = ctx.createLinearGradient(mx, my, mx + 140, my - 70);
        g.addColorStop(0, 'rgba(' + inkMeteor + ',' + (1 - k).toFixed(3) + ')');
        g.addColorStop(1, 'rgba(' + inkMeteor + ',0)');
        ctx.strokeStyle = g;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(mx, my);
        ctx.lineTo(mx + 140, my - 70);
        ctx.stroke();
      });
      meteors = alive;
      /* ... then the stars gather into the cat's head. */
      if (con) {
        var e = t - con.t0;
        var fade = e > 6200 ? Math.max(0, 1 - (e - 6200) / 1200) : 1;
        var oy = con.sy - sy;
        if (fade <= 0) {
          con = null;
        } else {
          var pts = con.pts.map(function (p, n) {
            var k2 = clamp((e - 900 - n * 60) / 1300, 0, 1);
            var q = 1 - Math.pow(1 - k2, 3);
            return [p.sx + (p.x - p.sx) * q, p.sy + (p.y + oy - p.sy) * q, k2];
          });
          var lk = clamp((e - 2600) / 1400, 0, 1);
          var ink = light ? '168,67,28' : '244,237,228';
          ctx.strokeStyle = 'rgba(' + ink + ',' + (0.55 * fade).toFixed(3) + ')';
          ctx.lineWidth = 1.2;
          CON_LINES.forEach(function (ab, n) {
            var part = clamp(lk * CON_LINES.length - n, 0, 1);
            if (!part) return;
            var A = pts[ab[0]];
            var B = pts[ab[1]];
            ctx.beginPath();
            ctx.moveTo(A[0], A[1]);
            ctx.lineTo(A[0] + (B[0] - A[0]) * part, A[1] + (B[1] - A[1]) * part);
            ctx.stroke();
          });
          pts.forEach(function (pt, n) {
            var tw = 0.7 + 0.3 * Math.sin(t / 200 + n);
            ctx.fillStyle = light ? 'rgba(168,67,28,' + (fade * tw).toFixed(3) + ')' : 'rgba(255,226,190,' + (fade * tw).toFixed(3) + ')';
            ctx.beginPath();
            ctx.arc(pt[0], pt[1], 2.4 * (pt[2] > 0.99 ? 1.3 : 1), 0, 6.283);
            ctx.fill();
            if (pt[2] > 0.99) {
              ctx.fillStyle = 'rgba(226,104,60,' + (0.25 * fade).toFixed(3) + ')';
              ctx.beginPath();
              ctx.arc(pt[0], pt[1], 8, 0, 6.283);
              ctx.fill();
            }
          });
        }
      }
      animated = anim && ((!light) || meteors.length > 0 || !!con);
    }

    var fx = {
      on: function () { return true; },
      read: measure,
      write: function (t, dt) {
        size();
        draw(t, dt);
        fx.busy = animated && !doc.hidden;
      },
      busy: false,
      onResize: true
    };
    skyApi = {
      fx: fx,
      /* The wish: cx, cy and s place the constellation (viewport px, px per grid unit). */
      wish: function (cx, cy, s) {
        var t = performance.now();
        if (reduce.matches) return false;
        var i;
        for (i = 0; i < 14; i++) meteors.push({ t0: t + i * 110 + Math.random() * 80, d: 700 + Math.random() * 300, x: W * (0.3 + Math.random() * 0.8), y: Math.random() * H * 0.4, len: 300 + Math.random() * 200 });
        con = {
          t0: t,
          sy: S.sy,
          pts: CON_PTS.map(function (xy) {
            return { x: cx + (xy[0] - 128) * s, y: cy + (xy[1] - 134) * s, sx: Math.random() * W, sy: Math.random() * H * 0.6 };
          })
        };
        loop.kick(fx);
        return true;
      }
    };
    loop.add(fx);
  }

  /* ---------- the notch ---------- */
  function initNotch() {
    var notch = $('#notch');
    var face = notch && $('.notch-face', notch);
    var panel = notch && $('#notch-panel');
    if (!notch || !face || !panel) return;
    face.removeAttribute('tabindex');

    var state = 'idle';
    var copyTimer = 0;
    var greetTimer = 0;
    var clockTimer = 0;
    var clockEl = $('#notch-clock');
    var open = false;
    var pinned = false;
    var closeTimer = 0;
    var hideTimer = 0;
    var openTimer = 0;
    var dockTimer = 0;
    var holdTimer = 0;
    var lastPointerType = 'mouse';

    /* ----- clock: one timeout aimed at the next minute, only while idle and visible ----- */
    function tickClock() {
      clearTimeout(clockTimer);
      clockTimer = 0;
      if (!clockEl || state !== 'idle' || doc.hidden) return;
      var d = new Date();
      clockEl.textContent = pad(d.getHours()) + ':' + pad(d.getMinutes());
      clockTimer = setTimeout(tickClock, 60000 - (d.getSeconds() * 1000 + d.getMilliseconds()) + 40);
    }

    function setState(next) {
      if (state === next) return;
      state = next;
      notch.setAttribute('data-state', next);
      tickClock();
    }

    doc.addEventListener('visibilitychange', tickClock);

    /* Mirror style.css from the same source it uses: 100vw and the 32.5em media
       query both count the scrollbar, root.clientWidth does not. */
    var phonePeek = window.matchMedia('(max-width: 32.5em)');
    function peekSize(name) {
      var vw = window.innerWidth;
      if (name === 'greet') return { w: Math.min(230, vw - 32), h: phonePeek.matches ? 45 : 38 };
      return { w: Math.min(340, vw - 32), h: 56 };
    }

    /* ----- room beside the header pills -----
       The header is fixed and always on screen, with a pill on each side of
       the notch. The copy toast (340 x 56) and the greeting (230 x 38) show at
       full size only while their box, plus PILL_MARGIN on every side, clears
       both pills; otherwise they keep the idle footprint (.is-tight). The
       pills are measured in the loop's read phase (pillFx): on start, on
       resize, when fonts arrive, and on every frame while one of these states
       is shown; the decisions use the last measured boxes. */
    var PILL_MARGIN = 8;
    var pillEls = $$('.site-header .pill');
    var pillBoxes = null;
    var room = { clipboard: false, greet: false };
    function roomFor(name) {
      if (!pillBoxes) return false;
      var size = peekSize(name);
      var mid = root.clientWidth / 2;
      var left = mid - size.w / 2 - PILL_MARGIN;
      var right = mid + size.w / 2 + PILL_MARGIN;
      var bottom = size.h + PILL_MARGIN;
      for (var i = 0; i < pillBoxes.length; i++) {
        var r = pillBoxes[i];
        if (r.width && r.height && r.right > left && r.left < right && r.top < bottom && r.bottom > -PILL_MARGIN) return false;
      }
      return true;
    }
    var pillFx = {
      on: function () { return copyTimer || greetTimer; },
      read: function () {
        pillBoxes = pillEls.map(function (el) { return el.getBoundingClientRect(); });
        pillFx.next = { clipboard: roomFor('clipboard'), greet: roomFor('greet') };
      },
      write: function () {
        var n = pillFx.next;
        if (n.clipboard === room.clipboard && n.greet === room.greet) return;
        room = n;
        refresh();
      },
      onResize: true
    };
    /* The state the notch shows: the 3 s copy toast first, then the greeting,
       then idle. Both wider states need room beside the pills. */
    function refresh() {
      var want = copyTimer ? 'clipboard' : greetTimer ? 'greet' : 'idle';
      setState(want);
      var tight = want === 'clipboard' ? !room.clipboard : want === 'greet' ? !room.greet : false;
      notch.classList.toggle('is-tight', tight);
    }
    loop.add(pillFx);
    if (doc.fonts) {
      var repill = function () { loop.kick(pillFx); };
      if (doc.fonts.ready) doc.fonts.ready.then(repill, function () {});
      if (doc.fonts.addEventListener) doc.fonts.addEventListener('loadingdone', repill);
    }

    /* ----- copy notice and greeting -----
       The copy notice is 340 x 56 and the greeting 230 x 38; refresh() keeps
       either at the idle footprint when it would touch a header pill. */
    notchApi = {
      showCopy: function (text) {
        if (open) return;
        var preview = $('#notch-clip-preview');
        if (preview) preview.textContent = text;
        clearTimeout(copyTimer);
        copyTimer = setTimeout(function () {
          copyTimer = 0;
          refresh();
        }, 3000);
        refresh();
        loop.kick(pillFx);
      },
      /* "Hi, I'm Vale!" with the happy cat for about 2 s (the dropped hero cat). */
      greet: function () {
        if (open) return;
        clearTimeout(greetTimer);
        greetTimer = setTimeout(function () {
          greetTimer = 0;
          refresh();
        }, 2000);
        refresh();
        loop.kick(pillFx);
      },
      /* The glow while the dragged hero cat is over the notch. */
      glow: function (on) { notch.classList.toggle('is-glow', !!on); },
      /* The pointer is still on the notch right after the cat is dropped
         there: the panel must not hover-open over the greeting. */
      hold: function (ms) {
        cancelHoverOpen();
        clearTimeout(holdTimer);
        holdTimer = setTimeout(function () { holdTimer = 0; }, ms);
      }
    };

    /* Any copy on the page with a non-empty selection shows the copy toast. */
    doc.addEventListener('copy', function () {
      if (fallbackCopying || !window.getSelection) return;
      var text = String(window.getSelection());
      if (!text.trim()) return;
      notchApi.showCopy(clipLine(text));
    });

    /* ----- pages ----- */
    var tabs = $$('[role="tab"]', panel);
    var pages = $$('[role="tabpanel"]', panel);
    var titleEl = $('#notch-title');
    var active = 0;
    function select(i) {
      active = i;
      tabs.forEach(function (t, n) {
        t.setAttribute('aria-selected', n === i ? 'true' : 'false');
        t.setAttribute('tabindex', n === i ? '0' : '-1');
      });
      pages.forEach(function (p, n) { p.hidden = n !== i; });
      if (titleEl) titleEl.textContent = tabs[i].getAttribute('aria-label');
    }
    tabs.forEach(function (t, n) {
      t.addEventListener('click', function () { select(n); });
      t.addEventListener('focus', function () {
        tabs.forEach(function (o) { o.setAttribute('tabindex', o === t ? '0' : '-1'); });
      });
    });
    $('.dock', panel).addEventListener('keydown', function (e) {
      var at = tabs.indexOf(doc.activeElement);
      if (at < 0) return;
      var to = -1;
      if (e.key === 'ArrowRight') to = (at + 1) % tabs.length;
      else if (e.key === 'ArrowLeft') to = (at - 1 + tabs.length) % tabs.length;
      else if (e.key === 'Home') to = 0;
      else if (e.key === 'End') to = tabs.length - 1;
      if (to < 0) return;
      e.preventDefault();
      tabs[to].focus();
    });

    /* ----- Dock magnification: scale = 1 + 0.9 * falloff(d), R = 2 x pitch, eased with tau 70 ms -----
       The easing runs in the shared loop; the pointer only sets targets. */
    var dock = $('.dock', panel);
    var dockItems = tabs.map(function (t) { return { ico: $('.dock-ico', t), cur: 1, target: 1 }; });
    var fine = window.matchMedia('(pointer: fine)');
    var narrow = window.matchMedia('(max-width: 32.5em)');
    var dockFx = {
      read: function () {},
      write: function (t, dt) {
        var k = reduce.matches ? 1 : 1 - Math.exp(-dt / 0.07);
        var moving = false;
        dockItems.forEach(function (it) {
          it.cur += (it.target - it.cur) * k;
          if (Math.abs(it.target - it.cur) < 0.002) it.cur = it.target; else moving = true;
          it.ico.style.transform = it.cur === 1 ? '' : 'scale(' + it.cur.toFixed(3) + ')';
        });
        dockFx.busy = moving;
      },
      busy: false
    };
    loop.add(dockFx);
    function dockReset() {
      dockItems.forEach(function (it) { it.target = 1; });
      loop.kick(dockFx);
    }
    dock.addEventListener('pointermove', function (e) {
      if (e.pointerType !== 'mouse' || !fine.matches || narrow.matches) return;
      var first = tabs[0].getBoundingClientRect();
      var second = tabs[1].getBoundingClientRect();
      var pitch = second.left - first.left;
      var reach = 2 * pitch;
      tabs.forEach(function (t, n) {
        var r = t.getBoundingClientRect();
        var d = Math.abs(e.clientX - (r.left + r.width / 2));
        var f = d < reach ? 0.5 * (1 + Math.cos(Math.PI * d / reach)) : 0;
        dockItems[n].target = 1 + 0.9 * f;
      });
      loop.kick(dockFx);
    }, { passive: true });
    dock.addEventListener('pointerleave', dockReset, { passive: true });

    /* Runs fn once the notch's width transition has ended (or at once when there
       is none, as with reduced motion). transitioncancel is not a signal: opening
       right after a close interrupts the closing width transition, and its
       cancel event would arrive while the notch is still narrow. The timer is
       the fallback for an opening transition that never reports its end. */
    var widthWait = null;
    function stopWidthWait() {
      clearTimeout(dockTimer);
      if (!widthWait) return;
      notch.removeEventListener('transitionend', widthWait);
      widthWait = null;
    }
    function afterWidthSettles(fn) {
      stopWidthWait();
      if (parseFloat(getComputedStyle(notch).transitionDuration) === 0) { fn(); return; }
      function done() {
        stopWidthWait();
        fn();
      }
      widthWait = function (e) { if (e.target === notch && e.propertyName === 'width') done(); };
      notch.addEventListener('transitionend', widthWait);
      dockTimer = setTimeout(done, 1000);
    }

    /* Scroll the dock (only the dock, never the page) so the tab is inside it. */
    function revealTab(t) {
      var d = dock.getBoundingClientRect();
      var r = t.getBoundingClientRect();
      if (r.left < d.left) dock.scrollLeft += r.left - d.left;
      else if (r.right > d.right) dock.scrollLeft += r.right - d.right;
    }

    /* ----- open and close ----- */
    function setFaceLabel() {
      face.setAttribute('aria-expanded', open ? 'true' : 'false');
      face.setAttribute('aria-label', open ? 'Vale island demo. Close the panel' : 'Vale island demo. Open the panel');
    }
    function openPanel(pin) {
      cancelHoverOpen();
      clearTimeout(closeTimer);
      clearTimeout(hideTimer);
      if (!open) {
        panel.hidden = false;
        panel.inert = false;
        void panel.offsetWidth;
        notch.classList.add('is-open');
        open = true;
        setFaceLabel();
      }
      if (pin) pinned = true;
    }
    function closePanel(returnFocus) {
      cancelHoverOpen();
      clearTimeout(closeTimer);
      stopWidthWait();
      if (!open) return;
      open = false;
      pinned = false;
      notch.classList.remove('is-open');
      panel.inert = true;
      setFaceLabel();
      dockReset();
      clearTimeout(hideTimer);
      hideTimer = setTimeout(function () { panel.hidden = true; }, reduce.matches ? 0 : 380);
      if (returnFocus) face.focus();
      refresh();
    }

    /* Hover intent: the idle notch sits between the two header pills, so a
       pointer moving along the header from one pill to the other crosses it. The panel opens only
       once the pointer has rested on the notch for HOVER_OPEN_MS (the app's
       shelf waits OPEN_DELAY_MS = 120); any movement restarts the wait, and
       leaving cancels it. */
    var HOVER_OPEN_MS = 120;
    var HOVER_STILL_PX = 4;
    var hoverX = 0;
    var hoverY = 0;
    function cancelHoverOpen() {
      clearTimeout(openTimer);
      openTimer = 0;
    }
    function armHoverOpen(e) {
      cancelHoverOpen();
      hoverX = e.clientX;
      hoverY = e.clientY;
      openTimer = setTimeout(function () {
        openTimer = 0;
        openPanel(false);
      }, HOVER_OPEN_MS);
    }
    notch.addEventListener('pointerenter', function (e) {
      if (e.pointerType === 'touch') return;
      clearTimeout(closeTimer);
      if (!open && !holdTimer) armHoverOpen(e);
    });
    notch.addEventListener('pointermove', function (e) {
      if (e.pointerType === 'touch' || open || !openTimer) return;
      if (Math.abs(e.clientX - hoverX) > HOVER_STILL_PX || Math.abs(e.clientY - hoverY) > HOVER_STILL_PX) armHoverOpen(e);
    }, { passive: true });
    notch.addEventListener('pointerleave', function (e) {
      if (e.pointerType === 'touch') return;
      cancelHoverOpen();
      if (pinned) return;
      clearTimeout(closeTimer);
      closeTimer = setTimeout(function () { closePanel(false); }, 160);
    });
    face.addEventListener('pointerdown', function (e) { lastPointerType = e.pointerType || 'mouse'; });
    face.addEventListener('click', function (e) {
      var byKeyboard = e.detail === 0;
      if (open && pinned) { closePanel(byKeyboard); return; }
      if (open && !pinned && lastPointerType !== 'touch' && !byKeyboard) { pinned = true; return; }
      if (open && !pinned) { closePanel(byKeyboard); return; }
      openPanel(true);
      if (byKeyboard) {
        /* The notch is still growing to its open width here; a scrolling focus
           would scroll the (scrollable, still narrow) dock and leave the first
           icons off to the left at full width. Focus without scrolling, then
           bring the focused tab into view once the width has settled. */
        tabs[active].focus({ preventScroll: true });
        afterWidthSettles(function () {
          revealTab(tabs.indexOf(doc.activeElement) >= 0 ? doc.activeElement : tabs[active]);
        });
      }
    });
    face.addEventListener('keydown', function () { lastPointerType = 'key'; });
    doc.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && open) closePanel(true);
    });
    doc.addEventListener('pointerdown', function (e) {
      if (open && pinned && !notch.contains(e.target)) closePanel(false);
    }, { passive: true });
    notch.addEventListener('focusout', function (e) {
      if (open && e.relatedTarget && !notch.contains(e.relatedTarget)) closePanel(false);
    });

    select(0);
    tickClock();
  }

  /* ---------- the hero cat ----------
     Inline SVG in the logo's 256 grid. Without script it is a still picture
     (role=img); here its wrapper becomes a button ("Vale's cat. Press to poke
     it"). Always: eyes follow the pointer (eased), the head tilts a little
     toward it, it blinks every 2.5 to 6 s, the right ear twitches every 4 to 9
     s, the ears perk on hover (CSS). Press, Enter or Space: a squash jump, a
     bubble (mrrp!, meow, hi!, nya~) and sparks. Rubbing it with a mouse or pen
     (no button down): it purrs (happy eyes, a fine vibration, hearts), never
     on touch. Drag (mouse: more than 6 px; touch: a 350 ms long press, then
     move): squint eyes, it follows the pointer and tilts with the velocity,
     and springs home when released; dropped on the live notch (which glows
     while the cat is over it) it shrinks in, the notch greets, and the cat
     pops back saying "again?". Five presses within 2 s make a wish: the
     bubbles "make a wish" and "granted", meteors, and a constellation of the
     cat's head behind it. Reduced motion: no jump, sparks, vibration, meteors
     or constellation; bubbles and eye changes still work. */
  function initHeroCat() {
    var wrap = $('#cat-wrap');
    var stage = $('#hero-cat');
    var cat = $('#cat');
    var eyes = $('#cat-eyes');
    var all = $('#cat-all');
    var bubble = $('#cat-bubble');
    if (!wrap || !stage || !cat || !eyes || !all || !bubble) return;
    var notchEl = $('#notch');

    stage.setAttribute('role', 'button');
    stage.setAttribute('tabindex', '0');
    stage.setAttribute('aria-label', 'Vale’s cat. Press to poke it');
    /* Quick swipes still scroll; only an armed long press on touch drags. */
    stage.addEventListener('contextmenu', function (e) { e.preventDefault(); });

    var h = new Date().getHours();
    var greeting = h < 4 ? 'Up late?' : h < 11 ? 'Good morning!' : h < 18 ? 'Good afternoon!' : 'Good evening!';
    var sayTimer = 0;
    function say(text, ms) {
      bubble.textContent = text;
      bubble.classList.add('on');
      clearTimeout(sayTimer);
      sayTimer = setTimeout(function () { bubble.classList.remove('on'); }, ms || 1300);
    }
    setTimeout(function () { say(greeting, 2600); }, 700);

    function animate(el, frames, opts) {
      if (reduce.matches || !el.animate) return null;
      return el.animate(frames, opts);
    }
    function burst(chars, n) {
      if (reduce.matches || !wrap.animate) return;
      for (var i = 0; i < n; i++) {
        var s = doc.createElement('span');
        s.className = 'fx';
        s.setAttribute('aria-hidden', 'true');
        s.textContent = chars[i % chars.length];
        wrap.appendChild(s);
        var a = Math.random() * 6.28;
        var d = 80 + Math.random() * 90;
        var an = s.animate([
          { transform: 'translate(-50%,-50%) scale(.4)', opacity: 1 },
          { transform: 'translate(' + (Math.cos(a) * d).toFixed(1) + 'px,' + (Math.sin(a) * d - 40).toFixed(1) + 'px) scale(1.1)', opacity: 0 }
        ], { duration: 900 + Math.random() * 400, easing: 'cubic-bezier(.2,.8,.2,1)' });
        an.onfinish = (function (el) { return function () { if (el.parentNode) el.parentNode.removeChild(el); }; })(s);
      }
    }
    function pop() {
      animate(cat, [{ transform: 'none' }, { transform: 'scale(.82,1.12)', offset: 0.3 }, { transform: 'none' }], { duration: 450, easing: 'cubic-bezier(.3,1.7,.5,1)' });
    }
    if (coatApi) coatApi.pop = pop;

    /* ----- blink and ear twitch ----- */
    var purring = false;
    var held = false;
    (function blink() {
      setTimeout(function () {
        if (!doc.hidden && !purring && !held) {
          cat.classList.add('blink');
          setTimeout(function () { cat.classList.remove('blink'); }, 130);
        }
        blink();
      }, rnd(2500, 6000));
    })();
    (function twitch() {
      setTimeout(function () {
        var ear = $('.er', cat);
        if (!doc.hidden && ear && !reduce.matches) animate(ear, [{ transform: 'none' }, { transform: 'rotate(14deg)', offset: 0.4 }, { transform: 'none' }], { duration: 350 });
        twitch();
      }, rnd(4000, 9000));
    })();

    /* ----- poke, and the wish after five presses within 2 s ----- */
    var SAYS = ['mrrp!', 'meow', 'hi!', 'nya~'];
    var sayAt = 0;
    var presses = [];
    var looking = false;
    var lookTimer = 0;
    function wish() {
      say('make a wish ✨', 3000);
      if (reduce.matches || !skyApi) return;
      var r = wrap.getBoundingClientRect();
      var ok = skyApi.wish(r.left + r.width / 2, r.top + r.height / 2, 1.8 * r.width / 256);
      if (!ok) return;
      looking = true;
      clearTimeout(lookTimer);
      lookTimer = setTimeout(function () { looking = false; loop.kick(catFx); }, 6500);
      setTimeout(function () { say('✨ granted', 1800); }, 5200);
      loop.kick(catFx);
    }
    function poke() {
      var now = Date.now();
      presses = presses.filter(function (x) { return now - x < 2000; });
      presses.push(now);
      if (presses.length >= 5) { presses = []; wish(); return; }
      say(SAYS[sayAt++ % SAYS.length]);
      animate(cat, [
        { transform: 'none' },
        { transform: 'scale(1.12,.85)', offset: 0.18 },
        { transform: 'translateY(-34px) scale(.92,1.1)', offset: 0.45 },
        { transform: 'scale(1.05,.95)', offset: 0.75 },
        { transform: 'none' }
      ], { duration: 550, easing: 'cubic-bezier(.3,1.4,.5,1)' });
      burst(['✦', '•', '•'], 9);
    }
    stage.addEventListener('keydown', function (e) {
      if (e.key !== 'Enter' && e.key !== ' ') return;
      e.preventDefault();
      if (e.repeat) return; /* a held key is one press, not a run toward the wish */
      poke();
    });

    /* ----- the pointer: press = poke, drag = carry, rub = purr ----- */
    var down = null;
    var lpTimer = 0;
    var absorbed = false;
    var ox = 0;
    var oy = 0;
    var vx = 0;
    var vy = 0;
    var gx = 0;
    var gy = 0;
    var tx = 0;
    var ty = 0;
    var over = false;
    var returning = false;
    var pet = 0;
    var petT = 0;
    var petTimer = 0;
    var lx = null;
    var ly = null;
    var lastHeart = 0;
    var DRAG_PX = 6;
    /* .hero-inner is its own stacking context, so the stage's z-index alone cannot lift the carried cat above the header. */
    var heroInner = stage.closest ? stage.closest('.hero-inner') : null;
    var LONG_PRESS_MS = 350;

    function startHold() {
      held = true;
      returning = false;
      cat.classList.add('is-held');
      stage.classList.add('is-held');
      if (heroInner) heroInner.classList.add('is-carrying');
      stopPurr();
      loop.kick(catFx);
    }
    stage.addEventListener('pointerdown', function (e) {
      if (absorbed) return;
      if (e.pointerType === 'mouse' && e.button !== 0) return;
      var touch = e.pointerType === 'touch';
      down = { x: e.clientX, y: e.clientY, id: e.pointerId, touch: touch, armed: !touch };
      gx = e.clientX - ox;
      gy = e.clientY - oy;
      tx = ox;
      ty = oy;
      try { stage.setPointerCapture(e.pointerId); } catch (err) { /* capture can fail for a pointer that already ended */ }
      clearTimeout(lpTimer);
      if (touch) lpTimer = setTimeout(function () { if (down && !held) down.armed = true; }, LONG_PRESS_MS);
    });
    /* A non-passive touchmove on the cat only, and only while a drag is
       armed or running, so a quick swipe over the cat still scrolls. */
    stage.addEventListener('touchmove', function (e) {
      if ((down && down.armed && down.touch) || held) e.preventDefault();
    }, { passive: false });

    function stopPurr() {
      if (!purring) return;
      purring = false;
      cat.classList.remove('purr');
      clearTimeout(petTimer);
      pet = 0;
    }
    function checkPurr() {
      pet *= Math.exp(-(performance.now() - petT) / 545);
      petT = performance.now();
      if (pet < 120) stopPurr(); else petTimer = setTimeout(checkPurr, 250);
    }
    function rub(e) {
      var now = performance.now();
      if (lx !== null) pet = pet * Math.exp(-(now - petT) / 545) + Math.hypot(e.clientX - lx, e.clientY - ly);
      petT = now;
      lx = e.clientX;
      ly = e.clientY;
      if (pet > 260 && !purring && !held) {
        purring = true;
        cat.classList.add('purr');
        clearTimeout(petTimer);
        petTimer = setTimeout(checkPurr, 250);
      }
      if (purring && now - lastHeart > 260) { lastHeart = now; burst(['♥'], 1); }
    }
    window.addEventListener('pointermove', function (e) {
      if (down && e.pointerId === down.id) {
        var dist = Math.hypot(e.clientX - down.x, e.clientY - down.y);
        if (!held) {
          if (down.touch && !down.armed && dist > 10) { clearTimeout(lpTimer); down = null; return; }
          if (down.armed && dist > DRAG_PX) startHold();
        }
        if (held) {
          tx = e.clientX - gx;
          ty = e.clientY - gy;
          loop.kick(catFx);
        }
        return;
      }
      if (e.pointerType === 'touch' || e.buttons || held || !stage.contains(e.target)) { lx = null; return; }
      rub(e);
    }, { passive: true });
    stage.addEventListener('pointerleave', function () { lx = null; });

    function release(e) {
      if (!down || (e && e.pointerId !== down.id)) return;
      var wasHeld = held;
      clearTimeout(lpTimer);
      down = null;
      held = false;
      cat.classList.remove('is-held');
      stage.classList.remove('is-held');
      if (heroInner) heroInner.classList.remove('is-carrying');
      if (e && e.type === 'pointercancel') { returning = true; loop.kick(catFx); return; }
      if (!wasHeld) { poke(); return; }
      if (over) absorb(); else { returning = true; loop.kick(catFx); }
    }
    window.addEventListener('pointerup', release);
    window.addEventListener('pointercancel', release);

    function absorb() {
      over = false;
      if (notchApi) notchApi.glow(false);
      absorbed = true;
      returning = false;
      if (notchApi) notchApi.hold(2800);
      var a = stage.getBoundingClientRect();
      var b = notchEl ? notchEl.getBoundingClientRect() : a;
      var dx = b.left + b.width / 2 - (a.left + a.width / 2);
      var dy = b.top + b.height / 2 - (a.top + a.height / 2);
      var anim = animate(stage, [
        { transform: 'translate(' + ox + 'px,' + oy + 'px)', opacity: 1 },
        { transform: 'translate(' + (ox + dx) + 'px,' + (oy + dy) + 'px) scale(.12)', opacity: 0 }
      ], { duration: 320, easing: 'cubic-bezier(.5,0,.8,.4)', fill: 'forwards' });
      if (!anim) stage.style.opacity = '0';
      setTimeout(function () { if (notchApi) notchApi.greet(); }, 300);
      setTimeout(function () {
        if (anim) anim.cancel();
        ox = oy = vx = vy = tx = ty = 0;
        absorbed = false;
        stage.style.transform = '';
        stage.style.opacity = '';
        animate(stage, [{ transform: 'scale(.3)', opacity: 0 }, { transform: 'scale(1)', opacity: 1 }], { duration: 480, easing: 'cubic-bezier(.3,1.6,.5,1)' });
        say('again?');
      }, 2600);
    }

    /* ----- the cat's frame: eyes, tilt, drag, the spring home -----
       One task in the shared loop. Reads: the stage box (and the notch box
       while dragged). Writes: the eye and tilt transforms, the stage
       transform, the notch glow. */
    var ex = 0;
    var ey = 0;
    var tl = 0;
    var box = null;
    var notchBox = null;
    var shownEx = null;
    var shownTl = null;
    var shownStage = '';
    var catFx = {
      near: true,
      on: function () { return catFx.near || held || returning; },
      read: function () {
        box = stage.getBoundingClientRect();
        notchBox = held && notchEl ? notchEl.getBoundingClientRect() : null;
      },
      write: function (t, dt) {
        var busy = false;
        var snap = reduce.matches;
        /* Drag, and the spring home (steps of one 60 Hz frame). */
        if (held) {
          var nx = tx;
          var ny = ty;
          vx += ((nx - ox) - vx) * 0.4;
          vy += ((ny - oy) - vy) * 0.4;
          ox = nx;
          oy = ny;
          if (notchBox) {
            over = Math.abs(box.left + box.width / 2 - (notchBox.left + notchBox.width / 2)) < 110 && box.top + box.height / 2 < notchBox.bottom + 70;
            if (notchApi) notchApi.glow(over);
          }
          busy = true;
        } else if (returning && !absorbed) {
          if (snap) {
            ox = oy = vx = vy = 0;
          } else {
            var steps = clamp(Math.round(dt * 60), 1, 4);
            for (var i = 0; i < steps; i++) {
              vx += -ox * 0.08;
              vy += -oy * 0.08;
              vx *= 0.82;
              vy *= 0.82;
              ox += vx;
              oy += vy;
            }
          }
          if (Math.abs(ox) < 0.3 && Math.abs(oy) < 0.3 && Math.abs(vx) < 0.3 && Math.abs(vy) < 0.3) {
            ox = oy = vx = vy = 0;
            returning = false;
          } else {
            busy = true;
          }
          if (notchApi) notchApi.glow(false);
        }
        if (!absorbed) {
          var st = (ox || oy || vx) ? 'translate(' + ox.toFixed(1) + 'px,' + oy.toFixed(1) + 'px) rotate(' + clamp(vx * 1.5, -25, 25).toFixed(1) + 'deg)' : '';
          if (st !== shownStage) { shownStage = st; stage.style.transform = st; }
        }
        /* Eyes and tilt toward the pointer (up during the wish). */
        var cx = box.left + box.width / 2;
        var cy = box.top + box.height * 0.62;
        var dx = looking ? 0 : (S.mx < 0 ? 0 : S.mx - cx);
        var dy = looking ? -400 : (S.my < 0 ? 0 : S.my - cy);
        var d = Math.hypot(dx, dy) || 1;
        var k = Math.min(1, d / 360);
        var wantX = (S.mx < 0 && !looking) ? 0 : dx / d * 16 * k;
        var wantY = (S.my < 0 && !looking) ? 0 : dy / d * (looking ? 18 : 11) * k;
        var wantT = (S.mx < 0 && !looking) ? 0 : dx / d * 6 * k;
        if (snap) {
          ex = wantX; ey = wantY; tl = wantT;
        } else {
          var f = 1 - Math.exp(-dt / 0.11);
          var f2 = 1 - Math.exp(-dt / 0.15);
          ex += (wantX - ex) * f;
          ey += (wantY - ey) * f;
          tl += (wantT - tl) * f2;
          if (Math.abs(wantX - ex) > 0.03 || Math.abs(wantY - ey) > 0.03 || Math.abs(wantT - tl) > 0.03) busy = true;
          else { ex = wantX; ey = wantY; tl = wantT; }
        }
        var es = 'translate(' + ex.toFixed(2) + ' ' + ey.toFixed(2) + ')';
        if (es !== shownEx) { shownEx = es; eyes.setAttribute('transform', es); }
        var ts = 'rotate(' + tl.toFixed(2) + ' 128 230)';
        if (ts !== shownTl) { shownTl = ts; all.setAttribute('transform', ts); }
        catFx.busy = busy;
      },
      busy: false
    };
    loop.gate(wrap, catFx, '100px 0px');
    loop.add(catFx);
  }

  /* ---------- marquee: the 14 names move sideways with the scroll ---------- */
  function initMarquee() {
    var wrapEl = $('.marquee');
    var row = $('#marquee');
    if (!wrapEl || !row) return;
    var shown = 0;
    var fx = {
      on: function () { return fx.near && S.dirty; },
      read: function () {},
      write: function () {
        var x = reduce.matches ? 0 : Math.round(S.sy * 0.35);
        if (x === shown) return;
        shown = x;
        row.style.transform = x ? 'translate3d(' + (-x) + 'px,0,0)' : '';
      }
    };
    loop.gate(wrapEl, fx, '100px 0px');
    loop.add(fx);
  }

  /* ---------- the header pills: transparent at the top, tinted once scrolled ----------
     Set once at start (a reload mid-page restores the scroll position before
     or soon after this runs) and then by the loop. */
  var SCROLLED_AT = 12;
  function initHeader() {
    var header = $('.site-header');
    if (!header) return;
    var scrolled = window.pageYOffset > SCROLLED_AT;
    header.classList.toggle('is-scrolled', scrolled);
    var next = scrolled;
    loop.add({
      on: function () { return S.dirty; },
      read: function () { next = S.sy > SCROLLED_AT; },
      write: function () {
        if (next === scrolled) return;
        scrolled = next;
        header.classList.toggle('is-scrolled', scrolled);
      }
    });
  }

  /* ---------- go to top ----------
     Without script the link is always shown and is a plain #top link. With
     script it shows (.is-shown) once the hero's bottom has left the top of
     the viewport, or after one viewport height on pages without a hero, and
     hides again near the top. It also hides while its box lies over a button
     (Download, Copy, a coat chip, a playground control), a details summary,
     the install line or a footer link, so it never blocks one. The list is
     built once. A click or Enter scrolls to the top (smooth only when motion
     is welcome), then focuses #top without scrolling, so the next Tab is the
     skip link; the URL keeps no #top. */
  function initToTop() {
    var btn = $('.to-top');
    var target = $('#top');
    if (!btn || !target) return;
    btn.addEventListener('click', function (e) {
      e.preventDefault();
      window.scrollTo({ top: 0, left: 0, behavior: reduce.matches ? 'instant' : 'smooth' });
      target.focus({ preventScroll: true });
    });
    var hero = $('.hero');
    var avoid = $$('.btn, .coat-chip, .play-card button, .more summary, .cmd-scroll, .site-footer a');
    var shown = false;
    var next = false;
    loop.add({
      on: function () { return S.dirty; },
      read: function () {
        var past = hero ? hero.getBoundingClientRect().bottom < 0 : S.sy > S.vh;
        var covers = false;
        if (past) {
          var b = btn.getBoundingClientRect();
          for (var i = 0; i < avoid.length && !covers; i++) {
            var r = avoid[i].getBoundingClientRect();
            covers = r.width > 0 && r.height > 0 && r.right > b.left && r.left < b.right && r.bottom > b.top && r.top < b.bottom;
          }
        }
        next = past && !covers;
      },
      write: function () {
        if (next === shown) return;
        shown = next;
        btn.classList.toggle('is-shown', shown);
      }
    });
  }

  /* ---------- the story: one hover, fourteen pages ----------
     The wrapper (.story-pin) is about 560vh (420vh on phones) while .is-pin is
     set, and the stage inside it, 100svh tall, sticks to the top. p =
     clamp(-top / (height - svh), 0, 1) of the wrapper. z = clamp(p * 8, 0, 1)
     grows the panel from notch size to full; q = clamp((p - .1) / .9, 0, 1)
     picks the page, round(q * 13), while the page position fpos eases toward
     q * 13 and the dock icons magnify as the live notch's dock does
     (1 + 0.9 * falloff(d), falloff = .5 (1 + cos(pi d / 2)) within 2 icons).
     .is-pin is set only with motion welcome and a stage that fits 100svh
     (measured with a hidden probe, again once fonts are in and on viewport
     changes, never on innerHeight); otherwise the story is static: the first
     page on the screen and the 14 names as a readable list.
     Scroll position (kept from the old Susun scrub): (1) the first choice is
     made synchronously when this runs, before the browser handles a
     fragment; (2) until the reader scrolls or presses something, and while
     the target still sits where the fragment scroll put it, a #fragment
     target after the wrapper is put back at its scroll margin; (3) otherwise,
     with the wrapper above the view, the first element after it is measured
     before and after the switch and only the remaining shift is scrolled;
     (4) a resize flushes the effect, so the switch happens at the resize;
     (5) the pages are set in the same task as the class. */
  function initStory() {
    var sec = $('#features.story');
    var outer = sec && $('.story-pin', sec);
    var stage = sec && $('.story-stage', sec);
    var inner = sec && $('.story-grid', sec);
    var screen = sec && $('.screen', sec);
    var copy = sec && $('.story-copy', sec);
    if (!sec || !outer || !stage || !inner || !screen || !copy) return;
    var pagesEls = $$('.bp-pg', sec);
    var dockEls = $$('.bp-dock svg', sec);
    var listEls = $$('.story-list li', sec);
    var nEl = $('.story-n', sec);
    var nameEl = $('.st-name', sec);
    var lineEl = $('.st-line', sec);
    var titleEl = $('.bp-title', sec);
    var names = listEls.map(function (li) { return li.querySelector('b').textContent; });
    var lines = listEls.map(function (li) { return li.querySelector('span').textContent; });
    var N = pagesEls.length;
    if (!N || dockEls.length !== N || names.length !== N) return;

    /* Measures 100svh (100vh where svh is unknown), the sticky stage's height. */
    var probe = doc.createElement('div');
    probe.setAttribute('aria-hidden', 'true');
    probe.style.cssText = 'position:fixed;top:0;left:0;width:0;height:100vh;visibility:hidden;pointer-events:none';
    probe.style.height = '100svh';
    doc.body.appendChild(probe);
    /* Until the reader scrolls or presses something, a fragment target below
       the wrapper is kept in view when the wrapper's height changes. These
       listeners only start when this script runs, so the target must also
       still sit where the fragment scroll put it: a reader who scrolled away
       before a slow site.js arrived is not pulled back. */
    var touched = false;
    function touch() { touched = true; }
    ['wheel', 'touchstart', 'keydown', 'pointerdown'].forEach(function (t) {
      window.addEventListener(t, touch, { passive: true, once: true });
    });
    function fragmentBelow() {
      var id = location.hash.slice(1);
      if (!id) return null;
      try { id = decodeURIComponent(id); } catch (e) { return null; }
      var el = doc.getElementById(id);
      return el && (outer.compareDocumentPosition(el) & 4) && !outer.contains(el) ? el : null;
    }
    /* Not scrolled yet (the browser has still to go to the fragment), or the
       block start at scroll-margin-top, or lower when the page ends first. */
    function atFragment(el) {
      if (window.scrollY === 0) return true;
      var t = el.getBoundingClientRect().top;
      var m = parseFloat(getComputedStyle(el).scrollMarginTop) || 0;
      var end = doc.documentElement.scrollHeight - window.innerHeight;
      return Math.abs(t - m) < 2 || (t > m && window.scrollY >= end - 1);
    }
    var vw = -1;
    var vh = -1;
    var sv = 1;
    var fitsView = false;
    var enabled = false;
    var top = 0;
    var bottom = 0;
    var height = 0;
    var cur = -1;
    var fpos = 0;
    var swapTimer = 0;
    var shownZ = -1;
    var shownSp = -1;
    var shownS = [];

    /* Does the pinned layout fit 100svh? The pinned layout is tried on for the
       measurement (a synchronous class switch, nothing is painted). */
    function measureFit() {
      var was = sec.classList.contains('is-pin');
      if (!was) sec.classList.add('is-pin');
      var cs = getComputedStyle(stage);
      var fit = inner.offsetHeight + parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom) <= sv;
      if (!was) sec.classList.remove('is-pin');
      return fit;
    }
    function measure() {
      if (window.innerWidth !== vw || window.innerHeight !== vh) {
        vw = window.innerWidth;
        vh = window.innerHeight;
        sv = probe.offsetHeight || vh;
        fitsView = measureFit();
      }
      var r = outer.getBoundingClientRect();
      top = r.top;
      bottom = r.bottom;
      height = r.height;
    }
    function progress() { return clamp(-top / Math.max(height - sv, 1), 0, 1); }
    function showPage(i) {
      pagesEls.forEach(function (e, j) { e.classList.toggle('on', j === i); e.classList.toggle('prev', j < i); });
      dockEls.forEach(function (d, j) { d.classList.toggle('on', j === i); });
      if (titleEl) titleEl.textContent = names[i];
      if (nEl) nEl.textContent = pad(i + 1);
      var prev = cur;
      cur = i;
      clearTimeout(swapTimer);
      if (prev < 0 || reduce.matches) {
        nameEl.textContent = names[i];
        lineEl.textContent = lines[i];
        copy.classList.remove('swap');
      } else {
        copy.classList.add('swap');
        swapTimer = setTimeout(function () {
          nameEl.textContent = names[cur];
          lineEl.textContent = lines[cur];
          copy.classList.remove('swap');
        }, 180);
      }
    }
    function resetStatic() {
      shownZ = -1;
      shownSp = -1;
      screen.style.removeProperty('--z');
      copy.style.removeProperty('--sp');
      dockEls.forEach(function (d) { d.style.removeProperty('--s'); });
      shownS = [];
      cur = -1;
      fpos = 0;
      clearTimeout(swapTimer);
      copy.classList.remove('swap');
      showPage(0);
      nameEl.textContent = names[0];
      lineEl.textContent = lines[0];
      if (nEl) nEl.textContent = '01';
    }
    /* Switches .is-pin; the wrapper's height changes by about 460vh. With the
       wrapper above the view, the view must not move. Scroll anchoring
       (overflow-anchor) may already have kept it in place, so the first
       element after the wrapper is measured before and after, and only what
       is left of its shift is scrolled away. */
    function apply(want) {
      enabled = want;
      var target = touched ? null : fragmentBelow();
      if (target && !atFragment(target)) target = null;
      var ref = !target && outer.getBoundingClientRect().bottom <= 0
        ? (outer.nextElementSibling || sec.nextElementSibling) : null;
      var before = ref ? ref.getBoundingClientRect().top : 0;
      sec.classList.toggle('is-pin', want);
      if (target) {
        /* A load or a link at #install: aim at where the target is now. This
           also replaces a smooth fragment scroll that set off for the old spot. */
        target.scrollIntoView({ block: 'start', behavior: 'instant' });
      } else if (ref) {
        var moved = ref.getBoundingClientRect().top - before;
        if (Math.abs(moved) >= 1) window.scrollBy({ top: moved, left: 0, behavior: 'instant' });
      }
      if (want) {
        var r = outer.getBoundingClientRect();
        top = r.top;
        height = r.height;
        var q0 = clamp((progress() - 0.1) / 0.9, 0, 1);
        fpos = q0 * (N - 1);
        cur = -1;
        showPage(Math.round(fpos));
      } else {
        resetStatic();
      }
    }
    var fx = {
      on: function () { return fx.near && S.dirty; },
      read: measure,
      write: function (t, dt) {
        var want = fitsView && !reduce.matches;
        if (want !== enabled) {
          apply(want);
          loop.kick(fx);
          return;
        }
        if (!enabled) { fx.busy = false; return; }
        var p = progress();
        var z = Math.round(clamp(p * 8, 0, 1) * 1000) / 1000;
        var q = clamp((p - 0.1) / 0.9, 0, 1);
        var targetPos = q * (N - 1);
        fpos += (targetPos - fpos) * (1 - Math.exp(-dt / 0.08));
        var moving = Math.abs(targetPos - fpos) > 0.002;
        if (!moving) fpos = targetPos;
        if (z !== shownZ) { shownZ = z; screen.style.setProperty('--z', String(z)); }
        var sp = Math.round(q * 1000) / 1000;
        if (sp !== shownSp) { shownSp = sp; copy.style.setProperty('--sp', String(sp)); }
        for (var j = 0; j < N; j++) {
          var d = Math.abs(j - fpos);
          var s = (1 + 0.9 * (d < 2 ? 0.5 * (1 + Math.cos(Math.PI * d / 2)) : 0)).toFixed(3);
          if (shownS[j] !== s) { shownS[j] = s; dockEls[j].style.setProperty('--s', s); }
        }
        var i = clamp(Math.round(targetPos), 0, N - 1);
        if (i !== cur) showPage(i);
        fx.busy = moving;
      },
      busy: false,
      onResize: true
    };
    measure();
    if (fitsView && !reduce.matches) apply(true);
    /* Geist often lands after this first measure, and its metrics can make
       the stage taller or shorter than the fallback font did. Measure again
       once fonts are in; apply() keeps the view if the choice flips. */
    function refit() { vw = -1; loop.kick(fx); }
    if (doc.fonts) {
      if (doc.fonts.ready) doc.fonts.ready.then(refit, function () {});
      if (doc.fonts.addEventListener) doc.fonts.addEventListener('loadingdone', refit);
    }
    loop.gate(outer, fx, '100px 0px');
    loop.add(fx);
  }

  /* ---------- the playground ----------
     Six live cards. The controls are .js-only (the text and pictures show
     without script). Reduced motion: the dance, the dots and the paws stand
     still, the Susun windows jump at once. */
  function playCard(name) { return $('.play-card[data-card="' + name + '"]'); }

  function initDance() {
    var card = playCard('dance');
    var btn = card && $('[data-act="dance"]', card);
    var pic = card && $('.play-pic', card);
    if (!btn || !pic) return;
    var timer = 0;
    function note() {
      if (doc.hidden || !pic.animate) return;
      var n = doc.createElement('span');
      n.className = 'note';
      n.setAttribute('aria-hidden', 'true');
      n.textContent = Math.random() < 0.5 ? '♪' : '♫';
      pic.appendChild(n);
      var an = n.animate([
        { transform: 'translate(-50%,0)', opacity: 1 },
        { transform: 'translate(' + ((Math.random() - 0.5) * 200).toFixed(0) + 'px,110px) rotate(' + ((Math.random() - 0.5) * 60).toFixed(0) + 'deg)', opacity: 0 }
      ], { duration: 1600, easing: 'ease-out' });
      an.onfinish = function () { if (n.parentNode) n.parentNode.removeChild(n); };
    }
    function sync() {
      clearInterval(timer);
      timer = 0;
      if (card.classList.contains('is-on') && !reduce.matches) timer = setInterval(note, 500);
    }
    btn.addEventListener('click', function () {
      var on = card.classList.toggle('is-on');
      btn.textContent = on ? 'Pause' : 'Play a beat';
      sync();
    });
    if (reduce.addEventListener) reduce.addEventListener('change', sync);
  }

  function initTyping() {
    var card = playCard('typing');
    var btn = card && $('[data-act="start"]', card);
    var term = card && $('.term-lines', card);
    var live = card && $('[data-live]', card);
    if (!btn || !term) return;
    var LINES = ['> claude "fix the flaky test"', 'Reading src/app.rs', 'Editing 2 files', 'Running cargo test', '1857 passed'];
    function say(text) { if (live) live.textContent = text; }
    btn.addEventListener('click', function () {
      if (card.getAttribute('data-s') === 'work') return;
      card.setAttribute('data-s', 'work');
      btn.disabled = true;
      term.textContent = '';
      say('Claude is working');
      LINES.forEach(function (l, i) {
        setTimeout(function () {
          var row = doc.createElement('div');
          row.textContent = l;
          term.appendChild(row);
        }, 500 + i * 600);
      });
      setTimeout(function () { card.setAttribute('data-s', 'done'); say('Claude is done'); }, 3600);
      setTimeout(function () {
        card.setAttribute('data-s', 'idle');
        btn.disabled = false;
        term.textContent = '';
      }, 7000);
    });
  }

  function initPeek() {
    var card = playCard('peek');
    var pic = card && $('.play-pic', card);
    var mn = card && $('.mn-peek', card);
    var eyeEls = card ? $$('.pe i', card) : [];
    if (!pic || !mn || eyeEls.length < 2) return;
    var eyesOn = false;
    var box = null;
    var shown = '';
    function set(on) {
      eyesOn = on;
      card.classList.toggle('is-eyes', on);
      loop.kick(fx);
    }
    var fx = {
      on: function () { return eyesOn; },
      read: function () { box = mn.getBoundingClientRect(); },
      write: function () {
        if (!eyesOn) { if (shown) { shown = ''; eyeEls.forEach(function (e) { e.style.transform = ''; }); } return; }
        var dx = S.mx < 0 ? 0 : clamp((S.mx - (box.left + box.width / 2)) / 40, -2.5, 2.5);
        var dy = S.my < 0 ? 0 : clamp((S.my - box.bottom) / 60, 0, 2.5);
        var v = 'translate(' + dx.toFixed(2) + 'px,' + dy.toFixed(2) + 'px)';
        if (v === shown) return;
        shown = v;
        eyeEls.forEach(function (e) { e.style.transform = v; });
      }
    };
    loop.add(fx);
    /* Mouse and pen: while the pointer is inside the picture. Touch: a tap toggles. */
    pic.addEventListener('pointerenter', function (e) { if (e.pointerType !== 'touch') set(true); });
    pic.addEventListener('pointerleave', function (e) { if (e.pointerType !== 'touch') set(false); });
    pic.addEventListener('pointerup', function (e) { if (e.pointerType === 'touch') set(!eyesOn); });
    pic.addEventListener('pointermove', function () { if (eyesOn) loop.kick(fx); }, { passive: true });
    (function blink() {
      setTimeout(function () {
        if (eyesOn && !doc.hidden) {
          card.classList.add('is-blink');
          setTimeout(function () { card.classList.remove('is-blink'); }, 130);
        }
        blink();
      }, rnd(2200, 4700));
    })();
  }

  function initSusunCard() {
    var card = playCard('susun');
    var wins = card ? $$('.play-pic .win', card) : [];
    var label = card && $('.ar-t', card);
    var live = card && $('[data-live]', card);
    var btns = card ? $$('.lay button', card) : [];
    if (wins.length !== 3 || !label || !btns.length) return;
    var LAYOUTS = {
      cols: [[3, 22, 30, 72], [35, 22, 30, 72], [67, 22, 30, 72]],
      main: [[3, 22, 60, 72], [65, 22, 32, 35], [65, 59, 32, 35]],
      top: [[3, 22, 94, 35], [3, 59, 46, 35], [51, 59, 46, 35]]
    };
    var timers = [];
    function say(text) {
      label.textContent = text;
      if (live) live.textContent = text;
    }
    btns.forEach(function (b) {
      b.addEventListener('click', function () {
        timers.forEach(clearTimeout);
        timers = [];
        btns.forEach(function (x) { x.setAttribute('aria-pressed', x === b ? 'true' : 'false'); });
        var l = LAYOUTS[b.getAttribute('data-l')];
        function place(i) {
          var w = wins[i].style;
          w.left = l[i][0] + '%';
          w.top = l[i][1] + '%';
          w.width = l[i][2] + '%';
          w.height = l[i][3] + '%';
        }
        if (reduce.matches) {
          wins.forEach(function (w, i) { place(i); });
          say('Arranged');
          return;
        }
        wins.forEach(function (w, i) {
          timers.push(setTimeout(function () {
            say('Tile ' + (i + 1) + ' of 3');
            place(i);
          }, i * 260));
        });
        timers.push(setTimeout(function () { say('Arranged'); }, 900));
      });
    });
  }

  function initDraw() {
    var card = playCard('draw');
    var pic = card && $('.play-pic', card);
    var cv = card && $('.draw-cv', card);
    var sel = card && $('.sel', card);
    var toolBtns = card ? $$('.tools button', card) : [];
    if (!pic || !cv || !sel || !toolBtns.length) return;
    var ctx = cv.getContext('2d');
    if (!ctx) return;
    /* The selection box inside the picture, as fractions (matches .sel in style.css). */
    var X0 = 0.12;
    var Y0 = 0.22;
    var X1 = 0.88;
    var Y1 = 0.76;
    var tool = 'pen';
    var strokes = [];
    var current = null;
    var W = 0;
    var H = 0;
    var picBox = null;
    function redraw() {
      ctx.clearRect(0, 0, W, H);
      ctx.strokeStyle = '#E2683C';
      ctx.fillStyle = '#E2683C';
      ctx.lineWidth = 3;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      var list = current ? strokes.concat([current]) : strokes;
      list.forEach(function (s) {
        if (s.type === 'box') {
          var a = s.pts[0];
          var b = s.pts[s.pts.length - 1];
          ctx.strokeRect(a[0] * W, a[1] * H, (b[0] - a[0]) * W, (b[1] - a[1]) * H);
        } else if (s.pts.length === 1) {
          ctx.beginPath();
          ctx.arc(s.pts[0][0] * W, s.pts[0][1] * H, 1.8, 0, 6.283);
          ctx.fill();
        } else {
          ctx.beginPath();
          ctx.moveTo(s.pts[0][0] * W, s.pts[0][1] * H);
          for (var i = 1; i < s.pts.length; i++) ctx.lineTo(s.pts[i][0] * W, s.pts[i][1] * H);
          ctx.stroke();
        }
      });
    }
    /* Strokes are kept as fractions of the picture, so a resize redraws them in place. */
    var fx = {
      on: function () { return false; },
      read: function () { picBox = pic.getBoundingClientRect(); },
      write: function () {
        var r = Math.min(2, window.devicePixelRatio || 1);
        var w = Math.max(1, Math.round(picBox.width));
        var h = Math.max(1, Math.round(picBox.height));
        if (cv.width !== Math.round(w * r) || cv.height !== Math.round(h * r)) {
          cv.width = Math.round(w * r);
          cv.height = Math.round(h * r);
        }
        W = w;
        H = h;
        ctx.setTransform(r, 0, 0, r, 0, 0);
        redraw();
      },
      onResize: true
    };
    loop.add(fx);
    function point(e) {
      var r = pic.getBoundingClientRect();
      return [clamp((e.clientX - r.left) / Math.max(r.width, 1), X0, X1), clamp((e.clientY - r.top) / Math.max(r.height, 1), Y0, Y1)];
    }
    sel.addEventListener('pointerdown', function (e) {
      if (e.pointerType === 'mouse' && e.button !== 0) return;
      try { sel.setPointerCapture(e.pointerId); } catch (err) { /* the pointer may already be gone */ }
      current = { type: tool, pts: [point(e)] };
      redraw();
    });
    sel.addEventListener('pointermove', function (e) {
      if (!current) return;
      var p = point(e);
      if (current.type === 'box') current.pts = [current.pts[0], p]; else current.pts.push(p);
      redraw();
    });
    function end() {
      if (!current) return;
      strokes.push(current);
      current = null;
      redraw();
    }
    sel.addEventListener('pointerup', end);
    sel.addEventListener('pointercancel', end);
    toolBtns.forEach(function (b) {
      b.addEventListener('click', function () {
        var t = b.getAttribute('data-t');
        if (t === 'clear') { strokes = []; current = null; redraw(); return; }
        if (t === 'copy') { if (notchApi) notchApi.showCopy('Screenshot 640 x 360'); return; }
        tool = t;
        toolBtns.forEach(function (x) {
          if (x.getAttribute('data-t') === 'pen' || x.getAttribute('data-t') === 'box') x.setAttribute('aria-pressed', x === b ? 'true' : 'false');
          x.classList.toggle('on', x === b);
        });
      });
    });
  }

  function initMiniClocks() {
    var d = new Date();
    var t = pad(d.getHours()) + ':' + pad(d.getMinutes());
    $$('.clk').forEach(function (el) { el.textContent = t; });
  }

  /* ---------- the walking cat and the bottom easter egg ----------
     A fixed 3 px trail at the very bottom and a 30 px cat head that walks
     along it with page progress, bobbing only while the page scrolls. The
     track ends 12 px before the go-to-top button (the right edge minus the
     edge gutter, 48 px and 12 px), so it never lies over it or a footer link.
     At the end of the page the cat curls up and sleeps (z's float up); only
     then is it a real, focusable button, "Wake the cat". Waking it: a
     stretch, "mrrow? back to the top?", and a dash left along the track while
     the page scrolls to the top (smooth only when motion is welcome). While
     it walks it is aria-hidden, tabindex -1 and not clickable. Reduced motion:
     no float or dash; it still wakes and scrolls at once. */
  function initWalk() {
    if (!$('#h-hero') || !$('.to-top')) return;
    var bar = doc.createElement('div');
    bar.className = 'walk';
    bar.innerHTML = '<span class="walk-trail" aria-hidden="true"></span>' +
      '<div class="walk-pos"><button class="walk-cat" type="button" tabindex="-1" aria-hidden="true">' +
      '<svg viewBox="0 0 256 256" focusable="false"><use href="#cat-idle"/></svg></button>' +
      '<span class="zz" aria-hidden="true">z</span><span class="zz" aria-hidden="true">z</span><span class="zz" aria-hidden="true">z</span>' +
      '<p class="walk-bubble" role="status" aria-live="polite"></p></div>';
    doc.body.appendChild(bar);
    var trail = $('.walk-trail', bar);
    var pos = $('.walk-pos', bar);
    var btn = $('.walk-cat', bar);
    var use = $('use', bar);
    var bubble = $('.walk-bubble', bar);
    var wide = window.matchMedia('(min-width: 40em)');
    var state = 'walk';
    var slept = false;
    var sleepTimer = 0;
    var sayTimer = 0;
    var x = 0;
    var shownX = -1;
    var track = 0;
    var vw = 0;
    function say(text, ms) {
      bubble.textContent = text;
      bubble.classList.add('on');
      clearTimeout(sayTimer);
      sayTimer = setTimeout(function () { bubble.classList.remove('on'); }, ms);
    }
    function asleep(on) {
      pos.classList.toggle('is-asleep', on);
      btn.classList.toggle('is-asleep', on);
      use.setAttribute('href', on ? '#cat-sleep' : '#cat-idle');
      if (on) {
        btn.removeAttribute('aria-hidden');
        btn.setAttribute('tabindex', '0');
        btn.setAttribute('aria-label', 'Wake the cat');
      } else {
        /* Never hide the button while it holds focus: hand focus to the top of the page. */
        if (doc.activeElement === btn) {
          var top = $('#top');
          if (top) top.focus({ preventScroll: true });
          else btn.blur();
        }
        btn.setAttribute('aria-hidden', 'true');
        btn.setAttribute('tabindex', '-1');
        btn.removeAttribute('aria-label');
      }
    }
    function wake() {
      if (state !== 'asleep') return;
      state = 'dash';
      asleep(false);
      say('mrrow? back to the top?', 2800);
      var top = $('#top');
      if (top) top.focus({ preventScroll: true });
      if (reduce.matches) {
        window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
        state = 'walk';
        loop.kick(fx);
        return;
      }
      btn.classList.add('is-stretch');
      setTimeout(function () {
        btn.classList.remove('is-stretch');
        btn.classList.add('is-dash');
        window.scrollTo({ top: 0, left: 0, behavior: reduce.matches ? 'instant' : 'smooth' });
        loop.kick(fx);
        /* Failsafe if the scroll never reaches the top. */
        setTimeout(function () { if (state === 'dash') endDash(); }, 4000);
      }, 450);
    }
    function endDash() {
      state = 'walk';
      btn.classList.remove('is-dash');
      loop.kick(fx);
    }
    btn.addEventListener('click', wake);
    var fx = {
      on: function () { return S.dirty || state !== 'walk'; },
      /* S.moving is worked out here, not in the progress task: that task runs only
         on dirty frames, so a value kept there would stay true after the scroll stops
         and keep the loop (and the bob) going. */
      read: function () { vw = root.clientWidth; S.moving = performance.now() - S.scrollAt < 160; },
      write: function () {
        var edge = wide.matches ? 24 : 16;
        track = Math.max(0, vw - edge - 48 - 12 - 30);
        x = Math.round(S.p * track);
        if (state === 'asleep') x = Math.round(track);
        if (x !== shownX) {
          shownX = x;
          pos.style.transform = 'translate3d(' + x + 'px,0,0)';
          trail.style.width = (x + 15) + 'px';
          /* The bubble hangs to the left of the cat; near the left edge it hangs to the right. */
          var flip = x < 200;
          if (bubble.classList.contains('flip') !== flip) bubble.classList.toggle('flip', flip);
        }
        var walking = state === 'walk' && S.moving && !reduce.matches;
        if (btn.classList.contains('is-walking') !== walking) btn.classList.toggle('is-walking', walking);
        var atEnd = S.max > 0 && S.sy >= S.max - 2;
        if (state === 'walk') {
          if (atEnd && !slept && !sleepTimer) {
            sleepTimer = setTimeout(function () {
              sleepTimer = 0;
              if (state !== 'walk' || S.sy < S.max - 2) return;
              slept = true;
              state = 'asleep';
              asleep(true);
              loop.kick(fx);
            }, 500);
          }
          if (!atEnd && S.sy < S.max - 120) { slept = false; clearTimeout(sleepTimer); sleepTimer = 0; }
        } else if (state === 'dash' && S.sy <= 2) {
          endDash();
        } else if (state === 'asleep' && S.sy < S.max - 120) {
          /* The reader scrolled away on their own: it wakes quietly. */
          state = 'walk';
          slept = false;
          asleep(false);
        }
        fx.busy = state === 'dash' || (state === 'walk' && S.moving);
      },
      busy: false,
      onResize: true
    };
    loop.add(fx);
  }

  /* ---------- reveal: once, for every browser ----------
     .reveal is hidden by CSS only under html.js. An element already on screen
     or scrolled past at start is shown at once; the others get .is-in the first
     time they intersect and are then unobserved, so nothing replays on scroll
     back. Reduced motion, or no IntersectionObserver: everything shows now. */
  function initReveal() {
    var items = $$('.reveal');
    if (!items.length) return;
    function show(el) { el.classList.add('is-in'); }
    function showAll() { items.forEach(show); }
    if (reduce.matches || !('IntersectionObserver' in window)) { showAll(); return; }
    var fold = window.innerHeight * 0.9;
    var waiting = items.filter(function (el) {
      if (el.getBoundingClientRect().top < fold) { show(el); return false; }
      return true;
    });
    if (!waiting.length) return;
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting) return;
        show(e.target);
        io.unobserve(e.target);
      });
    }, { rootMargin: '0px 0px -10% 0px' });
    waiting.forEach(function (el) { io.observe(el); });
    if (reduce.addEventListener) {
      reduce.addEventListener('change', function () {
        if (!reduce.matches) return;
        io.disconnect();
        showAll();
      });
    }
  }

  /* One part failing must not stop the others, and must not leave .reveal hidden. */
  function safe(fn) {
    try { return fn(); } catch (e) { return null; }
  }

  root.classList.add('js');
  loop = safe(makeLoop);
  safe(initTheme);
  safe(initMenu);
  safe(initCopy);
  safe(initCoats);
  safe(initMiniClocks);
  if (loop) {
    safe(initSky);
    safe(initNotch);
    safe(initHeroCat);
    safe(initMarquee);
    safe(initHeader);
    safe(initToTop);
    safe(initStory);
    safe(initDance);
    safe(initTyping);
    safe(initPeek);
    safe(initSusunCard);
    safe(initDraw);
    safe(initWalk);
  }
  safe(initReveal);
  /* The head script removes .js at DOMContentLoaded unless this is set (script blocked or broken). */
  root.classList.add('ready');
})();
