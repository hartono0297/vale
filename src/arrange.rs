//! Susun (Arrange): the pure model. A layout is a binary tree of splits
//! (`Node`); its leaves, numbered depth first, are the tiles the user fills
//! one by one. Everything here is plain data and arithmetic with no Win32, so
//! it is fully tested: the built-in layouts, the split / merge / divider
//! rules of the editor, the gap maths, the config types (read leniently, one
//! broken entry never throws the rest away), the matching of saved windows to
//! open ones, the monitor fallback and the pick flow.
//!
//! "Tile" is avoided in names on purpose: `tiles.rs` / `ShortcutTiles` are
//! the Pintasan tiles. The windows go into "slots" of an arrangement
//! (`Node` leaves, `PickFlow`), and the code says `arrange`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

pub use crate::capture::Rect;

/// At most this many leaves in one layout.
pub const MAX_LEAVES: usize = 6;
/// Every leaf stays at least this fraction of the monitor wide and high.
pub const MIN_FRAC: f32 = 0.15;
/// A dragged divider snaps to multiples of this fraction of the monitor.
pub const SNAP: f32 = 0.05;
/// Custom templates and saved arrangements kept in the config.
pub const MAX_TEMPLATES: usize = 12;
pub const MAX_SAVED: usize = 12;
/// A template's or a saved arrangement's name, in characters.
pub const NAME_MAX: usize = 24;
/// The gap between tiles, in logical px (Settings), and its default.
pub const GAP_MAX: u32 = 24;
pub const GAP_DEFAULT: u32 = 8;

/// Tolerance for float comparisons of fractions.
const EPS: f32 = 1e-4;

/// Which way a split divides its region: `Cols` puts the halves side by side
/// (a vertical divider), `Rows` stacks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dir {
    Cols,
    Rows,
}

/// A rectangle in fractions (0..1) of the monitor's work area.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl NRect {
    pub const FULL: NRect = NRect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Start, size and end along `dir`'s axis.
    fn span(&self, dir: Dir) -> (f32, f32) {
        match dir {
            Dir::Cols => (self.x, self.w),
            Dir::Rows => (self.y, self.h),
        }
    }

    /// The part before and after a divider at absolute `pos` along `dir`.
    fn cut(&self, dir: Dir, pos: f32) -> (NRect, NRect) {
        match dir {
            Dir::Cols => (
                NRect {
                    w: pos - self.x,
                    ..*self
                },
                NRect {
                    x: pos,
                    w: self.right() - pos,
                    ..*self
                },
            ),
            Dir::Rows => (
                NRect {
                    h: pos - self.y,
                    ..*self
                },
                NRect {
                    y: pos,
                    h: self.bottom() - pos,
                    ..*self
                },
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Leaf,
    Split {
        dir: Dir,
        /// Where the divider sits inside this node's region, 0..1: the share
        /// of the first half `a`.
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

/// One divider of a layout, for the editor: where it is and how to reach it.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    /// The way down from the root: `false` = `a`, `true` = `b`.
    pub path: Vec<bool>,
    pub dir: Dir,
    /// The region of the split this divider belongs to.
    pub region: NRect,
    /// The divider's absolute position along `dir`'s axis (fraction of the
    /// monitor).
    pub pos: f32,
}

fn split(dir: Dir, ratio: f32, a: Node, b: Node) -> Node {
    Node::Split {
        dir,
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

impl Node {
    /// How many tiles the layout has.
    pub fn leaves(&self) -> usize {
        match self {
            Node::Leaf => 1,
            Node::Split { a, b, .. } => a.leaves() + b.leaves(),
        }
    }

    /// The tiles' rectangles in order (depth first, `a` before `b`).
    pub fn to_rects(&self) -> Vec<NRect> {
        let mut out = Vec::new();
        self.walk(NRect::FULL, &mut out);
        out
    }

    fn walk(&self, region: NRect, out: &mut Vec<NRect>) {
        match self {
            Node::Leaf => out.push(region),
            Node::Split { dir, ratio, a, b } => {
                let (start, size) = region.span(*dir);
                let (ra, rb) = region.cut(*dir, start + size * ratio);
                a.walk(ra, out);
                b.walk(rb, out);
            }
        }
    }

    /// Every divider, depth first (a split before the splits inside it).
    pub fn dividers(&self) -> Vec<Divider> {
        let mut out = Vec::new();
        self.walk_dividers(NRect::FULL, &mut Vec::new(), &mut out);
        out
    }

    fn walk_dividers(&self, region: NRect, path: &mut Vec<bool>, out: &mut Vec<Divider>) {
        if let Node::Split { dir, ratio, a, b } = self {
            let (start, size) = region.span(*dir);
            let pos = start + size * ratio;
            out.push(Divider {
                path: path.clone(),
                dir: *dir,
                region,
                pos,
            });
            let (ra, rb) = region.cut(*dir, pos);
            path.push(false);
            a.walk_dividers(ra, path, out);
            path.pop();
            path.push(true);
            b.walk_dividers(rb, path, out);
            path.pop();
        }
    }

    /// The smallest span this subtree needs along `dir` (a fraction of the
    /// monitor) for every tile to keep `MIN_FRAC`: side by side tiles add up,
    /// stacked ones each need the whole span.
    fn min_extent(&self, along: Dir) -> f32 {
        match self {
            Node::Leaf => MIN_FRAC,
            Node::Split { dir, a, b, .. } if *dir == along => {
                a.min_extent(along) + b.min_extent(along)
            }
            Node::Split { a, b, .. } => a.min_extent(along).max(b.min_extent(along)),
        }
    }

    /// Splits tile `leaf` in two halves, 50/50. Refused (false) with six
    /// tiles already, when `leaf` does not exist, or when a half would be
    /// under `MIN_FRAC` of the monitor's width (`Cols`) or height (`Rows`).
    pub fn split(&mut self, leaf: usize, dir: Dir) -> bool {
        if self.leaves() >= MAX_LEAVES {
            return false;
        }
        let Some(rect) = self.to_rects().get(leaf).copied() else {
            return false;
        };
        let size = match dir {
            Dir::Cols => rect.w,
            Dir::Rows => rect.h,
        };
        if size / 2.0 < MIN_FRAC - EPS {
            return false;
        }
        let mut index = 0;
        match self.leaf_mut(leaf, &mut index) {
            Some(node) => {
                *node = split(dir, 0.5, Node::Leaf, Node::Leaf);
                true
            }
            None => false,
        }
    }

    fn leaf_mut(&mut self, target: usize, seen: &mut usize) -> Option<&mut Node> {
        match self {
            Node::Leaf => {
                let here = *seen;
                *seen += 1;
                (here == target).then_some(self)
            }
            Node::Split { a, b, .. } => match a.leaf_mut(target, seen) {
                Some(found) => Some(found),
                None => b.leaf_mut(target, seen),
            },
        }
    }

    /// Removes tile `leaf`: its sibling (a tile or a whole subtree) takes the
    /// parent's place and grows into the room. Refused (false) with one tile
    /// or for a tile that does not exist.
    pub fn merge(&mut self, leaf: usize) -> bool {
        if self.leaves() < 2 || leaf >= self.leaves() {
            return false;
        }
        self.merge_at(0, leaf)
    }

    fn merge_at(&mut self, base: usize, target: usize) -> bool {
        let Node::Split { a, b, .. } = self else {
            return false;
        };
        let in_a = a.leaves();
        if target < base + in_a {
            if matches!(**a, Node::Leaf) {
                *self = std::mem::replace(&mut **b, Node::Leaf);
                true
            } else {
                a.merge_at(base, target)
            }
        } else if matches!(**b, Node::Leaf) {
            *self = std::mem::replace(&mut **a, Node::Leaf);
            true
        } else {
            b.merge_at(base + in_a, target)
        }
    }

    /// Moves the divider at `path` to where `ratio` of its region puts it:
    /// the absolute position snaps to multiples of `SNAP` of the monitor,
    /// then is clamped so every tile on both sides keeps `MIN_FRAC`. Dividers
    /// inside the two halves keep their absolute places (clamped likewise),
    /// so dragging a big divider never squeezes a small tile under the
    /// minimum. False for a bad path or a non-finite ratio.
    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) -> bool {
        if !ratio.is_finite() {
            return false;
        }
        let dividers = self.dividers();
        let Some(target) = dividers.iter().find(|d| d.path == path) else {
            return false;
        };
        let (start, size) = target.region.span(target.dir);
        let snapped = ((start + size * ratio.clamp(0.0, 1.0)) / SNAP).round() * SNAP;
        let want = Wanted {
            path: path.to_vec(),
            pos: snapped,
            others: dividers,
        };
        self.refit(NRect::FULL, &mut Vec::new(), &want);
        true
    }

    /// Rewrites every ratio from the absolute positions in `want` (the moved
    /// divider's new one, the others' old ones), clamped top-down.
    fn refit(&mut self, region: NRect, path: &mut Vec<bool>, want: &Wanted) {
        let Node::Split { dir, ratio, a, b } = self else {
            return;
        };
        let (start, size) = region.span(*dir);
        let desired = if *path == want.path {
            want.pos
        } else {
            want.others
                .iter()
                .find(|d| d.path == *path)
                .map_or(start + size * *ratio, |d| d.pos)
        };
        let low = start + a.min_extent(*dir);
        let high = start + size - b.min_extent(*dir);
        let pos = if low <= high {
            desired.clamp(low, high)
        } else {
            start + size * 0.5
        };
        *ratio = if size > 0.0 {
            (pos - start) / size
        } else {
            0.5
        };
        let (ra, rb) = region.cut(*dir, pos);
        path.push(false);
        a.refit(ra, path, want);
        path.pop();
        path.push(true);
        b.refit(rb, path, want);
        path.pop();
    }

    /// A tree that came from a file is only used when it is sound: at most
    /// `MAX_LEAVES` tiles, every ratio finite and strictly inside 0..1, and
    /// every tile at least `MIN_FRAC` wide and high.
    pub fn is_valid(&self) -> bool {
        self.leaves() <= MAX_LEAVES
            && self.ratios_ok()
            && self
                .to_rects()
                .iter()
                .all(|r| r.w >= MIN_FRAC - EPS && r.h >= MIN_FRAC - EPS)
    }

    fn ratios_ok(&self) -> bool {
        match self {
            Node::Leaf => true,
            Node::Split { ratio, a, b, .. } => {
                ratio.is_finite() && *ratio > 0.0 && *ratio < 1.0 && a.ratios_ok() && b.ratios_ok()
            }
        }
    }
}

/// What `Node::set_ratio` carries down the tree.
struct Wanted {
    path: Vec<bool>,
    pos: f32,
    others: Vec<Divider>,
}

/// The six built-in layouts. Their ids are what the config stores; they can
/// be neither edited nor deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltIn {
    Cols2,
    Cols3,
    Main2,
    Grid4,
    TwoThirds,
    Main3,
}

impl BuiltIn {
    pub const ALL: [BuiltIn; 6] = [
        BuiltIn::Cols2,
        BuiltIn::Cols3,
        BuiltIn::Main2,
        BuiltIn::Grid4,
        BuiltIn::TwoThirds,
        BuiltIn::Main3,
    ];

    /// The stable id in `config.json`.
    pub fn id(self) -> &'static str {
        match self {
            BuiltIn::Cols2 => "cols2",
            BuiltIn::Cols3 => "cols3",
            BuiltIn::Main2 => "main2",
            BuiltIn::Grid4 => "grid4",
            BuiltIn::TwoThirds => "two_thirds",
            BuiltIn::Main3 => "main3",
        }
    }

    pub fn from_id(id: &str) -> Option<BuiltIn> {
        BuiltIn::ALL.into_iter().find(|b| b.id() == id.trim())
    }

    /// The layout. Thirds are nested splits (1/3, then 1/2 of the rest), so
    /// the one rect function serves every layout. Tile order: left to right,
    /// top to bottom within a column, and the 2x2 grid reads like text.
    pub fn node(self) -> Node {
        const THIRD: f32 = 1.0 / 3.0;
        const TWO_THIRDS: f32 = 2.0 / 3.0;
        let pair = |dir| split(dir, 0.5, Node::Leaf, Node::Leaf);
        match self {
            BuiltIn::Cols2 => pair(Dir::Cols),
            BuiltIn::Cols3 => split(Dir::Cols, THIRD, Node::Leaf, pair(Dir::Cols)),
            BuiltIn::Main2 => split(Dir::Cols, TWO_THIRDS, Node::Leaf, pair(Dir::Rows)),
            BuiltIn::Grid4 => split(Dir::Rows, 0.5, pair(Dir::Cols), pair(Dir::Cols)),
            BuiltIn::TwoThirds => split(Dir::Cols, TWO_THIRDS, Node::Leaf, Node::Leaf),
            BuiltIn::Main3 => split(
                Dir::Cols,
                TWO_THIRDS,
                Node::Leaf,
                split(Dir::Rows, THIRD, Node::Leaf, pair(Dir::Rows)),
            ),
        }
    }
}

/// A layout the user drew in Settings.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CustomTemplate {
    pub id: String,
    pub name: String,
    pub root: Node,
}

/// A name trimmed and kept when it is 1..=`NAME_MAX` characters.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    let n = name.chars().count();
    (1..=NAME_MAX).contains(&n).then(|| name.to_owned())
}

impl CustomTemplate {
    /// The one gate for a new or loaded template: a good name, a sound tree.
    pub fn new(id: &str, name: &str, root: Node) -> Option<CustomTemplate> {
        let id = id.trim();
        (!id.is_empty() && id.chars().count() <= 40 && root.is_valid())
            .then(|| clean_name(name))
            .flatten()
            .map(|name| CustomTemplate {
                id: id.to_owned(),
                name,
                root,
            })
    }
}

/// Which layout a saved arrangement used: a built-in by id, or a full copy
/// of a custom tree (so deleting the template later does not break it).
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutRef {
    BuiltIn(BuiltIn),
    Custom(Node),
}

impl LayoutRef {
    pub fn node(&self) -> Node {
        match self {
            LayoutRef::BuiltIn(b) => b.node(),
            LayoutRef::Custom(n) => n.clone(),
        }
    }
}

impl Serialize for LayoutRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(1))?;
        match self {
            LayoutRef::BuiltIn(b) => map.serialize_entry("builtin", b.id())?,
            LayoutRef::Custom(n) => map.serialize_entry("custom", n)?,
        }
        map.end()
    }
}

/// `{"builtin":"main2"}` or `{"custom":{...tree}}`; `None` for an unknown
/// built-in id or a tree that is not sound.
fn layout_from_value(v: &Value) -> Option<LayoutRef> {
    let map = v.as_object()?;
    if let Some(id) = map.get("builtin") {
        return BuiltIn::from_id(id.as_str()?).map(LayoutRef::BuiltIn);
    }
    let node: Node = serde_json::from_value(map.get("custom")?.clone()).ok()?;
    node.is_valid().then_some(LayoutRef::Custom(node))
}

/// The monitor a saved arrangement was made on.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SavedMonitor {
    /// The GDI device name, like `\\.\DISPLAY1`.
    pub device: String,
    /// Its full rectangle in virtual-screen px when saved.
    pub rect: Rect,
}

/// What a saved slot remembers about the window that was in it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedWindow {
    /// The exe's full path.
    pub exe: String,
    pub class: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SavedArrangement {
    pub name: String,
    pub layout: LayoutRef,
    pub monitor: SavedMonitor,
    /// One entry per tile of the layout; `None` for a tile left empty.
    pub tiles: Vec<Option<SavedWindow>>,
}

impl SavedArrangement {
    /// The one gate for a new or loaded entry: a good name, and as many
    /// slots as the layout has tiles.
    pub fn new(
        name: &str,
        layout: LayoutRef,
        monitor: SavedMonitor,
        tiles: Vec<Option<SavedWindow>>,
    ) -> Option<SavedArrangement> {
        let name = clean_name(name)?;
        (tiles.len() == layout.node().leaves() && !monitor.rect.is_empty()).then_some(
            SavedArrangement {
                name,
                layout,
                monitor,
                tiles,
            },
        )
    }

    /// Names of the apps in the slots (the exe's file name without `.exe`),
    /// in order, each once: the line under a saved arrangement.
    pub fn app_names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in self.tiles.iter().flatten() {
            let name = app_name(&w.exe, &w.class);
            if !out.contains(&name) {
                out.push(name);
            }
        }
        out
    }
}

/// The exe's file name without its extension, else the class.
pub fn app_name(exe: &str, class: &str) -> String {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or("");
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    if stem.is_empty() {
        class.to_owned()
    } else {
        stem.to_owned()
    }
}

fn saved_from_value(v: &Value) -> Option<SavedArrangement> {
    let map = v.as_object()?;
    let layout = layout_from_value(map.get("layout")?)?;
    let mon = map.get("monitor")?.as_object()?;
    let monitor = SavedMonitor {
        device: mon.get("device")?.as_str()?.to_owned(),
        rect: serde_json::from_value(mon.get("rect")?.clone()).ok()?,
    };
    let tiles: Vec<Option<SavedWindow>> = map
        .get("tiles")?
        .as_array()?
        .iter()
        .map(|t| {
            if t.is_null() {
                Some(None)
            } else {
                serde_json::from_value(t.clone()).ok().map(Some)
            }
        })
        .collect::<Option<_>>()?;
    SavedArrangement::new(map.get("name")?.as_str()?, layout, monitor, tiles)
}

/// A gap in px: any number clamped to `0..=GAP_MAX`; a non-number is the
/// default.
pub fn clean_gap(v: &Value) -> u32 {
    v.as_f64().map_or(GAP_DEFAULT, |n| {
        if n.is_finite() {
            n.clamp(0.0, f64::from(GAP_MAX)).round() as u32
        } else {
            GAP_DEFAULT
        }
    })
}

/// The Susun settings, `config.arrange`. Read leniently: a broken entry of
/// either list drops only itself, a non-list is empty, the gap is clamped.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ArrangeSettings {
    #[serde(deserialize_with = "lenient_gap")]
    pub gap: u32,
    #[serde(deserialize_with = "lenient_templates")]
    pub templates: Vec<CustomTemplate>,
    #[serde(deserialize_with = "lenient_saved")]
    pub saved: Vec<SavedArrangement>,
}

impl Default for ArrangeSettings {
    fn default() -> Self {
        ArrangeSettings {
            gap: GAP_DEFAULT,
            templates: Vec::new(),
            saved: Vec::new(),
        }
    }
}

fn lenient_gap<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    Ok(clean_gap(&Value::deserialize(d)?))
}

fn lenient_templates<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<CustomTemplate>, D::Error> {
    let v = Value::deserialize(d)?;
    let mut out: Vec<CustomTemplate> = Vec::new();
    for item in v.as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(map) = item.as_object() else {
            continue;
        };
        let parsed = (|| {
            let root: Node = serde_json::from_value(map.get("root")?.clone()).ok()?;
            CustomTemplate::new(map.get("id")?.as_str()?, map.get("name")?.as_str()?, root)
        })();
        if let Some(t) = parsed
            && !out.iter().any(|o| o.id == t.id)
            && out.len() < MAX_TEMPLATES
        {
            out.push(t);
        }
    }
    Ok(out)
}

fn lenient_saved<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<SavedArrangement>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(saved_from_value)
        .take(MAX_SAVED)
        .collect())
}

/// `config.arrange` read leniently: not an object is the default, a bad
/// field is that field's default.
pub fn lenient_settings<'de, D: Deserializer<'de>>(d: D) -> Result<ArrangeSettings, D::Error> {
    let v = Value::deserialize(d)?;
    if !v.is_object() {
        return Ok(ArrangeSettings::default());
    }
    Ok(ArrangeSettings::deserialize(v).unwrap_or_default())
}

/// A fresh template id that no template in `taken` has.
pub fn new_template_id(taken: &[CustomTemplate]) -> String {
    (1u32..)
        .map(|n| format!("t{n}"))
        .find(|id| !taken.iter().any(|t| &t.id == id))
        .expect("an unused id")
}

/// The default name for a new save: "Susunan n" with the lowest `n` that no
/// saved arrangement uses (`prefix` is the language's word).
pub fn default_save_name(prefix: &str, saved: &[SavedArrangement]) -> String {
    (1u32..)
        .map(|n| format!("{prefix} {n}"))
        .find(|name| !saved.iter().any(|s| &s.name == name))
        .expect("an unused name")
}

// ---------------------------------------------------------------- the gaps

/// The gap in physical px for a gap of `logical` px on a monitor of `dpi`.
pub fn scaled_gap(logical: u32, dpi: u32) -> i32 {
    let gap = logical.min(GAP_MAX) as f64 * f64::from(dpi.max(96)) / 96.0;
    gap.round() as i32
}

/// The tiles' rectangles in physical px inside `work`. An edge on the work
/// area's edge is inset by `gap`, an inner edge by half of it (the left or
/// top side rounds down, the right or bottom side up, so neighbours are
/// exactly `gap` apart and no pixel is lost). The edges come from the
/// fractions once per position, so tiles that share a divider share the
/// pixel.
pub fn tile_rects(nrects: &[NRect], work: Rect, gap: i32) -> Vec<Rect> {
    let gap = gap.max(0);
    let (lo, hi) = (gap / 2, gap - gap / 2);
    let edge = |f: f32, origin: i32, size: i32| -> i32 {
        origin
            + (f64::from(f) * f64::from(size))
                .round()
                .clamp(0.0, f64::from(size)) as i32
    };
    nrects
        .iter()
        .map(|n| {
            let x0 = edge(n.x, work.x, work.w);
            let x1 = edge(n.right(), work.x, work.w);
            let y0 = edge(n.y, work.y, work.h);
            let y1 = edge(n.bottom(), work.y, work.h);
            let left = if x0 == work.x {
                gap
            } else {
                hi_or(lo, hi, true)
            };
            let right = if x1 == work.right() {
                gap
            } else {
                hi_or(lo, hi, false)
            };
            let top = if y0 == work.y {
                gap
            } else {
                hi_or(lo, hi, true)
            };
            let bottom = if y1 == work.bottom() {
                gap
            } else {
                hi_or(lo, hi, false)
            };
            let (x, y) = (x0 + left, y0 + top);
            Rect::new(x, y, (x1 - right - x).max(1), (y1 - bottom - y).max(1))
        })
        .collect()
}

/// The inner inset of a leading edge (`lo`, rounds down) or a trailing one
/// (`hi`, rounds up).
fn hi_or(lo: i32, hi: i32, leading: bool) -> i32 {
    if leading { lo } else { hi }
}

// ----------------------------------------------------------------- matching

/// A window on screen, as the Win32 side lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenWindow {
    pub hwnd: isize,
    pub title: String,
    pub class: String,
    /// The exe's full path; empty when the process could not be opened.
    pub exe: String,
    pub minimized: bool,
    /// Key of the exe's icon in the icon cache (the exe path, lower case).
    pub icon_key: String,
}

/// A monitor, as the Win32 side lists it (physical px).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Monitor {
    pub device: String,
    pub rect: Rect,
    pub work: Rect,
    pub dpi: u32,
}

/// Titles compared by their letters and digits only, lower case, so a
/// changed counter or punctuation hardly matters. At most 120 characters are
/// looked at.
fn normalise(title: &str) -> Vec<char> {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .take(120)
        .collect()
}

/// How alike two titles are, 0.0 to 1.0: twice the longest common
/// subsequence of the normalised titles over their lengths added (two empty
/// titles are alike, an empty and a full one are not).
pub fn title_similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (normalise(a), normalise(b));
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut prev = vec![0usize; b.len() + 1];
    for &ca in &a {
        let mut cur = vec![0usize; b.len() + 1];
        for (j, &cb) in b.iter().enumerate() {
            cur[j + 1] = if ca == cb {
                prev[j] + 1
            } else {
                prev[j + 1].max(cur[j])
            };
        }
        prev = cur;
    }
    2.0 * prev[b.len()] as f32 / (a.len() + b.len()) as f32
}

/// For each saved slot, the open window to put in it. Greedy in slot order,
/// each open window used at most once. The candidates are the windows of the
/// same exe path (any case) when the slot knows its exe, else those of the
/// same class; among them the same class wins, then the closest title, then
/// the earlier in the list. No candidate: the slot is skipped (`None`).
pub fn match_windows(saved: &[Option<SavedWindow>], open: &[OpenWindow]) -> Vec<Option<usize>> {
    let mut used = vec![false; open.len()];
    saved
        .iter()
        .map(|slot| {
            let slot = slot.as_ref()?;
            let same_class = |w: &OpenWindow| {
                !slot.class.is_empty() && w.class.eq_ignore_ascii_case(&slot.class)
            };
            let best = open
                .iter()
                .enumerate()
                .filter(|(i, w)| {
                    !used[*i]
                        && if slot.exe.is_empty() {
                            same_class(w)
                        } else {
                            w.exe.eq_ignore_ascii_case(&slot.exe)
                        }
                })
                .map(|(i, w)| (i, same_class(w), title_similarity(&slot.title, &w.title)))
                .max_by(|x, y| {
                    x.1.cmp(&y.1)
                        .then(x.2.total_cmp(&y.2))
                        // The earlier window wins a tie.
                        .then(y.0.cmp(&x.0))
                })?;
            used[best.0] = true;
            Some(best.0)
        })
        .collect()
}

/// The monitor a saved arrangement runs on: the one with its device name,
/// else the one with the identical rectangle, else the monitor the notch is
/// on (`notch`, an index; out of range falls to the first). `None` only with
/// no monitors at all.
pub fn resolve_monitor(saved: &SavedMonitor, monitors: &[Monitor], notch: usize) -> Option<usize> {
    monitors
        .iter()
        .position(|m| !saved.device.is_empty() && m.device.eq_ignore_ascii_case(&saved.device))
        .or_else(|| monitors.iter().position(|m| m.rect == saved.rect))
        .or_else(|| (!monitors.is_empty()).then(|| notch.min(monitors.len() - 1)))
}

// ----------------------------------------------------------------- the flow

/// A window as the pick step knows it (its handle).
pub type WinId = isize;

/// The pick step: the tiles are filled one at a time.
#[derive(Clone, Debug, PartialEq)]
pub struct PickFlow {
    tiles: usize,
    current: usize,
    picks: Vec<Option<WinId>>,
}

impl PickFlow {
    pub fn new(tiles: usize) -> PickFlow {
        PickFlow {
            tiles,
            current: 0,
            picks: vec![None; tiles],
        }
    }

    pub fn tiles(&self) -> usize {
        self.tiles
    }

    /// The tile being filled (0-based); equals `tiles()` once done.
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn is_done(&self) -> bool {
        self.current >= self.tiles
    }

    /// Puts `win` in the current tile and moves on. Refused (false) when it
    /// is done or the window is already in another tile.
    pub fn pick(&mut self, win: WinId) -> bool {
        if self.is_done() || self.is_picked(win) {
            return false;
        }
        self.picks[self.current] = Some(win);
        self.current += 1;
        true
    }

    /// "Lewati ubin": the current tile stays empty.
    pub fn skip(&mut self) -> bool {
        if self.is_done() {
            return false;
        }
        self.picks[self.current] = None;
        self.current += 1;
        true
    }

    /// Backspace: back one tile, which frees its window. At the first tile
    /// nothing happens (false).
    pub fn back(&mut self) -> bool {
        if self.current == 0 {
            return false;
        }
        self.current -= 1;
        self.picks[self.current] = None;
        true
    }

    pub fn is_picked(&self, win: WinId) -> bool {
        self.picks.contains(&Some(win))
    }

    /// The window of each tile, `None` for an empty one.
    pub fn picked(&self) -> &[Option<WinId>] {
        &self.picks
    }

    /// The windows still free to pick.
    pub fn available<'a>(&self, list: &'a [OpenWindow]) -> Vec<&'a OpenWindow> {
        list.iter().filter(|w| !self.is_picked(w.hwnd)).collect()
    }
}

// ------------------------------------------------------------------ notices

/// The numbers of the notice after an arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    /// Windows that moved.
    pub moved: usize,
    /// Tiles that got no window: left empty, or the move was skipped.
    pub skipped: usize,
    /// Tiles that were meant to get a window (a saved run only counts the
    /// slots that remember one).
    pub wanted: usize,
}

impl Counts {
    /// After the pick step: `tiles` slots, `moved` windows moved. The
    /// skipped count is every slot that did not get a window.
    pub fn picked(tiles: usize, moved: usize) -> Counts {
        Counts {
            moved,
            skipped: tiles.saturating_sub(moved),
            wanted: tiles,
        }
    }

    /// After a saved run: `wanted` slots remembered a window, `moved` moved.
    pub fn saved_run(wanted: usize, moved: usize) -> Counts {
        Counts {
            moved,
            skipped: wanted.saturating_sub(moved),
            wanted,
        }
    }

    /// The skipped count when it is worth showing (more than none).
    pub fn skipped_shown(&self) -> Option<usize> {
        (self.skipped > 0).then_some(self.skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn rect_is(r: NRect, x: f32, y: f32, w: f32, h: f32) -> bool {
        near(r.x, x) && near(r.y, y) && near(r.w, w) && near(r.h, h)
    }

    fn area(rs: &[NRect]) -> f32 {
        rs.iter().map(|r| r.w * r.h).sum()
    }

    // ---------------------------------------------------------- built-ins

    #[test]
    fn the_six_built_ins_have_stable_ids_and_the_right_tile_counts() {
        let ids: Vec<_> = BuiltIn::ALL.iter().map(|b| b.id()).collect();
        assert_eq!(
            ids,
            ["cols2", "cols3", "main2", "grid4", "two_thirds", "main3"]
        );
        for b in BuiltIn::ALL {
            assert_eq!(BuiltIn::from_id(b.id()), Some(b));
        }
        assert_eq!(BuiltIn::from_id("nope"), None);
        let counts: Vec<_> = BuiltIn::ALL.iter().map(|b| b.node().leaves()).collect();
        assert_eq!(counts, [2, 3, 3, 4, 2, 4]);
    }

    #[test]
    fn built_in_rects_cover_the_monitor_without_overlap() {
        for b in BuiltIn::ALL {
            let node = b.node();
            let rs = node.to_rects();
            assert!(near(area(&rs), 1.0), "{:?} covers the screen", b);
            assert!(node.is_valid(), "{:?} is a sound tree", b);
            for (i, a) in rs.iter().enumerate() {
                for o in &rs[i + 1..] {
                    let w = a.right().min(o.right()) - a.x.max(o.x);
                    let h = a.bottom().min(o.bottom()) - a.y.max(o.y);
                    assert!(w <= 1e-4 || h <= 1e-4, "{:?}: no overlap", b);
                }
            }
        }
    }

    #[test]
    fn built_in_rects_are_where_the_names_say() {
        let r = BuiltIn::Cols2.node().to_rects();
        assert!(rect_is(r[0], 0.0, 0.0, 0.5, 1.0));
        assert!(rect_is(r[1], 0.5, 0.0, 0.5, 1.0));

        let r = BuiltIn::Cols3.node().to_rects();
        let third = 1.0 / 3.0;
        assert!(rect_is(r[0], 0.0, 0.0, third, 1.0));
        assert!(rect_is(r[1], third, 0.0, third, 1.0));
        assert!(rect_is(r[2], 2.0 * third, 0.0, third, 1.0));

        let r = BuiltIn::Main2.node().to_rects();
        assert!(rect_is(r[0], 0.0, 0.0, 2.0 / 3.0, 1.0));
        assert!(rect_is(r[1], 2.0 / 3.0, 0.0, third, 0.5));
        assert!(rect_is(r[2], 2.0 / 3.0, 0.5, third, 0.5));

        let r = BuiltIn::Grid4.node().to_rects();
        assert!(rect_is(r[0], 0.0, 0.0, 0.5, 0.5), "top left first");
        assert!(rect_is(r[1], 0.5, 0.0, 0.5, 0.5));
        assert!(rect_is(r[2], 0.0, 0.5, 0.5, 0.5));
        assert!(rect_is(r[3], 0.5, 0.5, 0.5, 0.5));

        let r = BuiltIn::TwoThirds.node().to_rects();
        assert!(rect_is(r[0], 0.0, 0.0, 2.0 / 3.0, 1.0));
        assert!(rect_is(r[1], 2.0 / 3.0, 0.0, third, 1.0));

        let r = BuiltIn::Main3.node().to_rects();
        assert!(rect_is(r[0], 0.0, 0.0, 2.0 / 3.0, 1.0));
        for (i, row) in r[1..].iter().enumerate() {
            assert!(rect_is(*row, 2.0 / 3.0, i as f32 * third, third, third));
        }
    }

    // ---------------------------------------------------- split and merge

    #[test]
    fn a_new_layout_is_one_tile_and_split_halves_it() {
        let mut n = Node::Leaf;
        assert_eq!(n.leaves(), 1);
        assert!(n.split(0, Dir::Cols));
        let r = n.to_rects();
        assert_eq!(r.len(), 2);
        assert!(rect_is(r[0], 0.0, 0.0, 0.5, 1.0));
        // Splitting the second tile top and bottom keeps the order.
        assert!(n.split(1, Dir::Rows));
        let r = n.to_rects();
        assert_eq!(r.len(), 3);
        assert!(rect_is(r[1], 0.5, 0.0, 0.5, 0.5));
        assert!(rect_is(r[2], 0.5, 0.5, 0.5, 0.5));
        assert!(!n.split(9, Dir::Cols), "no such tile");
    }

    #[test]
    fn at_most_six_tiles() {
        let mut n = Node::Leaf;
        assert!(n.split(0, Dir::Cols));
        assert!(n.split(0, Dir::Rows));
        assert!(n.split(2, Dir::Rows));
        assert!(n.split(3, Dir::Rows));
        assert!(n.split(0, Dir::Rows) || n.leaves() == 5);
        while n.leaves() < MAX_LEAVES {
            let before = n.leaves();
            let did = (0..n.leaves()).any(|i| n.split(i, Dir::Rows) || n.split(i, Dir::Cols));
            assert!(did && n.leaves() == before + 1);
        }
        assert_eq!(n.leaves(), 6);
        for i in 0..6 {
            assert!(!n.split(i, Dir::Cols), "a seventh is refused");
            assert!(!n.split(i, Dir::Rows));
        }
    }

    #[test]
    fn a_half_under_fifteen_percent_is_refused() {
        // Halving 0.5 -> 0.25 -> 0.125: the third column split is refused.
        let mut n = Node::Leaf;
        assert!(n.split(0, Dir::Cols));
        assert!(n.split(0, Dir::Cols)); // 0.25 + 0.25
        assert!(!n.split(0, Dir::Cols), "0.125 is under 15 %");
        assert!(!n.split(1, Dir::Cols));
        // A tile 0.3 wide can still be halved into two 0.15 ones.
        let mut m = Node::Leaf;
        assert!(m.split(0, Dir::Cols));
        assert!(m.set_ratio(&[], 0.3));
        let w = m.to_rects()[0].w;
        assert!(near(w, 0.3), "the divider is at 0.3");
        assert!(m.split(0, Dir::Cols), "exactly 15 % is allowed");
        assert!(m.is_valid());
        // The same rule for rows.
        let mut r = Node::Leaf;
        assert!(r.split(0, Dir::Rows));
        assert!(r.split(0, Dir::Rows));
        assert!(!r.split(0, Dir::Rows));
    }

    #[test]
    fn merge_lets_the_sibling_take_the_parents_place() {
        let mut n = BuiltIn::Main2.node();
        // Removing the first tile: the right column takes the whole width.
        assert!(n.merge(0));
        let r = n.to_rects();
        assert_eq!(r.len(), 2);
        assert!(rect_is(r[0], 0.0, 0.0, 1.0, 0.5));
        assert!(rect_is(r[1], 0.0, 0.5, 1.0, 0.5));
        // Removing one of the two leaves one full tile.
        assert!(n.merge(1));
        assert_eq!(n, Node::Leaf);
        assert!(!n.merge(0), "one tile cannot merge");

        let mut g = BuiltIn::Grid4.node();
        assert!(g.merge(3));
        let r = g.to_rects();
        assert_eq!(r.len(), 3);
        assert!(
            rect_is(r[2], 0.0, 0.5, 1.0, 0.5),
            "the sibling of the removed one grows"
        );
        assert!(!g.merge(7), "no such tile");
    }

    #[test]
    fn merging_a_tile_next_to_a_subtree_keeps_the_subtree() {
        let mut n = BuiltIn::Main3.node();
        assert!(n.merge(0));
        let r = n.to_rects();
        assert_eq!(r.len(), 3);
        for row in &r {
            assert!(near(row.w, 1.0) && near(row.h, 1.0 / 3.0));
        }
    }

    // ------------------------------------------------------------ dividers

    #[test]
    fn a_dragged_divider_snaps_to_five_percent_of_the_monitor() {
        let mut n = BuiltIn::Cols2.node();
        assert!(n.set_ratio(&[], 0.433));
        assert!(near(n.to_rects()[0].w, 0.45));
        assert!(n.set_ratio(&[], 0.4749));
        assert!(near(n.to_rects()[0].w, 0.45));
        assert!(n.set_ratio(&[], 0.476));
        assert!(near(n.to_rects()[0].w, 0.5));
    }

    #[test]
    fn the_snap_is_of_the_absolute_position_not_of_the_inner_ratio() {
        // A column 0.5 wide starting at 0.5: its inner divider snaps on the
        // monitor's grid (0.5 + 0.5 * r), not on the grid of its own width.
        let mut n = Node::Leaf;
        n.split(0, Dir::Cols);
        n.split(1, Dir::Cols);
        let path = [true];
        assert!(n.set_ratio(&path, 0.5 + 0.02));
        let d = n.dividers().into_iter().find(|d| d.path == path).unwrap();
        assert!(near(d.pos, 0.75), "0.76 snaps to 0.75: {}", d.pos);
        assert!(n.set_ratio(&path, 0.59));
        let d = n.dividers().into_iter().find(|d| d.path == path).unwrap();
        assert!(near(d.pos, 0.8), "0.795 snaps to 0.8: {}", d.pos);
    }

    #[test]
    fn a_divider_stops_where_a_tile_would_get_under_fifteen_percent() {
        let mut n = BuiltIn::Cols2.node();
        assert!(n.set_ratio(&[], 0.0));
        assert!(near(n.to_rects()[0].w, 0.15));
        assert!(n.set_ratio(&[], 1.0));
        assert!(near(n.to_rects()[1].w, 0.15));
        assert!(n.is_valid());
        assert!(!n.set_ratio(&[], f32::NAN));
        assert!(!n.set_ratio(&[true, false], 0.5), "no such divider");
    }

    #[test]
    fn a_big_divider_respects_the_tiles_inside_the_half() {
        // [A | [B | C]] with B and C 0.25 wide: the outer divider cannot go
        // past 0.5 + ... : the right half needs 0.3 for its two tiles.
        let mut n = Node::Leaf;
        n.split(0, Dir::Cols);
        n.split(1, Dir::Cols);
        assert!(n.set_ratio(&[], 1.0));
        let r = n.to_rects();
        assert!(r[1].w >= 0.15 - 1e-4 && r[2].w >= 0.15 - 1e-4);
        assert!(
            near(r[0].w, 0.7),
            "A takes what B and C can spare: {}",
            r[0].w
        );
        assert!(n.is_valid());
    }

    #[test]
    fn moving_an_outer_divider_keeps_the_inner_ones_where_they_were() {
        let mut n = Node::Leaf;
        n.split(0, Dir::Cols);
        n.split(1, Dir::Cols); // inner divider at 0.75
        assert!(n.set_ratio(&[], 0.4)); // outer 0.5 -> 0.4
        let inner = n.dividers().into_iter().find(|d| d.path == [true]).unwrap();
        assert!(near(inner.pos, 0.75), "stays at 0.75: {}", inner.pos);
        assert!(n.is_valid());
    }

    #[test]
    fn dividers_lists_every_split_with_its_path() {
        let ds = BuiltIn::Main3.node().dividers();
        assert_eq!(ds.len(), 3);
        assert_eq!(ds[0].path, Vec::<bool>::new());
        assert_eq!(ds[0].dir, Dir::Cols);
        assert!(near(ds[0].pos, 2.0 / 3.0));
        assert_eq!(ds[1].path, vec![true]);
        assert_eq!(ds[1].dir, Dir::Rows);
        assert_eq!(ds[2].path, vec![true, true]);
    }

    #[test]
    fn a_tree_from_a_file_is_checked() {
        assert!(Node::Leaf.is_valid());
        for b in BuiltIn::ALL {
            assert!(b.node().is_valid());
        }
        // Seven leaves.
        let mut seven = Node::Leaf;
        for _ in 0..6 {
            seven = split(Dir::Rows, 0.5, seven, Node::Leaf);
        }
        assert!(!seven.is_valid(), "max 6 leaves");
        // Bad ratios.
        assert!(!split(Dir::Cols, f32::NAN, Node::Leaf, Node::Leaf).is_valid());
        assert!(!split(Dir::Cols, f32::INFINITY, Node::Leaf, Node::Leaf).is_valid());
        assert!(!split(Dir::Cols, 0.0, Node::Leaf, Node::Leaf).is_valid());
        assert!(!split(Dir::Cols, 1.0, Node::Leaf, Node::Leaf).is_valid());
        assert!(!split(Dir::Cols, -0.2, Node::Leaf, Node::Leaf).is_valid());
        // Under the minimum size.
        assert!(!split(Dir::Cols, 0.1, Node::Leaf, Node::Leaf).is_valid());
        assert!(!split(Dir::Rows, 0.9, Node::Leaf, Node::Leaf).is_valid());
        assert!(split(Dir::Rows, 0.15, Node::Leaf, Node::Leaf).is_valid());
    }

    // ------------------------------------------------------------ the gaps

    fn assert_exact_cover(tiles: &[Rect], work: Rect, gap: i32) {
        // Paint every pixel of the work area: a pixel is in at most one tile,
        // and the pixels in none are exactly the gaps.
        let mut hits = vec![0u8; (work.w * work.h) as usize];
        for t in tiles {
            assert!(t.x >= work.x && t.y >= work.y);
            assert!(t.right() <= work.right() && t.bottom() <= work.bottom());
            for y in t.y..t.bottom() {
                for x in t.x..t.right() {
                    hits[((y - work.y) * work.w + (x - work.x)) as usize] += 1;
                }
            }
        }
        assert!(hits.iter().all(|h| *h <= 1), "no overlap");
        if gap == 0 {
            assert!(hits.iter().all(|h| *h == 1), "no pixel lost");
        }
    }

    #[test]
    fn with_no_gap_the_tiles_cover_the_work_area_exactly() {
        let work = Rect::new(0, 0, 1920, 1040);
        for b in BuiltIn::ALL {
            let tiles = tile_rects(&b.node().to_rects(), work, 0);
            assert_exact_cover(&tiles, work, 0);
        }
        // An odd size and an origin that is not 0 (a second monitor).
        let work = Rect::new(-1366, 120, 1365, 727);
        for b in BuiltIn::ALL {
            let tiles = tile_rects(&b.node().to_rects(), work, 0);
            assert_exact_cover(&tiles, work, 0);
        }
    }

    #[test]
    fn a_gap_insets_the_outer_edges_fully_and_the_inner_ones_by_half() {
        let work = Rect::new(100, 50, 1000, 600);
        for gap in [8, 24] {
            let t = tile_rects(&BuiltIn::Cols2.node().to_rects(), work, gap);
            assert_eq!(t[0].x, work.x + gap, "left edge inset by the gap");
            assert_eq!(t[0].y, work.y + gap);
            assert_eq!(t[0].bottom(), work.bottom() - gap);
            assert_eq!(t[1].right(), work.right() - gap);
            assert_eq!(t[1].x - t[0].right(), gap, "neighbours exactly gap apart");
            assert_exact_cover(&t, work, gap);
        }
    }

    #[test]
    fn the_inner_halves_round_so_an_odd_gap_loses_no_pixel() {
        let work = Rect::new(0, 0, 1000, 600);
        let t = tile_rects(&BuiltIn::Cols2.node().to_rects(), work, 9);
        // The divider is at 500: left tile ends 5 before (rounds up), right
        // starts 4 after (rounds down); 9 apart.
        assert_eq!(t[0].right(), 500 - 5);
        assert_eq!(t[1].x, 500 + 4);
        assert_eq!(t[1].x - t[0].right(), 9);
    }

    #[test]
    fn a_grid_has_the_same_gap_in_both_directions() {
        let work = Rect::new(0, 0, 1920, 1080);
        let t = tile_rects(&BuiltIn::Grid4.node().to_rects(), work, 8);
        assert_eq!(t[1].x - t[0].right(), 8);
        assert_eq!(t[2].y - t[0].bottom(), 8);
        assert_eq!(t[3].x - t[2].right(), 8);
        assert_eq!(t[3].y - t[1].bottom(), 8);
        assert_exact_cover(&t, work, 8);
    }

    #[test]
    fn thirds_with_a_gap_keep_every_gap_equal() {
        let work = Rect::new(0, 0, 1920, 1080);
        let t = tile_rects(&BuiltIn::Cols3.node().to_rects(), work, 8);
        assert_eq!(t[1].x - t[0].right(), 8);
        assert_eq!(t[2].x - t[1].right(), 8);
        assert_exact_cover(&t, work, 8);
        let t = tile_rects(&BuiltIn::Main3.node().to_rects(), work, 24);
        assert_eq!(t[2].y - t[1].bottom(), 24);
        assert_eq!(t[3].y - t[2].bottom(), 24);
        assert_exact_cover(&t, work, 24);
    }

    #[test]
    fn the_gap_scales_with_the_monitors_dpi() {
        assert_eq!(scaled_gap(8, 96), 8);
        assert_eq!(scaled_gap(8, 144), 12, "150 %");
        assert_eq!(scaled_gap(8, 120), 10);
        assert_eq!(scaled_gap(24, 192), 48);
        assert_eq!(scaled_gap(0, 144), 0);
        assert_eq!(scaled_gap(99, 96), 24, "never past the maximum");
        // At 150 % the tiles sit 12 px apart and the screen edge is 12 px.
        let work = Rect::new(0, 0, 2880, 1620);
        let gap = scaled_gap(8, 144);
        let t = tile_rects(&BuiltIn::Cols2.node().to_rects(), work, gap);
        assert_eq!(t[0].x, 12);
        assert_eq!(t[1].x - t[0].right(), 12);
        assert_exact_cover(&t, work, gap);
    }

    // -------------------------------------------------------------- config

    fn parse(json: &str) -> ArrangeSettings {
        serde_json::from_str(json).expect("lenient")
    }

    #[test]
    fn the_defaults() {
        let d = ArrangeSettings::default();
        assert_eq!(d.gap, 8);
        assert!(d.templates.is_empty() && d.saved.is_empty());
        assert_eq!(parse("{}"), d);
    }

    #[test]
    fn the_gap_clamps() {
        assert_eq!(parse(r#"{"gap":0}"#).gap, 0);
        assert_eq!(parse(r#"{"gap":24}"#).gap, 24);
        assert_eq!(parse(r#"{"gap":99}"#).gap, 24);
        assert_eq!(parse(r#"{"gap":-5}"#).gap, 0);
        assert_eq!(parse(r#"{"gap":7.6}"#).gap, 8);
        assert_eq!(
            parse(r#"{"gap":"wide"}"#).gap,
            8,
            "not a number: the default"
        );
        assert_eq!(parse(r#"{"gap":null}"#).gap, 8);
    }

    fn tpl(id: &str, name: &str, root: &Node) -> String {
        format!(
            r#"{{"id":"{id}","name":"{name}","root":{}}}"#,
            serde_json::to_string(root).unwrap()
        )
    }

    #[test]
    fn a_broken_template_drops_alone() {
        let good = BuiltIn::Cols2.node();
        let bad_ratio = r#"{"id":"b","name":"Bad","root":{"Split":{"dir":"Cols","ratio":0.02,"a":"Leaf","b":"Leaf"}}}"#;
        let json = format!(
            r#"{{"templates":[{},{bad_ratio},{},{{"id":"x"}},5,{},{}]}}"#,
            tpl("a", "Dua", &good),
            tpl("c", "Tiga", &BuiltIn::Cols3.node()),
            // Name too long, name empty, not a tree.
            tpl("d", &"x".repeat(25), &good),
            r#"{"id":"e","name":"  ","root":"Leaf"}"#,
        );
        let s = parse(&json);
        let ids: Vec<_> = s.templates.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
        assert_eq!(s.templates[0].root, good);
    }

    #[test]
    fn templates_trim_names_and_drop_repeated_ids_and_cap_at_twelve() {
        let n = Node::Leaf;
        let s = parse(&format!(
            r#"{{"templates":[{},{}]}}"#,
            tpl("a", "  Kerja  ", &n),
            tpl("a", "Lain", &n)
        ));
        assert_eq!(s.templates.len(), 1);
        assert_eq!(s.templates[0].name, "Kerja");
        let many: Vec<String> = (0..15).map(|i| tpl(&format!("t{i}"), "T", &n)).collect();
        let s = parse(&format!(r#"{{"templates":[{}]}}"#, many.join(",")));
        assert_eq!(s.templates.len(), 12, "entries past 12 are dropped");
        assert_eq!(s.templates[11].id, "t11");
        // A 24 character name is kept, 25 is not.
        assert!(CustomTemplate::new("a", &"x".repeat(24), Node::Leaf).is_some());
        assert!(CustomTemplate::new("a", &"x".repeat(25), Node::Leaf).is_none());
        assert!(CustomTemplate::new("", "T", Node::Leaf).is_none());
    }

    #[test]
    fn a_non_array_is_an_empty_list() {
        for v in ["5", "\"x\"", "null", "{}", "true"] {
            let s = parse(&format!(r#"{{"templates":{v},"saved":{v}}}"#));
            assert!(s.templates.is_empty() && s.saved.is_empty(), "{v}");
        }
    }

    fn saved_json(name: &str, layout: &str, tiles: &str) -> String {
        format!(
            r#"{{"name":"{name}","layout":{layout},"monitor":{{"device":"\\\\.\\DISPLAY1","rect":{{"x":0,"y":0,"w":2560,"h":1440}}}},"tiles":{tiles}}}"#
        )
    }

    const WIN: &str = r#"{"exe":"C:\\a\\code.exe","class":"Chrome_WidgetWin_1","title":"main.rs"}"#;

    #[test]
    fn a_broken_saved_entry_drops_alone() {
        let ok = saved_json("Kerja", r#"{"builtin":"cols2"}"#, &format!("[{WIN},null]"));
        let custom = saved_json(
            "Mine",
            &format!(
                r#"{{"custom":{}}}"#,
                serde_json::to_string(&BuiltIn::Cols3.node()).unwrap()
            ),
            "[null,null,null]",
        );
        let unknown = saved_json("U", r#"{"builtin":"nine"}"#, "[null]");
        let bad_name = saved_json("   ", r#"{"builtin":"cols2"}"#, "[null,null]");
        let long_name = saved_json(&"y".repeat(25), r#"{"builtin":"cols2"}"#, "[null,null]");
        let wrong_count = saved_json("W", r#"{"builtin":"cols3"}"#, "[null,null]");
        let bad_tree = saved_json(
            "T",
            r#"{"custom":{"Split":{"dir":"Cols","ratio":7.0,"a":"Leaf","b":"Leaf"}}}"#,
            "[null,null]",
        );
        let bad_tile = saved_json("B", r#"{"builtin":"cols2"}"#, r#"[{"exe":1},null]"#);
        let json = format!(
            r#"{{"saved":[{ok},{unknown},{custom},{bad_name},{long_name},{wrong_count},{bad_tree},{bad_tile},"junk"]}}"#
        );
        let s = parse(&json);
        let names: Vec<_> = s.saved.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Kerja", "Mine"]);
        let first = &s.saved[0];
        assert_eq!(first.layout, LayoutRef::BuiltIn(BuiltIn::Cols2));
        assert_eq!(first.monitor.device, r"\\.\DISPLAY1");
        assert_eq!(first.monitor.rect, Rect::new(0, 0, 2560, 1440));
        assert_eq!(first.tiles[0].as_ref().unwrap().title, "main.rs");
        assert!(first.tiles[1].is_none());
    }

    #[test]
    fn an_old_hotkey_key_inside_arrange_is_ignored_and_the_rest_kept() {
        let tpl_json = tpl("a", "Dua", &BuiltIn::Cols2.node());
        let saved = saved_json("Kerja", r#"{"builtin":"cols2"}"#, "[null,null]");
        for hotkey in [
            "null",
            "5",
            "true",
            "[]",
            "{}",
            r#""Nope""#,
            r#""CtrlAltS""#,
        ] {
            let json = format!(
                r#"{{"hotkey":{hotkey},"gap":12,"templates":[{tpl_json}],"saved":[{saved}]}}"#
            );
            let v: Value = serde_json::from_str(&json).unwrap();
            let s = lenient_settings(v).expect("lenient");
            assert_eq!(s.gap, 12, "{hotkey}");
            assert_eq!(s.templates.len(), 1, "{hotkey}: templates kept");
            assert_eq!(s.saved.len(), 1, "{hotkey}: saved kept");
            // It does not come back when the settings are written.
            let out = serde_json::to_value(&s).unwrap();
            assert!(out.get("hotkey").is_none(), "{hotkey}");
        }
    }

    #[test]
    fn saved_entries_past_twelve_are_dropped() {
        let one = saved_json("S", r#"{"builtin":"cols2"}"#, "[null,null]");
        let many = vec![one; 14].join(",");
        assert_eq!(parse(&format!(r#"{{"saved":[{many}]}}"#)).saved.len(), 12);
    }

    #[test]
    fn the_settings_survive_a_round_trip() {
        let s = ArrangeSettings {
            gap: 12,
            templates: vec![CustomTemplate::new("t1", "Kerja", BuiltIn::Main2.node()).unwrap()],
            saved: vec![
                SavedArrangement::new(
                    "Susunan 1",
                    LayoutRef::BuiltIn(BuiltIn::Cols2),
                    SavedMonitor {
                        device: r"\\.\DISPLAY2".into(),
                        rect: Rect::new(-1920, 0, 1920, 1080),
                    },
                    vec![
                        Some(SavedWindow {
                            exe: r"C:\x\a.exe".into(),
                            class: "K".into(),
                            title: "T".into(),
                        }),
                        None,
                    ],
                )
                .unwrap(),
                SavedArrangement::new(
                    "Susunan 2",
                    LayoutRef::Custom(BuiltIn::Grid4.node()),
                    SavedMonitor {
                        device: String::new(),
                        rect: Rect::new(0, 0, 100, 100),
                    },
                    vec![None; 4],
                )
                .unwrap(),
            ],
        };
        let text = serde_json::to_string(&s).unwrap();
        let back: ArrangeSettings = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn saving_needs_a_good_name_and_a_matching_tile_count() {
        let mon = SavedMonitor {
            device: "D".into(),
            rect: Rect::new(0, 0, 10, 10),
        };
        let l = LayoutRef::BuiltIn(BuiltIn::Cols2);
        assert!(SavedArrangement::new("A", l.clone(), mon.clone(), vec![None, None]).is_some());
        assert!(SavedArrangement::new("", l.clone(), mon.clone(), vec![None, None]).is_none());
        assert!(SavedArrangement::new("A", l.clone(), mon.clone(), vec![None]).is_none());
        let empty = SavedMonitor {
            rect: Rect::default(),
            ..mon
        };
        assert!(SavedArrangement::new("A", l, empty, vec![None, None]).is_none());
    }

    #[test]
    fn new_ids_and_default_names_skip_what_is_taken() {
        let a = CustomTemplate::new("t1", "A", Node::Leaf).unwrap();
        let b = CustomTemplate::new("t3", "B", Node::Leaf).unwrap();
        assert_eq!(new_template_id(&[]), "t1");
        assert_eq!(new_template_id(&[a, b]), "t2");
        let mon = SavedMonitor {
            device: "D".into(),
            rect: Rect::new(0, 0, 10, 10),
        };
        let s = |name: &str| {
            SavedArrangement::new(
                name,
                LayoutRef::BuiltIn(BuiltIn::TwoThirds),
                mon.clone(),
                vec![None, None],
            )
            .unwrap()
        };
        assert_eq!(default_save_name("Susunan", &[]), "Susunan 1");
        assert_eq!(
            default_save_name("Susunan", &[s("Susunan 1"), s("Susunan 2")]),
            "Susunan 3"
        );
    }

    #[test]
    fn app_names_come_from_the_exe_without_the_extension() {
        assert_eq!(
            app_name(r"C:\Program Files\Mozilla Firefox\firefox.exe", "X"),
            "firefox"
        );
        assert_eq!(app_name("", "CabinetWClass"), "CabinetWClass");
        let mon = SavedMonitor {
            device: "D".into(),
            rect: Rect::new(0, 0, 10, 10),
        };
        let w = |exe: &str| {
            Some(SavedWindow {
                exe: exe.into(),
                class: "c".into(),
                title: "t".into(),
            })
        };
        let s = SavedArrangement::new(
            "S",
            LayoutRef::BuiltIn(BuiltIn::Cols3),
            mon,
            vec![w(r"C:\a\code.exe"), None, w(r"D:\b\Code.exe")],
        )
        .unwrap();
        assert_eq!(s.app_names(), ["code", "Code"]);
    }

    // ------------------------------------------------------------ matching

    fn open(hwnd: isize, exe: &str, class: &str, title: &str) -> OpenWindow {
        OpenWindow {
            hwnd,
            title: title.into(),
            class: class.into(),
            exe: exe.into(),
            minimized: false,
            icon_key: exe.to_lowercase(),
        }
    }

    fn saved(exe: &str, class: &str, title: &str) -> Option<SavedWindow> {
        Some(SavedWindow {
            exe: exe.into(),
            class: class.into(),
            title: title.into(),
        })
    }

    #[test]
    fn similarity_is_one_for_equal_and_zero_for_nothing_shared() {
        assert!(near(
            title_similarity("main.rs - Code", "main.rs - Code"),
            1.0
        ));
        assert!(
            near(title_similarity("Main.RS", "main rs"), 1.0),
            "case and punctuation"
        );
        assert!(near(title_similarity("", ""), 1.0));
        assert!(near(title_similarity("abc", ""), 0.0));
        assert!(near(title_similarity("abc", "xyz"), 0.0));
        let close = title_similarity("report-final.docx - Word", "report-final (2).docx - Word");
        let far = title_similarity("report-final.docx - Word", "budget.xlsx - Excel");
        assert!(close > far && close > 0.7 && far < 0.5, "{close} {far}");
        // Symmetric.
        assert!(near(
            title_similarity("abcde", "ace"),
            title_similarity("ace", "abcde")
        ));
    }

    #[test]
    fn an_exact_match_picks_the_same_window() {
        let list = vec![
            open(1, r"C:\a\code.exe", "Code", "main.rs"),
            open(2, r"C:\b\chrome.exe", "Chrome", "Docs"),
        ];
        let m = match_windows(
            &[
                saved(r"C:\b\chrome.exe", "Chrome", "Docs"),
                saved(r"C:\a\code.exe", "Code", "main.rs"),
            ],
            &list,
        );
        assert_eq!(m, vec![Some(1), Some(0)]);
    }

    #[test]
    fn the_exe_path_matches_in_any_case() {
        let list = vec![open(1, r"C:\A\Code.EXE", "Code", "x")];
        let m = match_windows(&[saved(r"c:\a\code.exe", "Other", "x")], &list);
        assert_eq!(m, vec![Some(0)]);
    }

    #[test]
    fn two_windows_of_one_exe_are_told_apart_by_title() {
        let list = vec![
            open(1, r"C:\a\code.exe", "Code", "notes.md - Code"),
            open(2, r"C:\a\code.exe", "Code", "main.rs - Code"),
        ];
        let m = match_windows(
            &[
                saved(r"C:\a\code.exe", "Code", "main.rs - Code"),
                saved(r"C:\a\code.exe", "Code", "notes.md - Code"),
            ],
            &list,
        );
        assert_eq!(m, vec![Some(1), Some(0)]);
    }

    #[test]
    fn the_same_class_beats_a_closer_title() {
        let list = vec![
            open(1, r"C:\a\app.exe", "Dialog", "report"),
            open(2, r"C:\a\app.exe", "Main", "unrelated words"),
        ];
        let m = match_windows(&[saved(r"C:\a\app.exe", "Main", "report")], &list);
        assert_eq!(m, vec![Some(1)]);
    }

    #[test]
    fn a_window_is_used_once() {
        let list = vec![open(1, r"C:\a\x.exe", "X", "one")];
        let m = match_windows(
            &[
                saved(r"C:\a\x.exe", "X", "one"),
                saved(r"C:\a\x.exe", "X", "one"),
            ],
            &list,
        );
        assert_eq!(m, vec![Some(0), None], "the second slot is skipped");
    }

    #[test]
    fn a_title_tie_goes_to_the_earlier_window_and_slots_are_greedy_in_order() {
        let list = vec![
            open(1, r"C:\a\x.exe", "X", "same"),
            open(2, r"C:\a\x.exe", "X", "same"),
        ];
        let m = match_windows(
            &[
                saved(r"C:\a\x.exe", "X", "same"),
                saved(r"C:\a\x.exe", "X", "same"),
            ],
            &list,
        );
        assert_eq!(m, vec![Some(0), Some(1)]);
    }

    #[test]
    fn a_missing_app_and_an_empty_slot_are_skipped() {
        let list = vec![open(1, r"C:\a\code.exe", "Code", "x")];
        let m = match_windows(
            &[
                saved(r"C:\z\gone.exe", "Gone", "x"),
                None,
                saved(r"C:\a\code.exe", "Code", "x"),
            ],
            &list,
        );
        assert_eq!(m, vec![None, None, Some(0)]);
    }

    #[test]
    fn with_no_exe_the_class_is_the_fallback() {
        let list = vec![
            open(1, r"C:\a\one.exe", "CabinetWClass", "Downloads"),
            open(2, r"C:\a\two.exe", "Notepad", "a.txt"),
        ];
        let m = match_windows(&[saved("", "Notepad", "a.txt")], &list);
        assert_eq!(m, vec![Some(1)]);
        // No exe and no class: nothing to go on.
        assert_eq!(match_windows(&[saved("", "", "a.txt")], &list), vec![None]);
        // A class that is not open.
        assert_eq!(match_windows(&[saved("", "Nope", "x")], &list), vec![None]);
    }

    // ------------------------------------------------------------ monitors

    fn mon(device: &str, x: i32, w: i32) -> Monitor {
        Monitor {
            device: device.into(),
            rect: Rect::new(x, 0, w, 1080),
            work: Rect::new(x, 0, w, 1040),
            dpi: 96,
        }
    }

    #[test]
    fn a_saved_monitor_is_found_by_name_then_by_rect_then_the_notchs() {
        let monitors = vec![
            mon(r"\\.\DISPLAY1", 0, 1920),
            mon(r"\\.\DISPLAY2", 1920, 2560),
        ];
        let by_name = SavedMonitor {
            device: r"\\.\display2".into(),
            rect: Rect::new(0, 0, 1920, 1080),
        };
        assert_eq!(
            resolve_monitor(&by_name, &monitors, 0),
            Some(1),
            "the name wins"
        );
        let by_rect = SavedMonitor {
            device: r"\\.\DISPLAY9".into(),
            rect: Rect::new(1920, 0, 2560, 1080),
        };
        assert_eq!(resolve_monitor(&by_rect, &monitors, 0), Some(1));
        let neither = SavedMonitor {
            device: r"\\.\DISPLAY9".into(),
            rect: Rect::new(5, 5, 5, 5),
        };
        assert_eq!(resolve_monitor(&neither, &monitors, 1), Some(1));
        assert_eq!(resolve_monitor(&neither, &monitors, 0), Some(0));
        assert_eq!(
            resolve_monitor(&neither, &monitors, 7),
            Some(1),
            "out of range is clamped"
        );
        assert_eq!(resolve_monitor(&neither, &[], 0), None);
        // An empty saved name never matches an empty device.
        let blank = vec![mon("", 0, 1920)];
        let unnamed = SavedMonitor {
            device: String::new(),
            rect: Rect::new(9, 9, 9, 9),
        };
        assert_eq!(resolve_monitor(&unnamed, &blank, 0), Some(0));
    }

    // ------------------------------------------------------------- the flow

    #[test]
    fn the_flow_fills_the_tiles_in_order() {
        let mut f = PickFlow::new(3);
        assert_eq!((f.current(), f.tiles(), f.is_done()), (0, 3, false));
        assert!(f.pick(10));
        assert!(f.pick(20));
        assert_eq!(f.current(), 2);
        assert!(f.pick(30));
        assert!(f.is_done());
        assert_eq!(f.picked(), &[Some(10), Some(20), Some(30)]);
        assert!(!f.pick(40), "done");
        assert!(!f.skip(), "done");
    }

    #[test]
    fn a_picked_window_cannot_be_picked_twice() {
        let mut f = PickFlow::new(3);
        assert!(f.pick(10));
        assert!(!f.pick(10));
        assert_eq!(f.current(), 1, "nothing moved on");
        assert!(f.is_picked(10) && !f.is_picked(11));
    }

    #[test]
    fn skip_leaves_the_tile_empty_and_moves_on() {
        let mut f = PickFlow::new(3);
        assert!(f.skip());
        assert!(f.pick(5));
        assert!(f.skip());
        assert!(f.is_done());
        assert_eq!(f.picked(), &[None, Some(5), None]);
    }

    #[test]
    fn back_goes_one_tile_and_frees_its_window() {
        let mut f = PickFlow::new(3);
        assert!(!f.back(), "at the first tile nothing happens");
        f.pick(1);
        f.pick(2);
        assert!(f.back());
        assert_eq!(f.current(), 1);
        assert!(!f.is_picked(2), "its window is free again");
        assert!(f.is_picked(1));
        assert!(f.pick(2), "and can go in again");
        // Back over a skipped tile just steps back.
        let mut g = PickFlow::new(2);
        g.skip();
        assert!(g.back());
        assert_eq!(g.current(), 0);
        assert!(!g.back());
        // Back from done reopens the last tile.
        let mut h = PickFlow::new(1);
        h.pick(1);
        assert!(h.is_done());
        assert!(h.back());
        assert!(!h.is_done());
        assert_eq!(h.picked(), &[None]);
    }

    #[test]
    fn picked_windows_leave_the_list() {
        let list = vec![
            open(1, "a", "A", "a"),
            open(2, "b", "B", "b"),
            open(3, "c", "C", "c"),
        ];
        let mut f = PickFlow::new(3);
        assert_eq!(f.available(&list).len(), 3);
        f.pick(2);
        let left: Vec<_> = f.available(&list).iter().map(|w| w.hwnd).collect();
        assert_eq!(left, vec![1, 3]);
        f.back();
        assert_eq!(f.available(&list).len(), 3);
    }

    // ------------------------------------------------------------- notices

    #[test]
    fn the_numbers_of_the_notices() {
        let c = Counts::picked(4, 3);
        assert_eq!((c.moved, c.skipped, c.skipped_shown()), (3, 1, Some(1)));
        let c = Counts::picked(3, 3);
        assert_eq!(c.skipped_shown(), None, "nothing skipped: no suffix");
        let c = Counts::saved_run(5, 3);
        assert_eq!((c.moved, c.wanted, c.skipped), (3, 5, 2));
        let c = Counts::saved_run(2, 5);
        assert_eq!(c.skipped, 0, "never negative");
    }
}

// ------------------------------------------------------------------- events

/// A window's place before Vale moved it, kept for "Batalkan" (a plain copy
/// of `WINDOWPLACEMENT`, so the event stays comparable).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub hwnd: isize,
    pub flags: u32,
    pub show: u32,
    pub min: (i32, i32),
    pub max: (i32, i32),
    /// The restored rectangle: left, top, right, bottom (workspace
    /// coordinates, as `WINDOWPLACEMENT` has them).
    pub normal: (i32, i32, i32, i32),
    /// Where a window in its normal state really was (`GetWindowRect`, screen
    /// coordinates): left, top, right, bottom. A snapped window's `normal` is
    /// its place before the snap, not this. `None` when it was minimized or
    /// maximized.
    pub rect: Option<(i32, i32, i32, i32)>,
}

/// What the workers of `arrange_win` report to the island.
#[derive(Clone, Debug, PartialEq)]
pub enum ArrangeEvent {
    /// A move finished: `run` is the number `App` gave it, `moved` windows
    /// are in place, `skipped` were not moved (gone, hung, elevated, refused
    /// the move), `undo` the places they had before.
    Done {
        run: u64,
        moved: usize,
        skipped: usize,
        undo: Vec<Placement>,
    },
    /// An app's icon, read off the UI thread (`None`: it has none).
    Icon {
        key: String,
        thumb: Option<crate::clipboard::image::Thumb>,
    },
}
