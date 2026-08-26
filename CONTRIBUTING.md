# Contributing to Open Device Partnership

The Open Device Partnership project welcomes your suggestions and contributions! Before opening your first issue or pull request, please review our
[Code of Conduct](CODE_OF_CONDUCT.md) to understand how our community interacts in an inclusive and respectful manner.

## Contribution Licensing

Most of our code is distributed under the terms of the [MIT license](LICENSE), and when you contribute code that you wrote to our repositories,
you agree that you are contributing under those same terms. In addition, by submitting your contributions you are indicating that
you have the right to submit those contributions under those terms.

## Other Contribution Information

If you wish to contribute code or documentation authored by others, or using the terms of any other license, please indicate that clearly in your
pull request so that the project team can discuss the situation with you.

## Commit Message

We use [Conventional Commits](https://www.conventionalcommits.org/)
with a crate scope for readable history, scoping, and to guide the
hand-written per-crate CHANGELOGs at release time (see
[`.github/RELEASE.md`](.github/RELEASE.md)).

Format:

```
<type>(<scope>)<!>: <subject>

<body>

<footer>
```

- **Type:** `feat`, `fix`, `chore`, `docs`, `refactor`, `perf`, `test`,
  `build`, `ci`, `revert`. Use `!` (or a `BREAKING CHANGE:` footer) for
  breaking changes.
- **Scope:** the crate name, without the `pico-de-gallo-` prefix —
  one of `internal`, `lib`, `hal`, `ffi`, `application`, `mcp`,
  `pyco`, `firmware`. Non-crate subprojects have their own scope:
  `zephyr` for the Zephyr module under `zephyr/`. For repo-wide
  changes use `repo`. For changes that span several crates, list
  them comma-separated, e.g. `feat(internal, firmware): …`.
- **Subject:** imperative mood, no trailing period (Tim Pope's
  [classic guide](http://tbaggery.com/2008/04/19/a-note-about-git-commit-messages.html)
  still applies for body wording).

Examples:

```
feat(internal): add device/info endpoint with capability bitfield
fix(firmware): pin embassy-usb-driver to "=0.2.0"
feat(application)!: rename --baud to --baud-rate
chore(ci): re-enable cargo-semver-checks on internal
```

### Wire-protocol changes

Any commit that changes wire types in `pico-de-gallo-internal`
**must**:

- be marked breaking (`feat(internal)!:` or `BREAKING CHANGE:` footer),
- be paired with a corresponding `feat(firmware)!:` commit so that
  firmware bumps in lockstep with the schema version,
- and ideally be accompanied by version bumps on every downstream
  crate (`lib`, `hal`, `ffi`, `application`, `pyco`) in the same PR
  so the wire-coupled crates release together.

## PR Etiquette

* Create a draft PR first
* Make sure that your branch has `.github` folder and all the code linting/sanity check workflows are passing in your draft PR before sending it out to code reviewers.

## Clean Commit History

We disabled squashing of commit and would like to maintain a clean commit history. So please reorganize your commits with the following items:

* Each commit builds successfully without warning
* Miscellaneous commits to fix typos + formatting are squashed

## Regressions

When reporting a regression, please ensure that you use `git bisect` to find the first offending commit, as that will help us finding the culprit a lot faster.
