<div align="center">

# Close App

**A small Linux process killer for the stubborn ones.**

[![CI](https://github.com/SevenProxy/close_app/actions/workflows/ci.yml/badge.svg)](https://github.com/SevenProxy/close_app/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/Rust-2024-orange.svg)
![Renderer](https://img.shields.io/badge/render-Vulkan-5b4be1.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)

</div>

---

Vesktop under Hyprland has a habit of surviving `Win+Q`: the window goes away, the process doesn't. This is the two-click fix — search for `vesktop`, tick the row, send SIGTERM.

The UI is egui on wgpu with the backend pinned to Vulkan. There is no GL fallback, so a broken Vulkan setup fails loudly at startup instead of quietly rendering through something else.

```
┌──────────────────────────────────────────────────────┐
│  Gerenciador de processos                   Vulkan   │
│  Buscar: [ vesktop            ]  [Atualizar] [Auto]  │
├──────────────────────────────────────────────────────┤
│  PID    Nome       Comando                Memória    │
│  4217   vesktop    /usr/bin/vesktop       184.2 MiB  │
│  4231   vesktop    /usr/bin/vesktop --gpu    61 MiB  │
├──────────────────────────────────────────────────────┤
│  2 selecionado(s)      [Encerrar] [Forçar]           │
└──────────────────────────────────────────────────────┘
```

## Requirements

- Rust with 2024 edition support
- A working Vulkan driver; Mesa's `lvp` is enough if you have no real GPU
- Wayland or X11 — both are compiled into the binary

## Build

```sh
cargo build --release
./target/release/close_app
```

`cargo run` is fine for poking around. No environment tweaks are needed: the Vulkan backend is set in code, in `src/main.rs`.

## Usage

The search box matches PID, process name and command line. Spaces split terms and all of them have to match, so `vesktop electron` narrows things down. The list refreshes every two seconds unless you turn off **Auto**.

Tick the processes you care about, then pick an action at the bottom:

| Button | What it does |
| --- | --- |
| **Encerrar (SIGTERM)** | Asks nicely. If the PID is still alive two seconds later, SIGKILL goes out automatically. |
| **Forçar (SIGKILL)** | Straight to the uncatchable signal. |

Both go through a confirmation dialog. Mis-clicking in a process list is annoying.

## Behaviour

- PID 0, PID 1 and the app itself are left alone.
- Signalling a process owned by another user fails with a message in the status bar instead of silently doing nothing.
- Selections survive refreshes while the PID still exists.
- Zombie processes count as already gone and never get signalled.

## Project layout

| File | Job |
| --- | --- |
| `src/main.rs` | Window setup and the Vulkan backend pin. |
| `src/processes.rs` | `/proc` enumeration through `sysinfo`, filtering, signalling. |
| `src/app.rs` | egui layout, selection state, delayed SIGKILL escalation. |
| `.github/workflows/ci.yml` | Formatting, Clippy, tests, release build and artifact upload. |

## License

[MIT](LICENSE)
