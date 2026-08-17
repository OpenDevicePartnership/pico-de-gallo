# Changelog

All notable changes to the Pico de Gallo Zephyr module will be documented in
this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Breaking Changes

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
  peripheral controller drivers. The shield ships one disabled `pdg0` node;
  I2C and SPI controllers remain independent root siblings for now. At most
  one enabled parent may omit `serial-number`; multiple enabled parents
  without it are rejected at build time.

- Added validation against the firmware-reported GPIO count and explicit
  mappings for all #104 statuses: `-71` to `-EINVAL`, `-72` to `-EACCES`,
  `-73` to `-EBUSY`, `-74` to `-ENODEV`, and `-75` to `-ETIMEDOUT`. The
  mapping uses `switch ((enum Status)status)` with no `default:` label inside
  the switch and is enforced by `-Werror=switch`. Full `cs-gpios` support was
  rejected because it would split one atomic `spi/batch` into three USB
  round-trips and stop holding chip-select atomically. Closes #104.
