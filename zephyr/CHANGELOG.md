# Changelog

All notable changes to the Pico de Gallo Zephyr module will be documented in
this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Breaking Changes

- The `odp,pico-de-gallo-i2c` and `odp,pico-de-gallo-spi` controllers must
  now be **direct children of an enabled `odp,pico-de-gallo` parent**. A
  controller at the devicetree root, under a disabled parent, under an
  unrelated parent, or nested more than one level deep is rejected at build
  time with an explanatory static assertion rather than an unresolved device
  ordinal at link time.

- `serial-number` **moved from the controllers to the parent**. It is no
  longer declared on either controller binding, so a leftover child property
  fails devicetree processing with an undeclared-property error naming the
  node and the binding; it is not silently ignored. The parent's selection
  applies to all of its children. Omitting it remains safe only when exactly
  one matching board is physically attached: the host API cannot report which
  board it chose, and the build-time multi-parent check counts devicetree
  parents, not attached USB devices.

- The shield's controller nodes were renamed and reparented: `/pdg-i2c` and
  `/pdg-spi` are now `/pico-de-gallo/i2c` and `/pico-de-gallo/spi`. **Absolute
  devicetree node paths and the generated identifiers and dependency ordinals
  derived from them change accordingly.** The `pdg0`, `pdg_i2c0` and
  `pdg_spi0` labels are unchanged, so overlays that use `&pdg_i2c0` /
  `&pdg_spi0` continue to resolve; overlays or code referring to the absolute
  paths must be updated. Sample and application overlays must now also enable
  `&pdg0`.

- The parent is now the sole owner of the board's USB handle; the controllers
  borrow it and never close it. Happy-path I2C and SPI transfer behaviour is
  unchanged. Initialization ownership, failure coupling, failure location and
  worst-case boot latency change: one parent validation now gates both
  children. Physical USB opens were already deduplicated to one and remain
  one; registry calls and references drop from three to one. A parent open
  failure now fails both controllers coherently, at the parent's
  `POST_KERNEL/40` rather than the controllers' `POST_KERNEL/50`, and the
  worst case falls from as many as three independent five-minute strict opens
  to one attempt plus two fast child failures. The cost is that a controller
  can no longer initialize independently of the parent.

- An SPI child node's `reg` is now a selector into the controller's
  `cs-gpio-indices` array, not a firmware GPIO index. There is no identity
  fallback: a missing mapping or a selector beyond the array returns
  `-EINVAL` at runtime. The binding keeps the property optional so this
  error remains reachable. Duplicate firmware GPIO indices are deliberately
  permitted, matching upstream Zephyr's treatment of `cs-gpios` and allowing
  intentional shared-select wiring. If two peripherals mapped to one index
  are selected simultaneously, both may drive MISO; returned bytes represent
  contention, not data. Closes #104.

### Added

- Added the `odp,pico-de-gallo` devicetree compatible: a multi-function
  device parent node representing one physical USB-attached board. It owns
  the host connection for that board and exposes its opaque context to child
  peripheral controller drivers. The shield ships one disabled `pdg0` node,
  with the I2C and SPI controllers as its direct children; they borrow the
  parent's connection and carry no selector of their own. At most
  one enabled parent may omit `serial-number`; multiple enabled parents
  without it are rejected at build time.

- Added validation against the firmware-reported GPIO count and explicit
  mappings for all #104 statuses: `-71` to `-EINVAL`, `-72` to `-EACCES`,
  `-73` to `-EBUSY`, `-74` to `-ENODEV`, and `-75` to `-ETIMEDOUT`. The
  mapping uses `switch ((enum Status)status)` with no `default:` label inside
  the switch and is enforced by `-Werror=switch`. Full `cs-gpios` support was
  rejected because it would split one atomic `spi/batch` into three USB
  round-trips and stop holding chip-select atomically. Closes #104.
