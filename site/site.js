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
    var dark = window.matchMedia('(prefers-color-scheme: dark)');
    function current() {
      var t = root.getAttribute('data-theme');
      if (t === 'light' || t === 'dark') return t;
      return dark.matches ? 'dark' : 'light';
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
    if (dark.addEventListener) dark.addEventListener('change', paint);
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

  function accessory(name, black) {
    var frame = black ? '#F4EDE4' : '#17120F';
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
    if (look.accessory !== 'glasses') s += accessory(look.accessory, black);
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

    $$('input[name="coat"], input[name="accessory"]').forEach(function (input) {
      input.addEventListener('change', function () {
        if (!input.checked) return;
        look[input.name] = input.value;
        restyle();
      });
    });

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
    }

    return {
      entered: function () { if (lastX !== null) pointCats(lastX); }
    };
  }

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
        sectionState = chosen.getAttribute('data-notch') || 'idle';
        if (!copyTimer) setState(sectionState);
      }, { rootMargin: '-40% 0px -55% 0px' });
      sections.forEach(function (s) { io.observe(s); });
    }

    /* ----- lyric line follows the Music section's scroll progress ----- */
    var lyricEl = $('#notch-lyric');
    var musicSection = $('.f-music');
    var lyricIndex = 0;
    var lyricTick = false;
    function setLyric(i) {
      if (!lyricEl || i === lyricIndex) return;
      lyricIndex = i;
      lyricEl.textContent = LYRICS[i];
      if (!reduce.matches) {
        lyricEl.classList.remove('swap');
        void lyricEl.offsetWidth;
        lyricEl.classList.add('swap');
      }
    }
    function lyricUpdate() {
      lyricTick = false;
      var r = musicSection.getBoundingClientRect();
      var p = clamp((window.innerHeight * 0.5 - r.top) / Math.max(r.height, 1), 0, 0.999);
      setLyric(Math.floor(p * LYRICS.length));
    }
    function lyricScroll() {
      if (lyricTick) return;
      lyricTick = true;
      requestAnimationFrame(lyricUpdate);
    }
    if (lyricEl && musicSection && 'IntersectionObserver' in window) {
      new IntersectionObserver(function (entries) {
        if (entries[entries.length - 1].isIntersecting) {
          window.addEventListener('scroll', lyricScroll, { passive: true });
          lyricScroll();
        } else {
          window.removeEventListener('scroll', lyricScroll);
        }
      }).observe(musicSection);
    }

    /* ----- copy notice ----- */
    notchApi = {
      showCopy: function (text) {
        if (open) return;
        var line = text.length > 90 ? text.slice(0, 90) : text;
        var preview = $('#notch-clip-preview');
        if (preview) preview.textContent = line;
        clearTimeout(copyTimer);
        setState('clipboard');
        copyTimer = setTimeout(function () {
          copyTimer = 0;
          setState(sectionState);
        }, 3000);
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

    /* ----- open and close ----- */
    function setFaceLabel() {
      face.setAttribute('aria-expanded', open ? 'true' : 'false');
      face.setAttribute('aria-label', open ? 'Vale island demo. Close the panel' : 'Vale island demo. Open the panel');
    }
    function openPanel(pin) {
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
      clearTimeout(closeTimer);
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
    }

    notch.addEventListener('pointerenter', function (e) {
      if (e.pointerType === 'touch') return;
      openPanel(false);
    });
    notch.addEventListener('pointerleave', function (e) {
      if (e.pointerType === 'touch' || pinned) return;
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
      if (byKeyboard) tabs[active].focus();
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

  /* ---------- reveal fallback: add a class once, then stop watching ---------- */
  function initReveal() {
    var items = $$('.reveal');
    if (!items.length) return;
    var native = window.CSS && CSS.supports && CSS.supports('animation-timeline: view()');
    if (native || reduce.matches || !('IntersectionObserver' in window)) {
      items.forEach(function (el) { el.classList.add('is-in'); });
      return;
    }
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting) return;
        e.target.classList.add('is-in');
        io.unobserve(e.target);
      });
    }, { rootMargin: '0px 0px -10% 0px' });
    items.forEach(function (el) { io.observe(el); });
  }

  initTheme();
  initMenu();
  initCopy();
  var catApi = initCats();
  initNotch(catApi);
  initReveal();
})();
