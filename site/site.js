/* Vale site script. One plain deferred file, no framework, no library.
   Every part checks that its elements exist and does nothing otherwise, so
   the same file serves index, 404, privacy and terms. */
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

  /* ---------- theme button ---------- */
  var ICON_SUN = '<svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2.2M12 19.3v2.2M2.5 12h2.2M19.3 12h2.2M5.3 5.3l1.6 1.6M17.1 17.1l1.6 1.6M5.3 18.7l1.6-1.6M17.1 6.9l1.6-1.6"/></svg>';
  var ICON_MOON = '<svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z"/></svg>';

  function initTheme() {
    var btn = $('.theme-btn');
    if (!btn) return;
    /* Dark is the default whatever the system scheme says; only a stored or
       clicked choice of light changes it. */
    function current() {
      return root.getAttribute('data-theme') === 'light' ? 'light' : 'dark';
    }
    function paint() {
      var isDark = current() === 'dark';
      btn.setAttribute('aria-label', isDark ? 'Switch to light theme' : 'Switch to dark theme');
      btn.innerHTML = isDark ? ICON_SUN : ICON_MOON;
    }
    btn.addEventListener('click', function () {
      var next = current() === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-theme', next);
      store('vale-theme', next);
      paint();
    });
    paint();
    btn.hidden = false;
  }

  /* ---------- menu button (below the nav breakpoint) ---------- */
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
    var wide = window.matchMedia('(min-width: 76em)');
    if (wide.addEventListener) wide.addEventListener('change', function () { setOpen(false, false); });
  }

  /* ---------- copy buttons ---------- */
  var notchApi = null;

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
      try { area.select(); area.setSelectionRange(0, text.length); ok = doc.execCommand('copy'); } catch (e) { ok = false; }
      doc.body.removeChild(area);
      if (keep && keep.focus) keep.focus();
      return ok;
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text).then(function () { return true; }, function () { return fallback(); });
    }
    return Promise.resolve(fallback());
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
            if (notchApi) notchApi.showCopy(text);
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

  /* ---------- the cat, drawn from the app's geometry (src/ui/robot.rs, cat_style.rs) ---------- */
  var COATS = {
    terracotta: { head: '#E2683C', eyes: '#17120F' },
    calico: { head: '#F4EDE4', eyes: '#17120F' },
    white: { head: '#F6F1EA', eyes: '#17120F' },
    black: { head: '#1E1B22', eyes: '#F2B33D' }
  };
  /* The idle look in the app: head slide 7 dip, tilt 6 degrees, extra eye slide
     4 dip, on a cat drawn 26 dip wide (64 grid units): 7 / (26 / 64) units. */
  var LOOK_HEAD = 17.23;
  var LOOK_TILT = 6;
  var LOOK_EYES = 9.85;
  var EAR_L = 'M9 37L12 10L27 26Z';
  var EAR_R = 'M55 37L52 10L37 26Z';
  var HEAD = 'M7 50V41Q7 24 32 24Q57 24 57 41V50Q57 54 53 54H11Q7 54 7 50Z';

  /* Glasses sit with the eyes (they follow the eye slide), so catMarkup draws them;
     these are the accessories drawn after the eyes. */
  function accessory(name) {
    switch (name) {
      case 'bow':
        return '<g fill="#D23F6A" stroke="#D23F6A" stroke-width="1.6" stroke-linejoin="round"><path d="M45 23L37 18.5V27.5Z"/><path d="M45 23L53 18.5V27.5Z"/></g><circle cx="45" cy="23" r="2.4" fill="#F07A9A"/>';
      case 'nightcap':
        return '<path d="M19 25.8Q32 21.5 45 25.8Q40 6 14.5 7Q10.5 17 19 25.8Z" fill="#2F4A7A" stroke="#2F4A7A" stroke-width="1.2" stroke-linejoin="round"/><path d="M19 25.8Q32 21.5 45 25.8" fill="none" stroke="#F4EDE4" stroke-width="4.4" stroke-linecap="round"/><circle cx="14.5" cy="7" r="3" fill="#F4EDE4"/>';
      case 'scarf':
        return '<path d="M13 52L22 52L23 60.5L15 61.5Z" fill="#C48A18" stroke="#C48A18" stroke-width="1.6" stroke-linejoin="round"/><path d="M9.5 47H54.5Q58 47 58 50.5V51Q58 55 54 55H10Q6 55 6 51V50.5Q6 47 9.5 47Z" fill="#E0A526"/>';
      case 'flower':
        var petals = '';
        for (var i = 0; i < 5; i++) {
          var a = (-90 + 72 * i) * Math.PI / 180;
          petals += '<circle cx="' + (16 + 3.6 * Math.cos(a)).toFixed(2) + '" cy="' + (23 + 3.6 * Math.sin(a)).toFixed(2) + '" r="2.6" fill="#F29BB5"/>';
        }
        return petals + '<circle cx="16" cy="23" r="2" fill="#F2B33D"/>';
      case 'crown':
        return '<path d="M23.5 27.5V19L28 23.5L32 16L36 23.5L40.5 19V27.5Z" fill="#F2B33D" stroke="#F2B33D" stroke-width="1.4" stroke-linejoin="round"/><path d="M25 25.8H39" stroke="#C8861C" stroke-width="1.8" stroke-linecap="round"/>';
      default:
        return '';
    }
  }

  /* Paint order from docs/memory/cat-accessories.md: rim, silhouette, patches, eyes, accessories. */
  function catMarkup(look, uid) {
    var coat = COATS[look.coat] || COATS.terracotta;
    var black = look.coat === 'black';
    var frame = black ? '#F4EDE4' : '#17120F';
    var s = '<defs><mask id="' + uid + '-m" maskUnits="userSpaceOnUse" x="-30" y="-20" width="130" height="110">' +
      '<g fill="#fff" stroke="#fff" stroke-width="5" stroke-linejoin="round"><path d="' + EAR_L + '"/><path d="' + EAR_R + '"/></g>' +
      '<path d="' + HEAD + '" fill="#fff"/></mask></defs>';
    s += '<g class="cat-move">';
    if (black) {
      s += '<g fill="#8E7CB8" stroke="#8E7CB8" stroke-linejoin="round"><path d="' + EAR_L + '" stroke-width="8.2"/><path d="' + EAR_R + '" stroke-width="8.2"/><path d="' + HEAD + '" stroke-width="3.2"/></g>';
    }
    s += '<g fill="' + coat.head + '" stroke="' + coat.head + '" stroke-width="5" stroke-linejoin="round"><path d="' + EAR_L + '"/><path d="' + EAR_R + '"/></g>';
    s += '<path d="' + HEAD + '" fill="' + coat.head + '"/>';
    if (look.coat === 'calico') {
      s += '<g mask="url(#' + uid + '-m)"><path d="M-12 -12H30Q30 25 27 28Q19 34 4 40L-12 42Z" fill="#E2683C"/><path d="M76 -12H34Q34 25 37 27Q46 32 60 36L76 38Z" fill="#2B2420"/></g>';
    }
    s += '<g class="cat-eyes"><g fill="' + coat.eyes + '"><rect x="21.2" y="35" width="5.6" height="10" rx="2.8"/><rect x="37.2" y="35" width="5.6" height="10" rx="2.8"/></g>';
    if (look.accessory === 'glasses') {
      s += '<g fill="none" stroke="' + frame + '" stroke-width="1.9" stroke-linecap="round"><circle cx="24" cy="40" r="6.6"/><circle cx="40" cy="40" r="6.6"/><path d="M30.6 40H33.4"/></g>';
    }
    s += '</g>';
    if (look.accessory !== 'glasses') s += accessory(look.accessory);
    return s + '</g>';
  }

  var cats = [];
  var look = { coat: 'terracotta', accessory: 'none' };
  var bigVisible = false;
  var lastX = null;
  var catRaf = 0;
  var catLast = 0;

  function makeCat(svg, uid, isActive) {
    var c = { svg: svg, uid: uid, v: 0, target: 0, active: isActive, move: null, eyes: null };
    c.bind = function () { c.move = $('.cat-move', svg); c.eyes = $('.cat-eyes', svg); };
    c.apply = function () {
      if (!c.move) return;
      c.move.setAttribute('transform', 'translate(' + (LOOK_HEAD * c.v).toFixed(3) + ' 0) rotate(' + (LOOK_TILT * c.v).toFixed(3) + ' 32 40)');
      if (c.eyes) c.eyes.setAttribute('transform', 'translate(' + (LOOK_EYES * c.v).toFixed(3) + ' 0)');
    };
    c.render = function (l) {
      svg.innerHTML = catMarkup(l, uid);
      c.bind();
      c.apply();
    };
    c.bind();
    cats.push(c);
    return c;
  }

  function catStep(t) {
    var dt = Math.min(0.1, (t - catLast) / 1000);
    catLast = t;
    var k = 1 - Math.exp(-dt / 0.12);
    var moving = false;
    cats.forEach(function (c) {
      if (c.v === c.target) return;
      c.v += (c.target - c.v) * k;
      if (Math.abs(c.target - c.v) < 0.002) c.v = c.target; else moving = true;
      c.apply();
    });
    catRaf = moving ? requestAnimationFrame(catStep) : 0;
  }

  /* v = clamp(offset / distance to that side's viewport edge, -1, 1), as in the app. */
  function pointCats(x) {
    var changed = false;
    cats.forEach(function (c) {
      if (!c.active()) return;
      var r = c.svg.getBoundingClientRect();
      var cx = r.left + r.width / 2;
      var off = x - cx;
      var dist = off >= 0 ? window.innerWidth - cx : cx;
      var v = clamp(off / Math.max(dist, 1), -1, 1);
      if (Math.abs(v - c.target) < 0.02) return;
      c.target = v;
      changed = true;
      if (reduce.matches) { c.v = v; c.apply(); }
    });
    if (changed && !reduce.matches && !catRaf) {
      catLast = performance.now();
      catRaf = requestAnimationFrame(catStep);
    }
  }

  function initCats() {
    var bigSvg = $('#big-cat');
    var bigCat = bigSvg ? makeCat(bigSvg, 'bigcat', function () { return bigVisible; }) : null;
    var notchSvg = $('#notch-cat');
    var notchCat = notchSvg ? makeCat(notchSvg, 'notchcat', function () { var n = $('#notch'); return !!n && n.getAttribute('data-state') === 'cat'; }) : null;
    if (!bigCat && !notchCat) return null;

    function restyle() {
      if (bigCat) bigCat.render(look);
      if (notchCat) notchCat.render(look);
    }

    var pickers = $$('input[name="coat"], input[name="accessory"]');
    pickers.forEach(function (input) {
      input.addEventListener('change', function () {
        if (!input.checked) return;
        look[input.name] = input.value;
        restyle();
      });
    });

    /* Browsers restore checked radios on back/forward (Firefox also on reload)
       without a change event, so the look is read from the pickers, not assumed. */
    function syncLook() {
      var changed = false;
      pickers.forEach(function (input) {
        if (input.checked && look[input.name] !== input.value) {
          look[input.name] = input.value;
          changed = true;
        }
      });
      if (changed) restyle();
    }
    syncLook();
    window.addEventListener('pageshow', syncLook);

    if (bigSvg && 'IntersectionObserver' in window) {
      new IntersectionObserver(function (entries) {
        bigVisible = entries[entries.length - 1].isIntersecting;
      }, { rootMargin: '100px 0px' }).observe(bigSvg);
    }

    doc.addEventListener('pointermove', function (e) {
      lastX = e.clientX;
      if (e.pointerType === 'touch' || reduce.matches) return;
      pointCats(e.clientX);
    }, { passive: true });
    /* A tap turns the cat toward the tap; with reduced motion every press does, and nothing else moves it. */
    doc.addEventListener('pointerdown', function (e) {
      lastX = e.clientX;
      if (e.pointerType === 'touch' || reduce.matches) pointCats(e.clientX);
    }, { passive: true });

    var bubble = $('#cat-greeting');
    if (bubble) {
      var h = new Date().getHours();
      var text = h < 4 ? 'Up late?' : h < 11 ? 'Good morning' : h < 18 ? 'Good afternoon' : 'Good evening';
      bubble.textContent = text;
      bubble.hidden = false;
    }

    return {
      entered: function () { if (lastX !== null) pointCats(lastX); }
    };
  }

  /* ---------- one scroll loop for every scroll-linked effect ----------
     One passive scroll listener (plus resize and load) asks for at most one
     animation frame. In that frame every effect that is switched on reads
     first (rects, sizes) and then all of them write (classes, transform,
     opacity), so reads and writes never interleave. An effect is on while
     fx.on() says so, usually while an IntersectionObserver says its section
     is near the viewport (fx.near); fx.flush runs it once more after it
     switches off, so it can settle. No wheel or touch handling, no snapping. */
  function makeLoop() {
    var list = [];
    var pending = false;
    function frame() {
      pending = false;
      var run = [];
      var i;
      for (i = 0; i < list.length; i++) {
        if (list[i].on() || list[i].flush) { list[i].flush = false; run.push(list[i]); }
      }
      for (i = 0; i < run.length; i++) run[i].read();
      for (i = 0; i < run.length; i++) run[i].write();
    }
    function schedule() {
      if (pending) return;
      pending = true;
      requestAnimationFrame(frame);
    }
    window.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', schedule);
    window.addEventListener('load', schedule);
    if (reduce.addEventListener) reduce.addEventListener('change', function () { list.forEach(function (fx) { fx.flush = true; }); schedule(); });
    return {
      add: function (fx) { fx.flush = true; list.push(fx); schedule(); return fx; },
      kick: function (fx) { if (fx) fx.flush = true; schedule(); },
      /* fx.near follows el, with margin around the viewport. Without an
         observer the effect simply stays on. */
      gate: function (el, fx, margin) {
        if (!('IntersectionObserver' in window)) { fx.near = true; return; }
        fx.near = false;
        new IntersectionObserver(function (entries) {
          fx.near = entries[entries.length - 1].isIntersecting;
          fx.flush = true;
          schedule();
        }, { rootMargin: margin || '200px 0px' }).observe(el);
      }
    };
  }
  var loop = null;

  /* ---------- the notch ---------- */
  var LYRICS = [
    'Lights low over the harbour',
    'We count the hours by the bell',
    'Nothing here is a real song',
    'Hum along, it is only a demo',
    'Home is a few streets away'
  ];

  function initNotch(catApi) {
    var notch = $('#notch');
    var face = notch && $('.notch-face', notch);
    var panel = notch && $('#notch-panel');
    if (!notch || !face || !panel) return;
    face.removeAttribute('tabindex');

    var state = 'idle';
    var sectionState = 'idle';
    var copyTimer = 0;
    var clockTimer = 0;
    var clockEl = $('#notch-clock');
    var open = false;
    var pinned = false;
    var closeTimer = 0;
    var hideTimer = 0;
    var openTimer = 0;
    var dockTimer = 0;
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
      if (next === 'cat' && catApi) catApi.entered();
    }

    doc.addEventListener('visibilitychange', function () {
      root.classList.toggle('is-hidden', doc.hidden);
      tickClock();
    });

    /* ----- the peek and the copy notice must not cover text -----
       The panel peek (400 x 128, 64 high on phones) and the section's clipboard
       state (340 x 56) are wider than the idle notch and would lie over section
       text, so they are shown only while the band under the notch holds no text
       element (the tags below). When it does, the panel state shows the idle
       notch instead and the clipboard state keeps its content at the idle
       footprint (.is-tight). Sizes are mirrored from style.css. The small
       states (at most 45 px high) may pass over text, like a real notch. The
       copy notice after a Copy button is a 3 s toast and is exempt. Checked on
       scroll frames and resize only, and only while one of the two states is the
       section's; shrinking is immediate, growing waits FIT_GROW_MS of free band. */
    var TEXT_SEL = 'h1, h2, h3, p, li, dt, dd, figcaption, code, pre, label, legend, td, th, blockquote';
    var FIT_MARGIN_Y = 16;
    var FIT_MARGIN_X = 8;
    var FIT_GROW_MS = 150;
    var textEls = null;
    var fits = true;
    var growTimer = 0;
    /* Susun: the tile step (0 to 3) while its stage is pinned, -1 otherwise. */
    var arrangeStep = -1;
    var arrangeN = $('#notch-ar-n');

    /* Mirror style.css from the same source it uses: 100vw and the 32.5em media
       query both count the scrollbar, root.clientWidth does not. */
    var phonePeek = window.matchMedia('(max-width: 32.5em)');
    function peekSize(name) {
      var vw = window.innerWidth;
      if (name === 'panel') return { w: Math.min(400, vw - 32), h: phonePeek.matches ? 64 : 128 };
      return { w: Math.min(340, vw - 32), h: 56 };
    }
    function bandBlocked(name) {
      if (!textEls) textEls = $$(TEXT_SEL).filter(function (el) { return !notch.contains(el); });
      var size = peekSize(name);
      var mid = root.clientWidth / 2;
      var left = mid - size.w / 2 - FIT_MARGIN_X;
      var right = mid + size.w / 2 + FIT_MARGIN_X;
      var bottom = size.h + FIT_MARGIN_Y;
      for (var i = 0; i < textEls.length; i++) {
        var r = textEls[i].getBoundingClientRect();
        if (r.width && r.height && r.bottom > 0 && r.top < bottom && r.right > left && r.left < right) return true;
      }
      return false;
    }
    function peekState(name) { return name === 'panel' || name === 'clipboard'; }
    /* The state the notch shows: the 3 s copy toast first, then Susun while its
       stage is pinned, then the section's own state. The Susun section asks
       for "arrange", which is the idle notch whenever the stage is not pinned. */
    function refresh() {
      var want = copyTimer ? 'clipboard'
        : arrangeStep >= 0 ? 'arrange'
        : (sectionState === 'arrange' || (sectionState === 'panel' && !fits)) ? 'idle'
        : sectionState;
      setState(want);
      notch.classList.toggle('is-tight', !copyTimer && sectionState === 'clipboard' && !fits);
    }
    function enterSection(name) {
      sectionState = name;
      clearTimeout(growTimer);
      growTimer = 0;
      fits = !(peekState(name) && !open && !copyTimer && bandBlocked(name));
      refresh();
    }
    /* fitCheck runs in the shared scroll loop, only while the section's state
       is panel or clipboard and nothing else (open panel, copy toast) rules. */
    var blocked = false;
    var fitFx = {
      on: function () { return !open && !copyTimer && peekState(sectionState); },
      read: function () { blocked = fitFx.on() && bandBlocked(sectionState); },
      write: function () {
        if (!fitFx.on()) return;
        if (blocked) {
          clearTimeout(growTimer);
          growTimer = 0;
          if (fits) { fits = false; refresh(); }
        } else if (!fits && !growTimer) {
          growTimer = setTimeout(function () {
            growTimer = 0;
            if (!fitFx.on()) return;
            if (!bandBlocked(sectionState)) { fits = true; refresh(); }
          }, FIT_GROW_MS);
        }
      }
    };
    if (loop) loop.add(fitFx);

    /* ----- which state the section in view asks for ----- */
    var sections = $$('[data-notch]');
    if (sections.length && 'IntersectionObserver' in window) {
      var inBand = [];
      var io = new IntersectionObserver(function (entries) {
        entries.forEach(function (e) {
          var i = inBand.indexOf(e.target);
          if (e.isIntersecting && i < 0) inBand.push(e.target);
          if (!e.isIntersecting && i >= 0) inBand.splice(i, 1);
        });
        var chosen = null;
        sections.forEach(function (s) { if (inBand.indexOf(s) >= 0) chosen = s; });
        if (!chosen) return;
        enterSection(chosen.getAttribute('data-notch') || 'idle');
      }, { rootMargin: '-40% 0px -55% 0px' });
      sections.forEach(function (s) { io.observe(s); });
    }

    /* ----- lyric line follows the Music section's scroll progress ----- */
    var lyricEl = $('#notch-lyric');
    var musicSection = $('.f-music');
    var lyricIndex = 0;
    var lyricNext = 0;
    var lyricFlip = false;
    /* Swapping between two identical animations restarts it without a reflow. */
    function setLyric(i) {
      if (!lyricEl || i === lyricIndex) return;
      lyricIndex = i;
      lyricEl.textContent = LYRICS[i];
      lyricEl.classList.remove('swap-a', 'swap-b');
      if (!reduce.matches) {
        lyricFlip = !lyricFlip;
        lyricEl.classList.add(lyricFlip ? 'swap-a' : 'swap-b');
      }
    }
    if (lyricEl && musicSection && loop) {
      var lyricFx = {
        on: function () { return lyricFx.near; },
        read: function () {
          var r = musicSection.getBoundingClientRect();
          var p = clamp((window.innerHeight * 0.5 - r.top) / Math.max(r.height, 1), 0, 0.999);
          lyricNext = Math.floor(p * LYRICS.length);
        },
        write: function () { setLyric(lyricNext); }
      };
      loop.gate(musicSection, lyricFx, '0px');
      loop.add(lyricFx);
    }

    /* ----- copy notice ----- */
    /* The full notice is 340 x 56 and would cover the nav and the Menu button,
       so while any of the header is on screen the notch carries .is-compact and
       the notice keeps the idle footprint (style.css). Without an observer it
       stays compact everywhere. */
    var header = $('.site-header');
    if (header && 'IntersectionObserver' in window) {
      new IntersectionObserver(function (entries) {
        notch.classList.toggle('is-compact', entries[entries.length - 1].isIntersecting);
      }).observe(header);
    } else {
      notch.classList.add('is-compact');
    }
    notchApi = {
      showCopy: function (text) {
        if (open) return;
        var line = text.length > 90 ? text.slice(0, 90) : text;
        var preview = $('#notch-clip-preview');
        if (preview) preview.textContent = line;
        clearTimeout(copyTimer);
        copyTimer = setTimeout(function () {
          copyTimer = 0;
          enterSection(sectionState);
        }, 3000);
        refresh();
      },
      /* Susun's scrub: the tile step while its stage is pinned, -1 when not. */
      arrange: function (step) {
        if (step === arrangeStep) return;
        arrangeStep = step;
        if (step >= 0) {
          notch.setAttribute('data-ar', step >= 3 ? 'done' : 'pick');
          if (arrangeN && step < 3) arrangeN.textContent = String(step + 1);
        }
        refresh();
      }
    };

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

    /* ----- Dock magnification: scale = 1 + 0.9 * falloff(d), R = 2 x pitch, eased with tau 70 ms ----- */
    var dock = $('.dock', panel);
    var dockItems = tabs.map(function (t) { return { ico: $('.dock-ico', t), cur: 1, target: 1 }; });
    var fine = window.matchMedia('(pointer: fine)');
    var narrow = window.matchMedia('(max-width: 32.5em)');
    var dockRaf = 0;
    var dockLast = 0;
    function dockApply() {
      dockItems.forEach(function (it) { it.ico.style.transform = it.cur === 1 ? '' : 'scale(' + it.cur.toFixed(3) + ')'; });
    }
    function dockStep(t) {
      var dt = Math.min(0.1, (t - dockLast) / 1000);
      dockLast = t;
      var k = 1 - Math.exp(-dt / 0.07);
      var moving = false;
      dockItems.forEach(function (it) {
        it.cur += (it.target - it.cur) * k;
        if (Math.abs(it.target - it.cur) < 0.002) it.cur = it.target; else moving = true;
      });
      dockApply();
      dockRaf = moving ? requestAnimationFrame(dockStep) : 0;
    }
    function dockKick() {
      if (reduce.matches) {
        dockItems.forEach(function (it) { it.cur = it.target; });
        dockApply();
        return;
      }
      if (!dockRaf) {
        dockLast = performance.now();
        dockRaf = requestAnimationFrame(dockStep);
      }
    }
    function dockReset() {
      dockItems.forEach(function (it) { it.target = 1; });
      dockKick();
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
      dockKick();
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
      enterSection(sectionState);
    }

    /* Hover intent: the idle notch (y 0..34) overlaps the nav row (y 6..50),
       so a pointer sweeping along the header crosses it. The panel opens only
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
      if (!open) armHoverOpen(e);
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

  /* ---------- hero: the headline steps back as the hero scrolls away ----------
     q = how far the hero has left the viewport (0 at the top, 1 when its
     bottom passes the top edge). The headline scales 1 -> 0.92 and fades
     1 -> 0.2 with q, both ways. Transform and opacity only; will-change only
     while the hero is near the viewport. Reduced motion: nothing moves. */
  function initHero() {
    var hero = $('.hero');
    var title = $('.hero-title');
    if (!hero || !title || !loop) return;
    var q = 0;
    var shown = 0;
    var hinted = false;
    var fx = {
      on: function () { return fx.near; },
      read: function () {
        var r = hero.getBoundingClientRect();
        q = clamp(-r.top / Math.max(r.height, 1), 0, 1);
      },
      write: function () {
        var v = reduce.matches ? 0 : Math.round(q * 1000) / 1000;
        var hint = fx.near && !reduce.matches;
        if (hint !== hinted) { hinted = hint; title.style.willChange = hint ? 'transform, opacity' : ''; }
        if (v === shown) return;
        shown = v;
        if (v === 0) {
          title.style.transform = '';
          title.style.opacity = '';
        } else {
          title.style.transform = 'scale(' + (1 - 0.08 * v).toFixed(4) + ')';
          title.style.opacity = (1 - 0.8 * v).toFixed(3);
        }
      }
    };
    loop.gate(hero, fx, '100px 0px');
    loop.add(fx);
  }

  /* ---------- Susun: the sticky scrub ----------
     The wrapper (.ar-scrub) is 130vh taller than the viewport (70vh on
     phones) while .is-scrub is set, and the stage inside it sticks to the
     top. p = clamp(-top / (height - innerHeight), 0, 1) of the wrapper;
     below 0.25 tile 1 is current, then each quarter fills one more tile, and
     from 0.75 all three are filled and the notice shows. Scrolling back runs
     it backwards. .is-scrub is set only with motion welcome and a stage that
     fits the viewport; otherwise the drawing keeps its default, the final
     arranged state. While the stage is pinned (top <= 0 and bottom >= the
     viewport) the notch shows the step; otherwise it follows the sections. */
  function initSusun() {
    var sec = $('.susun');
    var outer = sec && $('.ar-scrub', sec);
    var stage = sec && $('.ar-stage', sec);
    var inner = sec && $('.ar-grid', sec);
    var demo = $('#ar-demo');
    var nEl = $('#ar-n');
    if (!sec || !outer || !stage || !inner || !demo || !loop) return;
    var vw = -1;
    var vh = -1;
    var fitsView = false;
    var enabled = false;
    var step = -1;
    var top = 0;
    var bottom = 0;
    var height = 0;
    function notchStep(s) { if (notchApi && notchApi.arrange) notchApi.arrange(s); }
    var fx = {
      on: function () { return fx.near; },
      read: function () {
        if (window.innerWidth !== vw || window.innerHeight !== vh) {
          vw = window.innerWidth;
          vh = window.innerHeight;
          var cs = getComputedStyle(stage);
          fitsView = inner.offsetHeight + parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom) <= vh;
        }
        var r = outer.getBoundingClientRect();
        top = r.top;
        bottom = r.bottom;
        height = r.height;
      },
      write: function () {
        var want = fitsView && !reduce.matches;
        if (want !== enabled) {
          enabled = want;
          sec.classList.toggle('is-scrub', want);
          /* The wrapper's height just changed. When it lies above the view (a
             load at #cat or #install, a restored scroll position), keep the
             view where it was: one measurement, only on this switch. */
          if (bottom <= 0) {
            var grown = outer.getBoundingClientRect().height - height;
            if (grown) window.scrollBy({ top: grown, left: 0, behavior: 'instant' });
          }
          loop.kick(fx);
          return;
        }
        if (!enabled) {
          if (step !== -1) { step = -1; demo.removeAttribute('data-step'); }
          notchStep(-1);
          return;
        }
        var p = clamp(-top / Math.max(height - vh, 1), 0, 1);
        var s = p < 0.25 ? 0 : p < 0.5 ? 1 : p < 0.75 ? 2 : 3;
        if (s !== step) {
          step = s;
          demo.setAttribute('data-step', String(s));
          if (nEl && s < 3) nEl.textContent = String(s + 1);
        }
        var pinned = fx.near && top <= 0.5 && bottom >= vh - 0.5;
        notchStep(pinned ? s : -1);
      }
    };
    loop.gate(outer, fx, '100px 0px');
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
  safe(initTheme);
  safe(initMenu);
  safe(initCopy);
  var catApi = safe(initCats);
  loop = safe(makeLoop);
  safe(function () { initNotch(catApi); });
  safe(initHero);
  safe(initSusun);
  safe(initReveal);
  /* The head script removes .js at DOMContentLoaded unless this is set (script blocked or broken). */
  root.classList.add('ready');
})();
