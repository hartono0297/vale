# Phone link over Tailscale (design)

Date: 2026-11-13. Settled in chat with the owner, in their words: "modul shelf,
instead maksa 1 jaringan via wifi, pake semacam tailscale aja", then "kasih jadi
option aja, bisa pilih lewat wifi atau tailscale", then A (use the Tailscale app
that is already installed), then "Di layar QR" for the switch, then "Udah pas"
for the behaviour below. Lines marked **builder's call** were proposed by the
agent and accepted as part of that "Udah pas"; anything not covered is left to
the builder (see the end).

## Intent

The shelf's **HP** link works only when the phone and the PC are on the same
local network (`docs/memory/shelf-phone.md`). The owner wants it to work when
they are not, for example the phone on mobile data away from home, by going
through Tailscale. The owner wants a choice, not a replacement: **Wi-Fi** (today's
behaviour) or **Tailscale**, picked on the QR view.

## Goals

- A two-way switch **Wi-Fi | Tailscale** on the phone link's QR view.
- In Tailscale mode the session listens on the PC's Tailscale address
  (100.x) and only phones on the owner's tailnet that scanned the QR get in.
- Everything else about a session stays as it is: the token, the cookie, the
  shared shelf, the uploads to `Downloads\Vale`, the session end rules.

## Non-goals

- Tailscale built into Vale (`tsnet`, `tailscale-rs`): rejected. `tailscale-rs`
  is experimental (it needs `TS_RS_EXPERIMENT=this_is_unstable_software`),
  `tsnet` is Go and would grow the ~4.7 MB exe many times over, and Vale would
  have to keep a Tailscale login key. The phone needs the Tailscale app either
  way.
- Tailscale Funnel, public links, anything reachable from the open internet.
- Showing the Tailscale device or user name (`tailscale whois`, LocalAPI). The
  chip keeps the User-Agent kind it shows today.
- An "automatic" mode that picks the network by itself.
- Listening on Wi-Fi and Tailscale at the same time.

## Anti-goals (a failure even if it works)

- Vale talks to Tailscale's servers or its LocalAPI, starts, installs or logs in
  to Tailscale, or ships any Tailscale code.
- Vale listens on `0.0.0.0` or on more than the one chosen address.
- A phone on the PC's Wi-Fi gets in while Tailscale mode is on.
- Vale changes the Windows Firewall or elevates.
- A new timer, thread or poll while no phone session runs
  (`docs/memory/bata-idle-power-budget.md`).

## Approaches considered

1. **Use the Tailscale app already installed (picked).** The owner installs
   and signs in to Tailscale on the PC and the phone. Vale only reads the PC's
   Tailscale address from Windows' adapter list and listens there. No new
   dependency, no size cost, nothing running at idle, and the traffic is
   end-to-end encrypted by WireGuard so plain HTTP inside it cannot be read on
   the way. Cost: Tailscale must be installed and on at both ends.
2. **Tailscale inside Vale** (`tailscale-rs` or Go `tsnet`): rejected for the
   reasons under Non-goals.
3. **A relay or tunnel** (Cloudflare Tunnel, ngrok) or **WebRTC with a
   signalling server**: dropped in the first round. Both put a third-party
   server in the path that the owner did not ask for, and both need far more
   code than reading one adapter.

## Behaviour

### The switch

- A segmented control **Wi-Fi | Tailscale** (the same words in Indonesian and
  English) on the **QR view** and on the **warning card** (the "no local
  network", "ports in use", "could not start" and the new "Tailscale isn't
  running" card), so a missing network on one side can be fixed by tapping the
  other. Not on the receiving view.
- The choice is saved in the config and used for the next session. One new key
  in the existing flat config, no Settings row (the QR view is the only place
  to change it). Default for a new config and for an unreadable value:
  **Wi-Fi** (lenient read, like the other config keys).
- Tapping the segment already chosen does nothing. Tapping the other one saves
  the choice, stops the running session the way Back does, and starts a new
  session on the other network at once: the view shows "Mencari jaringan…" and
  then the new QR or a warning card. The owner agreed that phones that joined
  lose the session: no confirm step. Everything the stop already does happens
  (the shared shelf files and a shared note are unshared with the old
  session; the new session shares the shelf again at `Ready`, as today).
- On a warning card there is no running session: a tap saves the choice and
  starts a session (the same path as Coba lagi), nothing to stop.
- **Builder's call:** while an upload is running the switch is dimmed and does
  nothing, so a switch never cuts a transfer.
- **Builder's call (placement):** in the **header row**, in place of the
  title text "Sambungkan HP" / "Connect a phone", on the QR view and on the
  warning card; the back button and the chip stay where they are. Reason: the
  body of the QR view is full (the 150 px QR column holds the two steps, the
  network box and the hint with no spare height), while the header row has the
  height and the title is the least needed text once the QR is there. The
  receiving view keeps its title. The control must fit between the back button
  and the chip in both fonts (DM Sans, JetBrains Mono), both styles and both
  languages (extend `every_phone_string_holds_its_box_in_both_fonts`), and the
  owner sees it in the demo scenes before merge. If it cannot fit there, stop
  and ask; do not shrink the QR.
- **Where the view reads the mode:** the QR view and its network line, hint and
  footer use the mode the running session started with (carried in `Ready`);
  the switch's lit segment, the "Mencari jaringan…" state and the warning card
  use the saved config value. Right after a tap the two agree, because the tap
  saves first and the old session is stopped.

### Tailscale mode

- **Finding the address.** From the existing adapter list
  (`net::read_adapters`): an adapter that is up, whose name or description
  contains "tailscale" (case-insensitive; Windows names it "Tailscale", the
  description is "Tailscale Tunnel"), and that has an IPv4 address in
  `100.64.0.0/10`. No default gateway is required (the Wi-Fi rule needs one,
  this one must not). The first such address is used. This is a new pure
  function next to `pick_adapter`, tested on adapter lists like the existing
  tests. `pick_adapter` itself is unchanged: the Wi-Fi mode still skips
  Tailscale and other VPN adapters.
- **Not found** (Tailscale not installed, off, or signed out so the adapter has
  no 100.x address): a new refusal and a warning card **"Tailscale belum
  jalan"** / **"Tailscale isn't running"**, body **"Nyalakan Tailscale di PC ini
  dan login, lalu coba lagi."** / **"Turn on Tailscale on this PC and sign in,
  then try again."**, with the existing **Coba lagi** button and the switch.
  It is a new refusal next to `NoLan` and a new warning kind with its own key
  pair; the exhaustive matches on `Refusal` and `Warn` (and their tests) grow
  by one arm. "Ports in use" and "could not start" are shared by both modes
  unchanged.
- **Listening.** Bound to that 100.x address only, same port range
  (47390-47399) and the same port refusal.
- **Who may connect.** A peer passes only if its address is in `100.64.0.0/10`
  or `fd7a:115c:a1e0::/48` (an IPv4-mapped IPv6 address is read as its IPv4).
  In Wi-Fi mode today's rule stays (`net::is_private`; it already refuses
  100.64/10). The server's peer check gets the mode, so each mode refuses the
  other's peers even if routing ever delivered one. Loopback stays a debug and
  test option only.
- **Network line and hints on the QR view.** The network name is
  **"Tailscale"** and the address is the 100.x one, as the Wi-Fi line shows its
  own today. No "publik" label and no Public Wi-Fi warning in this mode: the
  traffic is encrypted end to end. The hint box reads **"HP juga harus pakai
  Tailscale, akun yang sama"** / **"The phone needs Tailscale too, same
  account"**. The footer reads **"Lewat Tailscale, terenkripsi dari HP ke PC"**
  / **"Through Tailscale, encrypted from phone to PC"** instead of "Langsung
  lewat Wi-Fi lokal, tidak lewat internet".
- **Session end.** Unchanged rules. Tailscale turning off or signing out takes
  the 100.x address off the adapter, so `BoundCheck` ends the session as
  `NetworkChanged`, the same way a Wi-Fi drop does today. `BoundCheck` is used
  as it is (it only asks whether the address is still on an up adapter). If
  some Tailscale state leaves the address on the adapter while nothing routes
  (signed out but the address kept), the session simply ends by the idle rule;
  that is accepted and goes on the manual check list.
- **Network category and the Public flag.** In Tailscale mode the session is
  never flagged Public. Vale does not read or change the Tailscale adapter's
  network category.

### Wi-Fi mode

Today's behaviour, unchanged: `pick_adapter`, `is_private`, the Public warning,
the firewall hint, the footer.

## Security

- Tailscale mode is at least as tight as Wi-Fi mode: only devices in the
  owner's tailnet can reach the address at all, and they still need the
  5-minute token from the QR to get a session cookie. Someone on the same café
  Wi-Fi cannot reach the listener. Plain HTTP is acceptable inside the tailnet
  because WireGuard encrypts it from the phone to the PC; Tailscale's DERP
  relays, used when no direct path exists, only forward encrypted packets.
- Residual risk, same as today: anyone in the tailnet who sees the QR while it
  is on screen can join, send files and download the shelf. A tailnet with
  shared-in devices from other people widens "anyone in the tailnet"; the README
  says so.

## Firewall

Vale changes nothing. Windows decides per network category whether the
incoming connection on the Tailscale adapter is allowed. Which category Windows
gives the Tailscale adapter is **not verified**; it goes on the manual check
list. The README firewall note covers both modes: if the phone cannot connect,
allow Vale for that network's category in Windows Firewall.

## Power and size

- No new timer, thread or poll. The adapter is read on the session thread at
  start, as the Wi-Fi mode does. Switching only stops and starts a session.
- No new crate and no new `windows` feature. Measure the exe and add the entry
  to `docs/memory/exe-size.md`.

## Docs

- **README privacy**, rewritten: the phone link has two modes. Wi-Fi: local
  network only, nothing goes over the internet, plain HTTP on your Wi-Fi.
  Tailscale: it goes through your own Tailscale network (you install and sign in
  to Tailscale yourself), encrypted end to end by WireGuard, and may pass
  through Tailscale's relay servers when the phone and the PC cannot reach each
  other directly; Vale itself sends nothing to Tailscale and only listens on
  the address Tailscale gave the PC. Keep the existing lines on who can join and
  what is visible. The firewall note covers both modes.
- **README feature table** (File shelf row) and `docs/guide.md`: mention the
  switch and that Tailscale must be on both devices.
- **Memory:** a "Tailscale mode" section in `docs/memory/shelf-phone.md` (the
  adapter rule, the peer ranges, the config key, the switch rules, what is not
  verified live) and its index line.

## Demo scenes

Add `phone-qr-tailscale` (QR view in Tailscale mode, switch on Tailscale, the
100.x line, the Tailscale hint and footer) and `phone-notailscale` (the
"Tailscale isn't running" card with the switch). The existing `phone-qr` shows
the switch on Wi-Fi.

## Tests the change needs

- The Tailscale adapter rule: a Tailscale adapter with a 100.x address and no
  gateway is picked; down, no 100.x address, a 100.x address on an adapter not
  named Tailscale, and a plain Wi-Fi adapter are not.
- The tailnet peer ranges (edges of `100.64.0.0/10`, `fd7a:115c:a1e0::/48`, a
  mapped address) and that each mode refuses the other's peers.
- The config key: default, round trip, unknown value read as Wi-Fi.
- The switch rules in `app_phone.rs`: same segment does nothing, the other one
  saves and restarts, dimmed and inert during an upload.
- The new strings in the fit test.

## Manual checks (owner, on real devices)

- iPhone and Android on mobile data with Tailscale on: scan, send a photo, take
  a shelf file.
- Tailscale off on the PC: the card shows; turn it on, Coba lagi works.
- Tailscale turned off mid-session: the session ends. Signed out
  mid-session: does the 100.x address leave the adapter (session ends at once)
  or stay (session ends by the idle rule)?
- Windows Firewall on the Tailscale adapter: does the first connection get
  through, and which network category does Windows show for it.
- Switching back and forth on the QR view, and the switch's look in both fonts
  and both themes.

## Left to the builder

The names of the config key, the refusal, the new keys and functions; the
exact layout of the switch within the rule above; how the server receives the
mode (a field in `ServerConfig` replacing or next to `allow_loopback`).
