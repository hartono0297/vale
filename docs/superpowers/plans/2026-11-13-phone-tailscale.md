# Phone link over Tailscale Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A **Wi-Fi | Tailscale** switch on the shelf's phone link: in Tailscale mode the session listens on the PC's Tailscale address (100.x) and only accepts tailnet peers.

**Architecture:** Vale does not ship or talk to Tailscale. It reads the Tailscale adapter's 100.x address from the adapter list it already reads (`net::read_adapters`), binds there, and filters peers by the tailnet ranges. The mode is a new island-owned config key, chosen by a two-item segment in the header row of the QR view and the warning card; a tap saves it and restarts the session.

**Tech Stack:** Rust 2024, `windows` crate (no new features), Direct2D/DirectWrite renderer, serde config. No new crate.

**Spec:** `docs/superpowers/specs/2026-11-13-phone-tailscale-design.md` (read it first; this plan does not repeat its reasons).

## Global Constraints

- No new crate, no new `windows` feature, no new timer, thread or poll at idle (`docs/memory/bata-idle-power-budget.md`).
- Vale never binds `0.0.0.0`, never calls Tailscale's LocalAPI or servers, never changes the firewall.
- `pick_adapter` and `is_private` keep their current behaviour (Wi-Fi mode unchanged).
- Tailscale ranges: IPv4 `100.64.0.0/10`; IPv6 `fd7a:115c:a1e0::/48`; an IPv4-mapped IPv6 address is judged as its IPv4.
- Adapter rule: up, name or description contains `tailscale` (case-insensitive), an IPv4 in `100.64.0.0/10`; no gateway required.
- Config key `phone_net`, values `"Wifi"` / `"Tailscale"`, default and anything unreadable: `Wifi`; island-owned (`merge_owned`).
- Exact strings (Indonesian / English), no em dash in any new string:
  - `PhoneNetWifi`: "Wi-Fi" / "Wi-Fi"
  - `PhoneNetTailscale`: "Tailscale" / "Tailscale"
  - `PhoneWarnNoTailscale`: "Tailscale belum jalan" / "Tailscale isn't running"
  - `PhoneWarnNoTailscaleBody`: "Nyalakan Tailscale di PC ini dan login, lalu coba lagi." / "Turn on Tailscale on this PC and sign in, then try again."
  - `PhoneTailscaleHint`: "HP juga harus pakai Tailscale, akun yang sama" / "The phone needs Tailscale too, same account"
  - `PhoneTailscaleOnly`: "Lewat Tailscale, terenkripsi dari HP ke PC" / "Through Tailscale, encrypted from phone to PC"
- In Tailscale mode the network line's first line is just `Tailscale` (no "pribadi"/"publik" word); the session is never flagged Public.
- Tests: `cargo test --locked`. Tests never open sockets other than `127.0.0.1` port 0, never read real adapters (`docs/memory/test-spawns-no-window.md`).
- Subagents: never the Fable model. UI and copy work loads `antislop:antislop`, `antislop:antislop-ui` and `antislop:antislop-copywriting` (antislop mode: during).

## Review Focus

1. Switching while a session is still starting (adapter look on the server thread): the old session's late `Ready`/`Refused` must not land on the new one. Owned by Task 3 (`a_switch_while_starting_takes_a_new_generation`).
2. A non-Tailscale adapter with a carrier-grade NAT 100.x address (some mobile hotspots and ISPs hand those out) must not be picked as Tailscale, and Wi-Fi mode must keep refusing 100.x peers. Owned by Task 1 (`a_cgnat_address_on_another_adapter_is_not_tailscale`, `each_mode_refuses_the_other_modes_peers`).
3. A config written by v5.3.0 (no `phone_net`) reads as Wi-Fi, and junk (`7`, `null`, `"tail"`) reads as Wi-Fi without breaking the other keys. Owned by Task 1 (`phone_net_defaults_to_wifi_and_reads_leniently`).
4. Tapping the lit segment on a warning card does nothing (no retry, no restart); tapping during an upload does nothing. Owned by Task 3 (`switch_plan_rules`).
5. The switch labels and the new strings fit in DM Sans and JetBrains Mono, Pill and Notch, ID and EN, and the segment never runs into the chip. Owned by Task 4 (the fit test and `the_net_switch_sits_between_back_and_chip`).

---

### Task 1: Tailscale rules and the config key

**Files:**
- Modify: `src/config.rs` (new enum, field, lenient reader, default, `merge_owned`, tests)
- Modify: `src/phone/net.rs` (three pure functions, tests)

**Interfaces:**
- Produces: `config::PhoneNet { Wifi, Tailscale }` (`Clone, Copy, Debug, PartialEq, Eq, Default = Wifi`, serde as `"Wifi"`/`"Tailscale"`), `Config::phone_net: PhoneNet`.
- Produces: `net::is_tailnet(ip: IpAddr) -> bool`, `net::pick_tailscale(adapters: &[AdapterInfo]) -> Option<Ipv4Addr>`, `net::peer_allowed(ip: IpAddr, via: PhoneNet, allow_loopback: bool) -> bool`.

- [ ] **Step 1: Write the failing tests**

In `config.rs` tests, following `ui_font_defaults_to_sans_reads_leniently_and_belongs_to_the_island`:

```rust
#[test]
fn phone_net_defaults_to_wifi_and_reads_leniently() {
    assert_eq!(Config::default().phone_net, PhoneNet::Wifi);
    assert_eq!(parse(r#"{"style":"Notch"}"#).phone_net, PhoneNet::Wifi);
    assert_eq!(parse(r#"{"phone_net":"Tailscale"}"#).phone_net, PhoneNet::Tailscale);
    for junk in ["7", "null", r#""tail""#, "{}", "[]"] {
        let c = parse(&format!(r#"{{"style":"Notch","accent":"Sage","phone_net":{junk}}}"#));
        assert_eq!(c.phone_net, PhoneNet::Wifi, "{junk}");
        assert_eq!(c.style, Style::Notch, "{junk}");
    }
    let ts = Config { phone_net: PhoneNet::Tailscale, ..Config::default() };
    assert!(serde_json::to_string(&ts).unwrap().contains(r#""phone_net":"Tailscale""#));
    assert_eq!(merge_owned(Config::default(), &ts).phone_net, PhoneNet::Tailscale);
}
```

In `net.rs` tests (use the existing `ad(name, if_type, ip)` helper; set `has_gateway: false` on the Tailscale one):

```rust
#[test]
fn the_tailscale_adapter_is_found_without_a_gateway() {
    let mut ts = ad("Tailscale", 53, "100.101.102.5");
    ts.description = "Tailscale Tunnel".into();
    ts.has_gateway = false;
    let wifi = ad("Wi-Fi", IF_WIFI, "192.168.1.20");
    assert_eq!(pick_tailscale(&[wifi.clone(), ts.clone()]), Some("100.101.102.5".parse().unwrap()));
    let mut down = ts.clone();
    down.up = false;
    assert_eq!(pick_tailscale(&[down]), None);
    let mut no_100 = ts.clone();
    no_100.ipv4 = vec!["169.254.9.9".parse().unwrap()];
    assert_eq!(pick_tailscale(&[no_100]), None);
    assert_eq!(pick_tailscale(&[wifi]), None);
    let mut renamed = ts;
    renamed.name = "Ethernet 7".into(); // description still says Tailscale
    assert!(pick_tailscale(&[renamed]).is_some());
}

#[test]
fn a_cgnat_address_on_another_adapter_is_not_tailscale() {
    let hotspot = ad("Cellular", IF_WIFI, "100.72.3.4");
    assert_eq!(pick_tailscale(&[hotspot]), None);
}

#[test]
fn tailnet_ranges() {
    for ip in ["100.64.0.0", "100.101.102.103", "100.127.255.255", "fd7a:115c:a1e0::1", "::ffff:100.64.0.1"] {
        assert!(is_tailnet(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["100.63.255.255", "100.128.0.0", "192.168.1.1", "10.0.0.1", "fd7a:115c:a1e1::1", "127.0.0.1"] {
        assert!(!is_tailnet(ip.parse().unwrap()), "{ip}");
    }
}

#[test]
fn each_mode_refuses_the_other_modes_peers() {
    let lan: IpAddr = "192.168.1.30".parse().unwrap();
    let ts: IpAddr = "100.90.1.2".parse().unwrap();
    assert!(peer_allowed(lan, PhoneNet::Wifi, false));
    assert!(!peer_allowed(ts, PhoneNet::Wifi, false));
    assert!(peer_allowed(ts, PhoneNet::Tailscale, false));
    assert!(!peer_allowed(lan, PhoneNet::Tailscale, false));
    let lo: IpAddr = "127.0.0.1".parse().unwrap();
    for via in [PhoneNet::Wifi, PhoneNet::Tailscale] {
        assert!(!peer_allowed(lo, via, false));
        assert!(peer_allowed(lo, via, true));
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --locked phone_net_ tailscale tailnet each_mode`
Expected: compile errors (`PhoneNet`, `pick_tailscale`, `is_tailnet`, `peer_allowed` not defined).

- [ ] **Step 3: Implement**

- `config.rs`: `PhoneNet` next to `UiFont`; field `phone_net` with `#[serde(default, deserialize_with = "phone_net")]` and a lenient reader like `ui_font` (case-insensitive trim of `"wifi"`/`"tailscale"`, anything else `Wifi`); `Config::default()` and `merge_owned` (`phone_net: mem.phone_net`).
- `net.rs`: `is_tailnet` by the ranges in Global Constraints; `pick_tailscale` by the adapter rule (first matching adapter, first 100.64/10 address on it); `peer_allowed`: loopback passes only with `allow_loopback`, else `Wifi => is_private(ip, false)`, `Tailscale => is_tailnet(ip)`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked phone_net_ tailscale tailnet each_mode` then `cargo test --locked phone::net config::`
Expected: PASS, nothing else broken.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/phone/net.rs
git commit -m "feat(phone): Tailscale address and peer rules, phone_net config key"
```

---

### Task 2: The server listens on the chosen network

**Files:**
- Modify: `src/phone/mod.rs` (`ServerConfig`, `Runtime`, `Mode::Lan`, `Refusal`, `PhoneEvent::Ready`, `start_lan`, new `detect_tailscale`)
- Modify: `src/phone/server.rs` (`run` picks the detector, `accept` uses `peer_allowed`, `Runtime`/`Ready` carry the mode)
- Modify: `src/phone/debug.rs`, `src/phone/e2e.rs`, `src/phone/net.rs` test at the `Refusal` match (~line 441): add the new field / arm
- Modify: `src/ui/phone_view.rs` (`Warn::NoTailscale`), `src/app_phone.rs` (`warn_of`: `Refusal::NoTailscale => Warn::NoTailscale`), `src/ui/render_panel.rs` (`paint_phone_warn`: `Warn::NoTailscale => (Key::PhoneWarnNoTailscale, Key::PhoneWarnNoTailscaleBody)`), `src/i18n.rs` (those two keys, exact strings from Global Constraints). Nothing else in the app changes in this task.

**Interfaces:**
- Consumes: Task 1's `PhoneNet`, `pick_tailscale`, `peer_allowed`.
- Produces: `ServerConfig::net: PhoneNet`; `Runtime::net: PhoneNet`; `Refusal::NoTailscale`; `PhoneEvent::Ready { .., net: PhoneNet }`; `pub fn start_lan(lang: Lang, net: PhoneNet, sink: Sink) -> Handle`; `Warn::NoTailscale`; `Key::PhoneWarnNoTailscale`, `Key::PhoneWarnNoTailscaleBody`.

- [ ] **Step 1: Write the failing tests**

In `e2e.rs`, next to the existing loopback start test, a session started with `Mode::Fixed` on `127.0.0.1` and `net: PhoneNet::Tailscale`, `allow_loopback: true`:

```rust
#[test]
fn ready_says_which_network_the_session_is_on() {
    // start(cfg with net: PhoneNet::Tailscale) ... wait for Ready
    // assert!(matches!(ready, PhoneEvent::Ready { net: PhoneNet::Tailscale, public: false, .. }));
}
```

In `app_phone.rs` tests: `assert_eq!(warn_of(&Refusal::NoTailscale), Warn::NoTailscale);`

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked ready_says_which_network warn_of`
Expected: compile errors on the missing field / variant.

- [ ] **Step 3: Implement**

- `detect_tailscale(lang: Lang, pc_name: &str) -> Result<(ServerConfig, Ipv4Addr), Refusal>` next to `detect_lan`: `pick_tailscale(&read_adapters())` or `Refusal::NoTailscale`; `network_name: "Tailscale"`, `public: false`, `net: PhoneNet::Tailscale`, same `dest_dir`, `ports: PORTS`, no Network List Manager call (no COM needed). `detect_lan` sets `net: PhoneNet::Wifi`.
- `Mode::Lan { lang, pc_name, net }`; `server::run` calls `detect_tailscale` or `detect_lan` by `net`; the returned IP is the `watch` address for `BoundCheck` in both modes.
- `accept`: `if !peer_allowed(peer.ip(), rt.net, rt.allow_loopback) { return; }`.
- `Ready` carries `net: rt.net`. Update every constructor and match (debug, e2e, demo if it builds `Ready`).

- [ ] **Step 4: Run the tests**

Run: `cargo test --locked phone::`
Expected: PASS (all existing phone tests too).

- [ ] **Step 5: Commit**

```bash
git add src/phone src/app_phone.rs src/ui/phone_view.rs src/i18n.rs src/ui/render_panel.rs
git commit -m "feat(phone): listen on the Tailscale address in Tailscale mode"
```

---

### Task 3: The switch's rules (reducer and app)

**Files:**
- Modify: `src/model.rs` (`Hit::PhoneNet(PhoneNet)`, `Effect::PhoneNet(PhoneNet)`)
- Modify: `src/state.rs` (reducer arm next to `Hit::PhoneNewQr`, tests)
- Modify: `src/app.rs` (`Effect::PhoneNet(n) => self.phone_switch(n)`)
- Modify: `src/app_phone.rs` (`phone_begin` reads the config; `phone_switch`; `switch_plan`; `Ready` keeps `net`; `phone_view` fills the new fields)
- Modify: `src/ui/phone_view.rs` (`PhoneView::net`, `PhoneView::switch_live`, `CodeView::tailnet`)

**Interfaces:**
- Consumes: Task 2's `start_lan(lang, net, sink)`, `Ready { net }`.
- Produces: `PhoneView { .., net: PhoneNet, switch_live: bool }` (the lit segment: the saved config value; `switch_live` false while any row is `RowStatus::Receiving(_)`), `CodeView { .., tailnet: bool }` (from the running session's `Ready`), `Hit::PhoneNet(PhoneNet)`.
- Produces: `fn switch_plan(saved: PhoneNet, picked: PhoneNet, uploading: bool) -> Switch` with `enum Switch { Nothing, Restart }` in `app_phone.rs`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn switch_plan_rules() {
    use PhoneNet::*;
    assert_eq!(switch_plan(Wifi, Wifi, false), Switch::Nothing);
    assert_eq!(switch_plan(Tailscale, Tailscale, false), Switch::Nothing);
    assert_eq!(switch_plan(Wifi, Tailscale, false), Switch::Restart);
    assert_eq!(switch_plan(Tailscale, Wifi, false), Switch::Restart);
    assert_eq!(switch_plan(Wifi, Tailscale, true), Switch::Nothing);
}
```

In `state.rs`, next to the test around line 10103: `Hit::PhoneNet(PhoneNet::Tailscale)` gives `[Effect::PhoneNet(PhoneNet::Tailscale)]` while the stage shown is `PhoneStage::Qr` (the QR view and the warning card both are), and no effect on the Receiving stage or with no phone stage.

In `app_phone.rs` (pure part of the restart, testable without a window): `a_switch_while_starting_takes_a_new_generation`: with `PhoneState { starting: true, generation: 4, .. }`, the restart path bumps `generation` to 5 and an `Event::Phone(4, Ready{..})` is then ignored by the existing generation check. If the restart is not reachable without `App`, test the generation rule at the `PhoneState` level the way the existing late-`SessionEnded` test does.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked switch_plan phone_net generation`
Expected: compile errors.

- [ ] **Step 3: Implement**

- `phone_begin` passes `self.config.phone_net` to `start_lan`.
- `phone_switch(&mut self, picked: PhoneNet)`: `uploading` = any row `Receiving`; on `Switch::Restart`: `self.config.phone_net = picked; self.save_config();` then, if a handle exists, `phone_stop(EndReason::Done)`; then `phone_begin()`; keep the stage `PhoneStage::Qr`. On `Nothing`: return.
- `phone_view` sets `net: <saved config value>` (pass it in as a parameter; `phone_view` is an associated fn taking `state` and `st`), `switch_live: !uploading`, `CodeView::tailnet: ready.net == PhoneNet::Tailscale`.
- Reducer: `Hit::PhoneNet(n) => if self.phone_shown() == Some(PhoneStage::Qr) { effects.push(Effect::PhoneNet(n)) }`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/model.rs src/state.rs src/app.rs src/app_phone.rs src/ui/phone_view.rs
git commit -m "feat(phone): switch between Wi-Fi and Tailscale from the QR view"
```

---

### Task 4: The switch on screen, the Tailscale texts, demo scenes

Load `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-copywriting` first.

**Files:**
- Modify: `src/ui/panel_layout.rs` (`PhoneQrLayout`, `PhoneWarnLayout`: `title` replaced by `seg: Rect` and `seg_items: [Rect; 2]`; hits; tests)
- Modify: `src/ui/render_panel.rs` (paint the segment; network line, hint and footer by mode; the fit test)
- Modify: `src/i18n.rs` (`PhoneNetWifi`, `PhoneNetTailscale`, `PhoneTailscaleHint`, `PhoneTailscaleOnly`)
- Modify: `src/ui/demo.rs` (`PhoneDemo::QrTailscale` scene `phone-qr-tailscale`, `PhoneDemo::NoTailscale` scene `phone-notailscale`; the module doc comment lists them)

**Interfaces:**
- Consumes: Task 3's `PhoneView::{net, switch_live}`, `CodeView::tailnet`, `Hit::PhoneNet`; Task 2's `Warn::NoTailscale`.

- [ ] **Step 1: Write the failing tests**

In `panel_layout.rs`:

```rust
#[test]
fn the_net_switch_sits_between_back_and_chip() {
    for style in [Style::Pill, Style::Notch] {
        let s = shape_for(style, ShapeKind::Panel);
        for strip in [false, true] {
            let q = phone_qr_layout(s, strip, false);
            assert!(q.seg.x >= q.back.right() + 8.0);
            assert!(q.seg.right() <= q.chip.x - 8.0);
            assert!(q.seg_items[0].right() <= q.seg_items[1].x);
            let w = phone_warn_layout(s, strip);
            assert!(w.seg.x >= w.back.right() + 8.0);
            assert!(w.seg.right() <= header_right(s));
        }
    }
}
```

Extend the existing hit test near line 7494 so the QR view's hits are `[Hit::PhoneBack, Hit::PhoneNet(PhoneNet::Wifi), Hit::PhoneNet(PhoneNet::Tailscale), ..]` and the warning card's include both segments. Extend `every_phone_string_holds_its_box_in_both_fonts` with: both segment labels in `seg_items` (`small_center`), `PhoneWarnNoTailscale` (`title`, heading), `PhoneWarnNoTailscaleBody` (body, as the other bodies are checked), `PhoneTailscaleHint` (`small`, `inset(qr.hint, 10.0)`), `PhoneTailscaleOnly` (`small`, `qr.foot` and `warn.foot`), and `"Tailscale"` on the network line.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked the_net_switch every_phone_string`
Expected: compile errors / failures.

- [ ] **Step 3: Implement**

- Layout: the segment is 22 dip high, vertically centred in the 26 dip header row, starting at `back.right() + 8.0`; two equal items of a fixed width (`PHONE_SEG_ITEM_W`, start at 74.0; widen only if the fit test needs it) inside a 2 dip inset. The receiving view keeps its title.
- Paint: the same look as the cleaner's mode segment (`paint_clean_head`: `p.raised` track with `SEG_RADIUS`, `p.raised_hi` lit item with `SEG_ITEM_RADIUS`, `small_center` labels, `p.text` lit / `p.secondary` other). Extract the segment part into one helper used by both, rather than copying it. When `switch_live` is false draw it at the cleaner's inert alpha (`CLEAN_INERT`) and add no hits for it.
- Code view: `tailnet` true: line 1 is `Tailscale` alone; the hint box is `PhoneTailscaleHint`; the footer `PhoneTailscaleOnly`. Otherwise unchanged. Warning card and the starting state: footer by `v.net`.
- Demo scenes as listed; `phone-qr` shows the switch on Wi-Fi.

- [ ] **Step 4: Run the tests and look**

Run: `cargo test --locked`
Expected: PASS.
Run: `cargo run --release -- --demo phone-qr-tailscale` and `-- --demo phone-notailscale` (stop an installed copy first: `Stop-Process -Name vale -ErrorAction SilentlyContinue`), in DM Sans and JetBrains Mono (`DI_FONT`), and take screenshots for the owner.

- [ ] **Step 5: Commit**

```bash
git add src/ui src/i18n.rs
git commit -m "feat(phone): Wi-Fi | Tailscale switch on the QR view and warning card"
```

---

### After the tasks (the session that owns the run)

- Whole-branch review, merge into `main`, `cargo test --locked` on `main`.
- `cargo build --release --locked`, record the exe size in `docs/memory/exe-size.md`.
- README privacy paragraph, File shelf row and `docs/guide.md` as the spec's "Docs" section says; `docs/memory/shelf-phone.md` "Tailscale mode" section and its index line. Commit "docs: update memory and README", push `main`, clean up the worktree.
