# Crate Map

Pico de Gallo is split into small crates on purpose. You can start at the
surface that matches your language, and if you need to go deeper, the layers
stack cleanly.

At a high level:

- `pico-de-gallo-internal` defines the **wire protocol** shared by host code and
  firmware.
- `pico-de-gallo-lib` is the **async Rust client** that speaks that protocol over
  USB with `tokio` + `nusb`.
- `pico-de-gallo-hal` turns that client into
  `embedded-hal` / `embedded-hal-async` traits.
- `pico-de-gallo-ffi` exports a stable **C ABI** as a `cdylib`.
- `pyco-de-gallo` exposes the same device to **Python** via PyO3 + maturin.
- `gallo` is the **CLI** built on top of the Rust library.
- `gallo-mcp` is an **MCP server** exposing the device to AI agents over stdio.
- `pico-de-gallo-firmware` is the **RP2350 no_std firmware** running on the board.

> [!NOTE]
> `pico-de-gallo-firmware` lives in a **separate Cargo workspace** from the host
> crates. That split is deliberate: firmware targets `thumbv8m.main-none-eabihf`
> and shares only the wire types with the host side.

## Dependency Direction

```text
                         You interact here

+---------------------+   +--------------------+   +-------------------+
| gallo               |   | pyco-de-gallo      |   | pico-de-gallo-ffi |
| CLI                 |   | Python bindings    |   | C ABI / cdylib    |
+----------+----------+   +----------+---------+   +---------+---------+
           \                         |                       /
            \                        |                      /
             +-----------------------+---------------------+
                                     |
                                     v
                       +-------------------------------+
                       | pico-de-gallo-lib             |
                       | async host client             |
                       | tokio + nusb + postcard-rpc   |
                       +---------------+---------------+
                                       |
                    +------------------+------------------+
                    |                                     |
                    v                                     v
      +-------------------------------+    +-------------------------------+
      | pico-de-gallo-hal             |    | pico-de-gallo-internal        |
      | embedded-hal bridge           |    | wire protocol types           |
      +-------------------------------+    +---------------+---------------+
                                                           |
                                                           v
                                      +-----------------------------------+
                                      | pico-de-gallo-firmware            |
                                      | RP2350 no_std firmware            |
                                      +-----------------------------------+
```

The important line is the one between `pico-de-gallo-internal` and firmware:
that crate is the contract. If request or response types change there, the host
and firmware must move together.

## What Each Crate Is For

| Crate | What it gives you | Typical use |
|---|---|---|
| `pico-de-gallo-internal` | Shared protocol types, endpoint definitions, schema version | Building host or firmware layers on top of the wire protocol |
| `pico-de-gallo-lib` | Typed async Rust API | Writing Rust host tools and applications |
| `pico-de-gallo-hal` | `embedded-hal` and `embedded-hal-async` traits backed by USB | Running driver crates on your laptop without reflashing firmware |
| `pico-de-gallo-ffi` | C-compatible shared library and generated header | Using Pico de Gallo from C, C++, Zig, or other FFI-friendly languages |
| `pyco-de-gallo` | Python module `pyco_de_gallo` | Quick experiments, test scripts, notebooks, lab automation |
| `gallo` | Command-line utility | Interactive bring-up, one-off reads/writes, smoke tests, scripting |
| `gallo-mcp` | MCP server (stdio) exposing one tool per peripheral op | Letting AI coding agents probe and drive real hardware |
| `pico-de-gallo-firmware` | Device-side implementation for the RP2350 | Flashing the board, adding endpoints, changing hardware behavior |

## Which crate do I want?

| If you want to... | Start here |
|---|---|
| Probe hardware from a shell | [`gallo`](./app.md) |
| Let an AI agent drive hardware | [`gallo-mcp`](./mcp.md) |
| Write a Rust host tool | [`pico-de-gallo-lib`](./lib.md) |
| Test an `embedded-hal` driver on your laptop | [`pico-de-gallo-hal`](./hal.md) |
| Call Pico de Gallo from C or C++ | [`pico-de-gallo-ffi`](./ffi.md) |
| Script from Python | [`pyco-de-gallo`](./python.md) |
| Change the protocol itself | `pico-de-gallo-internal` |
| Change what runs on the board | `pico-de-gallo-firmware` |

If you are not sure, start with `gallo` for exploration, move to
`pico-de-gallo-lib` for Rust applications, and reach for `pico-de-gallo-hal`
when you want real driver code to run unchanged on the host.
