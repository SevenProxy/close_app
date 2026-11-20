# close_app

A small Linux GUI for finding processes and killing them by PID. I built it because Vesktop survives `Win+Q` under Hyprland — the window disappears, the process stays. Now I search for `vesktop`, tick the box, and send it a SIGTERM.

The UI is egui on wgpu, with the backend pinned to Vulkan. There is no GL fallback, so if your Vulkan setup is broken the app fails at startup instead of quietly rendering through something else.

## Build and run

You need a Rust toolchain with 2024 edition support and a working Vulkan driver (Mesa's `lvp` is enough if you have no real GPU).

```sh
cargo build --release
./target/release/close_app
```

Or just `cargo run` while hacking on it. Wayland and X11 are both enabled in the eframe build.

## Using it

Type in the search box — it matches PID, process name and command line. Spaces split terms, so `vesktop electron` narrows things down. Refresh happens every two seconds by default; the checkbox next to it turns that off.

Tick the processes you want, then use one of the buttons at the bottom:

- **Encerrar (SIGTERM)** — polite request. If the PID is still alive two seconds later, the app sends SIGKILL on its own.
- **Forçar (SIGKILL)** — straight to the uncatchable signal. There is a confirmation dialog for both, because mis-clicking in a process list is annoying.

Nothing happens to PID 0, PID 1, or the app itself. Anything else is fair game as long as your user is allowed to signal it; failures show up in the status bar instead of being swallowed.

## Layout

- `src/main.rs` — window setup, forces the wgpu backend to Vulkan.
- `src/processes.rs` — reads `/proc` through `sysinfo`, filtering and signal sending.
- `src/app.rs` — the egui layout, selection state, and the delayed SIGKILL escalation.

Selections survive the periodic refresh as long as the PID still exists. Zombies are treated as already gone.
