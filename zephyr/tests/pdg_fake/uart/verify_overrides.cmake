# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT
#
# BEST-EFFORT post-link gate: prove no Pico de Gallo bottom-layer function in
# the linked fake executable resolved to the real WEAK production definition.
#
# Why this exists. The recording fakes hand the driver an opaque NON-POINTER
# token as `ctx`. A bottom function that the fakes fail to override still links
# cleanly, because pdg_uart_bottom.c and common.c define every one of them
# `__attribute__((weak))` -- the weak definition simply wins by default. That
# definition casts the token to `PicoDeGallo *` and dereferences it, so the
# failure mode is not a link error but a hardware-free test binary reaching the
# real FFI with a fabricated pointer, which is undefined behaviour and is NOT
# guaranteed to crash loudly. `-Wl,--no-undefined` is blind to this by
# construction: there is no undefined reference to complain about. A post-link
# `nm` check is the only mechanical proof.
#
# THE RULE IS THREE-WAY, NOT TWO-WAY. For each declared bottom symbol:
#
#   absent  -- SAFE. Nothing in this topology references the symbol, so the
#              linker discarded it entirely. The dangerous cast can never
#              happen because no call site exists. Demanding a strong
#              definition here is a FALSE POSITIVE: on the UART suite
#              pdg_common_bottom_close is genuinely absent, because nothing
#              calls it, and an earlier revision of this gate would have
#              failed the build on exactly that.
#   `T`     -- CORRECT. A strong text definition. The fake won the link, which
#              is what the fakes exist to do.
#   weak    -- THE BUG. `W`/`V`/`w`/`v` means no strong definition existed and
#              the real production definition survived, so the opaque token
#              WILL be dereferenced at run time.
#
# So the check is on DEFINITION BINDING, and only a demonstrably weak binding
# fails the build.
#
# ABSENCE IS ONLY SAFE WHEN SOME SYMBOL IS PRESENT. "Absent" means "the linker
# discarded an unreferenced function" only if the symbol table is intact; a
# stripped executable, or a broken nm invocation, makes EVERY symbol absent and
# would otherwise be reported as a cheerful pass having verified nothing. Two
# guards close that: a `no symbols` report from nm is fatal, and a run in which
# ZERO symbols classify strong is fatal.
#
# The required symbol list is DERIVED from the declaration headers rather than
# hand-maintained here. A second literal list would be exactly the thing that
# rots: a bottom function added to pdg_uart_bottom.h next year would be
# forgotten by the fake AND by the gate simultaneously, which is the defect this
# gate exists to catch.
#
# Invoked in script mode (-P) from a custom target that runs after the
# native-simulator link step. Inputs:
#   PDG_NM          nm executable (CMAKE_NM when Zephyr defined one, else `nm`)
#   PDG_EXECUTABLE  absolute path to the linked native_sim executable
#   PDG_HEADERS     ;-separated absolute paths to the declaration headers
#
# This gate is BEST EFFORT. It degrades to a warning -- never a build failure --
# whenever it cannot make a positive determination (no nm on PATH, no
# executable at the expected path, an unreadable header, no symbols derived).
#
# EVERY PATH THROUGH THIS SCRIPT PRINTS. A gate that silently does nothing is
# worse than no gate, because it manufactures false assurance; a green build
# must be positive evidence the gate RAN, not the absence of evidence. All
# reporting therefore uses plain `message(...)` (NOTICE level) rather than
# `message(STATUS ...)`: NOTICE is written to stderr and is not filtered out by
# a `--log-level=WARNING` or `CMAKE_MESSAGE_LOG_LEVEL` setting further up, so
# the summary line reaches twister's build.log unconditionally.

if(NOT DEFINED PDG_NM OR PDG_NM STREQUAL "")
  set(PDG_NM "nm")
endif()

#
# 1. Derive the required symbol set from the declaration headers.
#
# The pattern deliberately matches any `pdg_<area>_bottom_<name>(` token, not a
# fixed prefix list, so a future bottom area declared in one of these headers is
# picked up without editing this script. Matching a trailing `(` also catches
# occurrences inside a doc comment that names a function with empty parens;
# that is harmless, because such a comment can only name a function that really
# is part of the contract.
#
set(required_symbols "")
foreach(header IN LISTS PDG_HEADERS)
  if(NOT EXISTS "${header}")
    message(WARNING
      "verify_overrides: GATE MALFUNCTION, NOT A PASS. Declaration header "
      "'${header}' not found, so the weak-override gate cannot derive its "
      "symbol list and is being skipped. Nothing was verified: the 'fake "
      "token never reaches real FFI' property is an assumption on this "
      "build, not evidence.")
    return()
  endif()

  file(READ "${header}" header_text)
  string(REGEX MATCHALL "pdg_[a-z0-9]+_bottom_[a-z0-9_]+[ \t]*\\(" hits "${header_text}")
  foreach(hit IN LISTS hits)
    string(REGEX REPLACE "[ \t]*\\($" "" symbol "${hit}")
    list(APPEND required_symbols "${symbol}")
  endforeach()
endforeach()

list(REMOVE_DUPLICATES required_symbols)
list(SORT required_symbols)
list(LENGTH required_symbols required_count)

if(required_count EQUAL 0)
  message(WARNING
    "verify_overrides: GATE MALFUNCTION, NOT A PASS. Derived ZERO bottom "
    "symbols from the declaration headers, so the regex no longer matches "
    "the declaration style. The weak-override gate is being skipped rather "
    "than silently passing. Nothing was verified.")
  return()
endif()

#
# 2. Read the linked executable's strong/weak definitions.
#
if(NOT EXISTS "${PDG_EXECUTABLE}")
  message(WARNING
    "verify_overrides: GATE MALFUNCTION, NOT A PASS. '${PDG_EXECUTABLE}' "
    "does not exist when the gate ran, so nothing was verified. The gate is "
    "attached to a target that must run AFTER the native-simulator Makefile "
    "links the final executable; if that ordering has been broken, fix the "
    "attach point or remove the gate -- do not leave it reporting this "
    "warning on every build.")
  return()
endif()

execute_process(
  COMMAND ${PDG_NM} -g --defined-only "${PDG_EXECUTABLE}"
  OUTPUT_VARIABLE nm_output
  ERROR_VARIABLE nm_error
  RESULT_VARIABLE nm_result)

#
# A stripped executable is the one input that makes this gate lie. Every symbol
# would classify as `absent`, which this script treats as SAFE ("unreferenced,
# so the dangerous cast cannot happen") -- a conclusion that only holds when
# absence really is the linker having discarded an uncalled function. GNU nm
# announces this specific condition as `no symbols` (on stderr, usually with a
# non-zero exit), so detect it by name and fail loudly BEFORE the generic
# nm-unavailable degradation below swallows it as a warning.
#
string(FIND "${nm_error}${nm_output}" "no symbols" pdg_no_symbols_at)
if(NOT pdg_no_symbols_at EQUAL -1)
  message(FATAL_ERROR
    "verify_overrides: GATE MALFUNCTION, NOT A PASS. '${PDG_NM}' reports 'no "
    "symbols' for '${PDG_EXECUTABLE}', which means the executable is "
    "stripped. Nothing was verified: with an empty symbol table every bottom "
    "symbol would classify as 'absent', which this gate treats as safe, so a "
    "stripped binary turns the gate into a cheerful pass that proves nothing. "
    "Build the fake suite without stripping, or point the gate at the "
    "unstripped link output.")
endif()

if(NOT nm_result EQUAL 0)
  message(WARNING
    "verify_overrides: GATE MALFUNCTION, NOT A PASS. '${PDG_NM}' is "
    "unavailable or failed (exit ${nm_result}): ${nm_error}. The "
    "weak-override gate is being skipped and NOTHING was verified. A "
    "toolchain without nm must not break the build, so this warns and "
    "passes; the 'fake token never reaches real FFI' property is then an "
    "assumption on this build, not evidence.")
  return()
endif()

#
# 3. Classify each derived symbol as strong / absent / weak.
#
# `nm -g` lists only external symbols. A `T` is the strong text definition the
# fake is supposed to supply. `W` (weak symbol) and `V` (weak object), and
# their lowercase forms, are the weak production definition surviving the link
# -- the one outcome that is actually dangerous. No line at all means the
# linker discarded the symbol because nothing references it on this topology,
# which is safe: an uncalled function cannot dereference anything.
#
set(strong_symbols "")
set(absent_symbols "")
set(weak_symbols "")
set(odd_symbols "")

foreach(symbol IN LISTS required_symbols)
  set(found_type "")

  string(REGEX MATCHALL "[^\n]*[ \t]${symbol}\r?\n" lines "${nm_output}\n")
  foreach(line IN LISTS lines)
    string(REGEX MATCH "^[0-9a-fA-F]*[ \t]+([A-Za-z])[ \t]+${symbol}" _m "${line}")
    if(_m)
      set(found_type "${CMAKE_MATCH_1}")
    endif()
  endforeach()

  if(found_type STREQUAL "")
    list(APPEND absent_symbols "${symbol}")
  elseif(found_type STREQUAL "T")
    list(APPEND strong_symbols "${symbol}")
  elseif(found_type MATCHES "^[WVwv]$")
    list(APPEND weak_symbols "${symbol}")
  else()
    list(APPEND odd_symbols "${symbol} (${found_type})")
  endif()
endforeach()

list(LENGTH strong_symbols strong_count)
list(LENGTH absent_symbols absent_count)
list(LENGTH weak_symbols weak_count)
list(LENGTH odd_symbols odd_count)

#
# 4. Report. Unconditionally, on every run, pass or fail.
#
string(REPLACE ";" ", " strong_list "${strong_symbols}")
string(REPLACE ";" ", " absent_list "${absent_symbols}")
string(REPLACE ";" ", " weak_list "${weak_symbols}")
string(REPLACE ";" ", " odd_list "${odd_symbols}")

message(
  "verify_overrides: ${required_count} bottom symbol(s) checked in "
  "'${PDG_EXECUTABLE}': ${strong_count} strongly overridden, "
  "${absent_count} absent (unreferenced), ${weak_count} weak, "
  "${odd_count} unexpected binding.")

if(strong_count GREATER 0)
  message("verify_overrides:   strong (T, fake won) : ${strong_list}")
endif()
if(absent_count GREATER 0)
  message("verify_overrides:   absent (unreferenced): ${absent_list}")
endif()
if(weak_count GREATER 0)
  message("verify_overrides:   WEAK (production won): ${weak_list}")
endif()
if(odd_count GREATER 0)
  message("verify_overrides:   unexpected binding   : ${odd_list}")
endif()

if(strong_count EQUAL 0)
  message(FATAL_ERROR
    "verify_overrides: GATE MALFUNCTION, NOT A PASS. ZERO of the "
    "${required_count} bottom symbol(s) are strongly defined in "
    "'${PDG_EXECUTABLE}', so nothing was verified. The fake demonstrably "
    "provides five strong overrides on this topology, and 'absent' is only "
    "evidence of safety when it means the linker discarded an unreferenced "
    "function -- it means nothing at all when NO symbol is present. An "
    "all-absent result therefore indicates the gate is looking at the wrong "
    "artefact, at a stripped binary, or at the output of a broken '${PDG_NM}' "
    "invocation. Refusing to report a pass.")
endif()

if(weak_count GREATER 0)
  message(FATAL_ERROR
    "verify_overrides: ${weak_count} bottom symbol(s) are defined with a WEAK "
    "binding in '${PDG_EXECUTABLE}': ${weak_list}. The recording fake did not "
    "override them, so the WEAK PRODUCTION definition won the link. That "
    "definition casts the fake's opaque non-pointer token to 'PicoDeGallo *' "
    "and dereferences it, which sends this hardware-free test binary into the "
    "real FFI -- undefined behaviour that is not guaranteed to crash loudly. "
    "Add a strong definition of each listed symbol to the fake.")
endif()

if(odd_count GREATER 0)
  message(FATAL_ERROR
    "verify_overrides: ${odd_count} bottom symbol(s) have a definition "
    "binding this gate does not recognise: ${odd_list}. Expected either no "
    "definition at all (unreferenced, safe) or a strong 'T' (the fake won). "
    "Anything else has not been reasoned about; refusing to report a pass.")
endif()
