"""Checks the static site in site/ (run from anywhere: python -I scripts/check-site.py).

Standard library only. Exit code 1 when any problem is found. For every
site/*.html it checks:
  - the file parses, its tags nest correctly, and attributes are separated by
    whitespace (html.parser accepts id="a"viewBox="b"; the HTML spec does not);
  - every id is unique;
  - every internal href, src and CSS url() points at a file that exists inside
    site/ with the exact same case (GitHub Pages is case-sensitive), and
    every #fragment exists in the file it points at (index.html#features, the
    legal table-of-contents anchors, the SVG sprite ids used by <use>);
  - the shared contract in docs/memory/site-landing.md: the same inline head
    script, style.css and site.js, and the same header and footer on every page
    (apart from the index.html# prefix and aria-current); the header links
    Features and Install to (index.html)#features and #install, GitHub and the
    Download (releases/latest/download/vale.exe); the footer links privacy.html
    and terms.html;
  - every page has the go-to-top target id "top" and an a.to-top[href="#top"]
    with an aria-label, the same markup on every page;
  - every <link rel=preload as=font> points at a .woff2 that an @font-face
    in style.css uses;
  - every table sits in div.table-wrap[role=region][tabindex=0] with a name;
  - .reveal sits only on h2, h3, figure and article.play-card;
  - the notch dock on index.html has the 14 tabs of Page::ALL, in order, each
    controlling an existing panel;
  - the story on index.html (ol.story-list) lists the same 14 pages in the
    same order (li[data-page]), the playground has exactly six .play-card, and
    the Google Calendar and privacy sections each hold a <details>;
  - no third-party resource is loaded (external http(s) links are listed, not
    fetched).
It also checks that no text file under site/ contains an em dash (U+2014).
"""
import os
import posixpath
import re
import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

SITE = Path(__file__).resolve().parent.parent / "site"
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"}
TEXT_SUFFIXES = {".html", ".css", ".js", ".md", ".svg", ".json", ".xml", ".webmanifest"}
EM_DASH = "—"
DOCK_TABS = ["tab-" + n for n in ("calendar", "pomodoro", "reminders", "notes", "music", "mixer", "clipboard",
                                  "notifications", "shelf", "system", "devices", "cli", "arrange", "shortcuts")]
STORY_PAGES = [t[len("tab-"):] for t in DOCK_TABS]  # the story's data-page ids are the dock's page ids
PLAY_CARDS = 6
problems = []
externals = []


def problem(where, msg):
    problems.append(f"{where}: {msg}")


class Page(HTMLParser):
    def __init__(self, name):
        super().__init__(convert_charrefs=True)
        self.name = name
        self.ids = []
        self.refs = []  # (tag, attr, value, line)
        self.stack = []
        self.table_ok = []
        self.scripts = []
        self.in_script = False
        self.reveal = []
        self.tabs = []  # (id, aria-controls) of role=tab buttons, in order
        self.to_top = []  # attrs of every a.to-top
        self.preloads = []  # (href, line) of every <link rel=preload as=font>
        self.story_pages = []  # data-page of every li inside ol.story-list, in order
        self.play_cards = 0  # .play-card elements inside #playground
        self.details = []  # id of the nearest section around every <details> (None when there is none)

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        line = self.getpos()[0]
        raw = self.get_starttag_text()
        if raw and not attrs_separated(raw):
            problem(self.name, f"line {line}: <{tag}> has no whitespace between two attributes")
        if "id" in a:
            self.ids.append(a["id"])
        for attr in ("href", "src"):
            if a.get(attr) is not None:
                self.refs.append((tag, attr, a[attr], line))
        if tag == "img" and "alt" not in a:
            problem(self.name, f"line {line}: img without alt")
        classes = (a.get("class") or "").split()
        if "reveal" in classes and not (tag in ("h2", "h3", "figure") or (tag == "article" and "play-card" in classes)):
            problem(self.name, f"line {line}: .reveal on <{tag}> (only headings, figures and article.play-card)")
        if tag == "li" and "data-page" in a and any(
                t == "ol" and "story-list" in (at.get("class") or "").split() for t, at, _ in self.stack):
            self.story_pages.append(a["data-page"])
        if "play-card" in classes and any(at.get("id") == "playground" for _, at, _ in self.stack):
            self.play_cards += 1
        if tag == "details":
            self.details.append(next((at.get("id") for t, at, _ in reversed(self.stack) if t == "section" and at.get("id")), None))
        if tag == "a" and "to-top" in classes:
            self.to_top.append(a)
        if tag == "link" and "preload" in (a.get("rel") or "").lower().split() and (a.get("as") or "").lower() == "font":
            self.preloads.append((a.get("href") or "", line))
        if a.get("role") == "tab":
            self.tabs.append((a.get("id"), a.get("aria-controls")))
        if tag == "table":
            wrap = next((e for e in reversed(self.stack) if e[0] == "div"), None)
            ok = bool(wrap) and "table-wrap" in (wrap[1].get("class") or "").split() \
                and wrap[1].get("role") == "region" and wrap[1].get("tabindex") == "0" \
                and bool(wrap[1].get("aria-label") or wrap[1].get("aria-labelledby"))
            if not ok:
                problem(self.name, f"line {line}: table is not inside div.table-wrap[role=region][tabindex=0] with a name")
        if tag == "script":
            self.in_script = True
            self.scripts.append({"src": a.get("src"), "text": ""})
        if tag not in VOID:
            self.stack.append((tag, a, line))

    def handle_startendtag(self, tag, attrs):
        # <svg ... /> style self-closing tags: treat as start and end.
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.stack.pop()

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        line = self.getpos()[0]
        if not self.stack:
            problem(self.name, f"line {line}: stray </{tag}>")
            return
        top = self.stack[-1]
        if top[0] == tag:
            self.stack.pop()
            if tag == "script":
                self.in_script = False
            return
        problem(self.name, f"line {line}: </{tag}> closes <{top[0]}> opened on line {top[2]}")
        for i in range(len(self.stack) - 1, -1, -1):
            if self.stack[i][0] == tag:
                del self.stack[i:]
                break

    def handle_data(self, data):
        if self.in_script and self.scripts:
            self.scripts[-1]["text"] += data

    def finish(self):
        for tag, a, line in self.stack:
            problem(self.name, f"line {line}: <{tag}> is never closed")


def attrs_separated(raw):
    """False when two attributes in the raw start tag touch, like
    id="a"viewBox="b". Browsers and html.parser both recover from that, but it
    is a parse error (missing-whitespace-between-attributes)."""
    i = 1
    n = len(raw)
    while i < n and not (raw[i].isspace() or raw[i] in "/>"):
        i += 1  # the tag name
    while True:
        start = i
        while i < n and (raw[i].isspace() or raw[i] == "/"):
            i += 1
        if i >= n or raw[i] == ">":
            return True
        if i == start:
            return False
        while i < n and not (raw[i].isspace() or raw[i] in "=/>"):
            i += 1  # the attribute name
        j = i
        while j < n and raw[j].isspace():
            j += 1
        if j < n and raw[j] == "=":
            j += 1
            while j < n and raw[j].isspace():
                j += 1
            if j < n and raw[j] in "\"'":
                close = raw.find(raw[j], j + 1)
                i = n if close < 0 else close + 1
            else:
                while j < n and not (raw[j].isspace() or raw[j] == ">"):
                    j += 1
                i = j


def split_ref(value):
    parts = urlsplit(value.strip())
    return parts, unquote(parts.path), parts.fragment


def is_external(value):
    v = value.strip().lower()
    return v.startswith(("http://", "https://", "//"))


def is_other_scheme(value):
    return value.strip().lower().startswith(("mailto:", "tel:", "data:", "javascript:"))


def site_file(rel):
    """Resolves a reference made from a top-level file in site/ the way GitHub
    Pages serves it. Returns (path relative to site/ in posix form, None), or
    (None, reason). Pages only deploys site/ and its paths are case-sensitive,
    so the target must stay inside site/ and every segment must match the real
    directory listing exactly (Windows and macOS file systems ignore case)."""
    outside = None, "points outside site/ (not deployed to GitHub Pages)"
    # URL paths resolve lexically, so normalise the string itself. Path.resolve()
    # would also rewrite each segment to its on-disk case on Windows, which
    # hides exactly the mismatch checked below.
    norm = posixpath.normpath(rel)
    if rel.endswith("/"):
        norm = posixpath.join(norm, "index.html")
    if norm.startswith("/") or norm == ".." or norm.startswith("../"):
        return outside
    root = SITE.resolve()
    if not (SITE / norm).resolve().is_relative_to(root):  # e.g. a symlink out of site/
        return outside
    parts = [p for p in norm.split("/") if p not in ("", ".")]
    parent = root
    for part in parts:
        try:
            entries = os.listdir(parent)
        except OSError:
            return None, "does not exist under site/"
        if part not in entries:
            if any(e.lower() == part.lower() for e in entries):
                return None, "differs in case from the file under site/ (GitHub Pages paths are case-sensitive)"
            return None, "does not exist under site/"
        parent = parent / part
    if not parent.is_file():
        return None, "does not exist under site/"
    return "/".join(parts), None


def region(text, start_marker, end_marker):
    i = text.find(start_marker)
    if i < 0:
        return None
    j = text.find(end_marker, i)
    return text[i:j + len(end_marker)] if j >= 0 else None


def normalise_shell(chunk):
    chunk = chunk.replace(' aria-current="page"', "")
    return chunk.replace('href="index.html#', 'href="#')


def main():
    if not SITE.is_dir():
        print(f"site folder not found: {SITE}")
        return 1
    pages = {}
    texts = {}
    for path in sorted(SITE.glob("*.html")):
        text = path.read_text(encoding="utf-8")
        texts[path.name] = text
        page = Page(path.name)
        try:
            page.feed(text)
            page.close()
        except Exception as exc:  # html.parser is lenient; anything raised is a real problem
            problem(path.name, f"does not parse: {exc}")
        page.finish()
        pages[path.name] = page
    if not pages:
        problem("site", "no html files found")

    for name, page in pages.items():
        seen = set()
        for i in page.ids:
            if i in seen:
                problem(name, f"duplicate id {i!r}")
            seen.add(i)

    def check_ref(name, where, value):
        if not value.strip():
            problem(name, f"{where}: empty reference")
            return
        if is_external(value):
            externals.append((name, value))
            return
        if is_other_scheme(value):
            return
        parts, rel, frag = split_ref(value)
        if parts.scheme or parts.netloc:
            problem(name, f"{where}: unsupported reference {value!r}")
            return
        target_name = name
        if rel:
            target_name, why = site_file(rel)
            if target_name is None:
                problem(name, f"{where}: {value!r} {why}")
                return
        target = SITE / target_name
        if frag:
            if target_name in pages:
                if frag not in pages[target_name].ids:
                    problem(name, f"{where}: {value!r} points at #{frag}, which is not an id in {target_name}")
            elif rel and target.suffix == ".html":
                problem(name, f"{where}: {value!r} points into a page that was not parsed")

    for name, page in pages.items():
        for tag, attr, value, line in page.refs:
            where = f"line {line} <{tag} {attr}>"
            check_ref(name, where, value)
            if is_external(value) and (tag in ("script", "img", "source") or (tag == "link" and attr == "href")):
                problem(name, f"{where}: loads a third-party resource {value!r}")

    for css in sorted(SITE.glob("*.css")):
        for m in re.finditer(r"url\(\s*(['\"]?)([^)'\"]+)\1\s*\)", css.read_text(encoding="utf-8")):
            value = m.group(2)
            if value.startswith("data:"):
                continue
            check_ref(css.name, "css url()", value)

    # The shared contract between the pages.
    head_scripts = {}
    for name, page in pages.items():
        inline = [s["text"].strip() for s in page.scripts if not s["src"]]
        head_scripts[name] = inline[0] if inline else ""
        srcs = [s["src"] for s in page.scripts if s["src"]]
        if "site.js" not in srcs:
            problem(name, "does not load site.js")
        links = [v for tag, attr, v, _ in page.refs if tag == "link" and attr == "href"]
        if "style.css" not in links:
            problem(name, "does not load style.css")
    if len(set(head_scripts.values())) > 1:
        problem("site", "the inline head script differs between pages: " + ", ".join(sorted(head_scripts)))
    if head_scripts and not next(iter(head_scripts.values())):
        problem("site", "no inline head script that adds the js class")

    shells = {}
    for name, text in texts.items():
        header = region(text, '<header class="site-header">', "</header>")
        footer = region(text, '<footer class="site-footer">', "</footer>")
        if header is None:
            problem(name, "no header.site-header")
        if footer is None:
            problem(name, "no footer.site-footer")
        shells[name] = (normalise_shell(header or ""), normalise_shell(footer or ""))
        if header:
            for label, frag in (("Features", "index.html#features"), ("Install", "index.html#install")):
                if f'href="{frag}">{label}<' not in header and f'href="#{frag.split("#")[1]}">{label}<' not in header:
                    problem(name, f"header link {label!r} is not {frag}")
            if 'href="https://github.com/hartono0297/vale">GitHub<' not in header:
                problem(name, "header has no GitHub link to https://github.com/hartono0297/vale")
            if not re.search(r'href="https://github\.com/hartono0297/vale/releases/latest/download/vale\.exe"[^>]*>Download<', header):
                problem(name, "header has no Download link to releases/latest/download/vale.exe")
        if footer:
            for target in ("privacy.html", "terms.html"):
                if f'href="{target}"' not in footer:
                    problem(name, f"footer does not link {target}")
    if shells:
        ref_name = "index.html" if "index.html" in shells else next(iter(shells))
        for name, (h, f) in shells.items():
            if h != shells[ref_name][0]:
                problem(name, f"header differs from {ref_name}")
            if f != shells[ref_name][1]:
                problem(name, f"footer differs from {ref_name}")

    # Go to top: the #top target and an a.to-top[href="#top"] with a name, the same markup everywhere.
    top_links = {}
    for name, page in pages.items():
        if "top" not in page.ids:
            problem(name, "no element with id 'top' (the go-to-top target)")
        links = [a for a in page.to_top if a.get("href") == "#top" and (a.get("aria-label") or "").strip()]
        if not links:
            problem(name, 'no a.to-top[href="#top"] with an aria-label')
        top_links[name] = region(texts[name], '<a class="to-top"', "</a>") or ""
    if len(set(top_links.values())) > 1:
        problem("site", "the a.to-top markup differs between pages: " + ", ".join(sorted(top_links)))

    # Font preloads must point at a .woff2 that style.css actually uses in an @font-face.
    css_text = (SITE / "style.css").read_text(encoding="utf-8") if (SITE / "style.css").is_file() else ""
    face_urls = set()
    for block in re.findall(r"@font-face\s*\{[^}]*\}", css_text):
        for m in re.finditer(r"url\(\s*(['\"]?)([^)'\"]+)\1\s*\)", block):
            face_urls.add(posixpath.normpath(m.group(2)))
    for name, page in pages.items():
        for href, line in page.preloads:
            norm = posixpath.normpath(split_ref(href)[1]) if href else ""
            if not norm.lower().endswith(".woff2"):
                problem(name, f"line {line}: font preload {href!r} is not a .woff2")
            elif norm not in face_urls:
                problem(name, f"line {line}: font preload {href!r} is not used by an @font-face in style.css")

    # The notch dock on index.html: the 14 pages of Page::ALL (src/model.rs), in order.
    if "index.html" in pages:
        page = pages["index.html"]
        got = [t for t, _ in page.tabs]
        if got != DOCK_TABS:
            problem("index.html", f"dock tabs are {got}, expected {DOCK_TABS} (Page::ALL order)")
        for tab, panel in page.tabs:
            if not panel or panel not in page.ids:
                problem("index.html", f"tab {tab!r} controls {panel!r}, which is not an id")

    # The story lists the 14 pages of Page::ALL in order, the playground has six cards,
    # and the Calendar and privacy sections each hold a <details>.
    if "index.html" in pages:
        page = pages["index.html"]
        if page.story_pages != STORY_PAGES:
            problem("index.html", f"the story lists {page.story_pages}, expected {STORY_PAGES} (Page::ALL order, ol.story-list li[data-page])")
        if page.play_cards != PLAY_CARDS:
            problem("index.html", f"#playground has {page.play_cards} .play-card, expected {PLAY_CARDS}")
        for section in ("google-calendar", "privacy"):
            if section not in page.details:
                problem("index.html", f"#{section} has no <details>")

    # No em dash in any text file under site/.
    for path in sorted(SITE.rglob("*")):
        if path.is_file() and path.suffix.lower() in TEXT_SUFFIXES:
            body = path.read_text(encoding="utf-8", errors="replace")
            for n, line in enumerate(body.splitlines(), 1):
                if EM_DASH in line:
                    problem(str(path.relative_to(SITE)), f"line {n}: contains an em dash (U+2014)")

    if externals:
        print("External links (listed, not fetched):")
        for name, value in sorted(set(externals)):
            print(f"  {name}: {value}")
    for p in problems:
        print("PROBLEM", p)
    print(f"{len(pages)} pages checked, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
