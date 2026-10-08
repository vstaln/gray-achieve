<p align="center">
  <img src="assets/gray-logo.svg" alt="gray" width="96">
</p>
<h1 align="center">gray-achieve</h1>
<p align="center">Achievement badges that unlock from the live event stream as you work.</p>
<p align="center">
  <a href="https://github.com/vstaln/gray-achieve/blob/main/LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <img alt="gray plugin" src="https://img.shields.io/badge/gray-plugin-7aa2f7.svg">
  <img alt="rust" src="https://img.shields.io/badge/built%20with-rust-orange.svg">
</p>

Achievement badges unlocked from the live event stream — the sidecar
watches `pre_tool`, `post_tool` and `turn_end` notifications in real time
and awards each badge the moment its condition fires.

A sidecar plugin for [gray](https://github.com/vstaln/gray), scaffolded by
[gray-account](https://github.com/vstaln/gray-account).

## Badges

| id | name | unlock |
|----|------|--------|
| `first_tool` | First Contact | any tool call |
| `tool_10` | Toolchain Ten | 10 tool calls (persistent counter) |
| `tool_100` | Centurion | 100 tool calls |
| `first_bash` | Shell Shocked | a `bash` call |
| `first_write` | Pen to Paper | a `write` / `edit` / `notebook_edit` call |
| `first_error_seen` | Red Text | a failed tool call |
| `night_owl` | Night Owl | `turn_end` between 00:00–06:00 local |
| `marathon` | Marathon | a single turn over 100k tokens |

Unlocks append `{id, name, ts}` to `~/.gray/achievements.json` and fire
`host/say "🏆 <name>"`. The `host.say` capability needs consent — without it
the say fails silently and the unlock still lands on disk:

```sh
gray plugin capabilities achieve --all
```

The tool-call counter persists in `~/.gray/achieve/state.json`.

## Usage

```text
/achievements    # unlocked (with UTC timestamps) and locked badges
```

## Install

```sh
gray plugin install achieve
```

## Wire methods

- `plugin/manifest` — declares `/achievements` + `host.say` capability
- `event/notify` — `pre_tool` / `post_tool` / `turn_end` notifications
- `command/run` — `/achievements` listing
- `host/say` — unlock announcements (capability `host.say`, degrades silently)
- `plugin/shutdown`

## Develop

```sh
cargo test
gray account check      # entry point + manifest handshake
gray account publish    # check → build → release → publish to the gray registry
```

Bump `version` in `Cargo.toml` before each `publish`; the registry refuses to
republish a version.

---
Part of the [gray](https://github.com/vstaln/gray) plugin ecosystem —
the open-source AI agent harness. <https://gray.alignment.id>
