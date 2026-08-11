/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Stores and tracks any PicoDeGallo instances, and tracks their usage
 * via reference counting. This is needed in case multiple boards
 * or multiple APIs are being used.
 */

#ifndef PDG_GALLO_REGISTRY_H
#define PDG_GALLO_REGISTRY_H

#include "pico_de_gallo.h"

#ifdef __cplusplus
extern "C" {
#endif

/*  opens a new pico de gallo board from a serial identifier.
*
*   if this is a totally new pico-de-gallo board, this function registers
*   that pico-de-gallo within the registry. if this pico-de-gallo board has
*   already been activated and is simply being opened by a new API/caller,
*   this function increments the board's reference count but does not re-initialize it.
*/
const PicoDeGallo *pdg_registry_open(const char *serial);

/*  closes a pico-de-gallo
*
*   first, searches for the pico-de-gallo in the registry.
*   if this gallo has rc == 1 (meaning this reference is the
*   last one standing), this function frees the gallo and removes
*   it from the registry. if this gallo has an rc greater than 1,
*   this funciton just decrements the rc.
*
*   if no matching gallo is found, this function prints out an error
*   log. it can't propogate an actual errno because of the rest of the
*   API, but it is impossible for no matching gallo to be found based
*   on the structure of this file and `common/` in general
*/
void pdg_registry_close(const PicoDeGallo *gallo);

#ifdef __cplusplus
}
#endif

#endif /* PDG_GALLO_REGISTRY_H */