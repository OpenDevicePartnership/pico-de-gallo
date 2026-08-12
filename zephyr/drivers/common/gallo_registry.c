#include "gallo_registry.h"
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*  reference-counted tracker for the pico-de-gallo bridge
*   used so multiple APIs can use the same gallo.
*/
typedef struct {
    /* pointer to the gallo */
    const PicoDeGallo *gallo;
    /* owned selector; an empty string represents the default device */
    char *serial;
    /* number of active references to the gallo */
    size_t rc;
} Rc;

/* Registry node stored in a host-side linked list. */
typedef struct RcNode {
    Rc rc;
    struct RcNode *next;
} RcNode;

/* the mallocing and pthread use is fine here since this is all native-sim host layer. */

/* Dynamic list of `Rc` values serialized across native-sim host threads. */
static RcNode *list;
static pthread_mutex_t list_lock = PTHREAD_MUTEX_INITIALIZER;

static const char *normalize_serial(const char *serial)
{
    return serial == NULL || serial[0] == '\0' ? "" : serial;
}

/*  internal helper that searches through the list for an existing node
*   associated with the serial identifier. if one is found, this function
*   returns a pointer to that `Rc`. if no node matches the serial id, then
*   this function returns NULL.
*/
static Rc *list_lookup_serial(const char *serial)
{
    RcNode *entry;

    for (entry = list; entry != NULL; entry = entry->next) {
        if (strcmp(entry->rc.serial, serial) == 0) {
            return &entry->rc;
        }
    }

    return NULL;
}

/*  internal helper like `list_lookup_serial()`, but taking in a `PicoDeGallo*`. */
static Rc *list_lookup_gallo(const PicoDeGallo *gallo)
{
    RcNode *entry;

    for (entry = list; entry != NULL; entry = entry->next) {
        if (entry->rc.gallo == gallo) {
            return &entry->rc;
        }
    }

    return NULL;
}

/*  internal helper to add an Rc to the list
*
*   this function should only be called to register a new unique
*   board. the `serial` string is how pico-de-gallo boards can be uniquely
*   identified, so if you try to add an `Rc` with an identical serial
*   number to an `Rc` that is already tracked, this function will
*   throw an error
*/
static int list_register(const PicoDeGallo *gallo, const char *serial)
{
    RcNode *allocation;
    size_t serial_size;

    if (list_lookup_serial(serial) != NULL) {
        fprintf(stderr, "Pico de Gallo registry: selector is already registered\n");
        return -EINVAL;
    }

    allocation = malloc(sizeof(*allocation));
    if (allocation == NULL) {
        fprintf(stderr, "Pico de Gallo registry: failed to allocate registry node\n");
        return -ENOMEM;
    }

    serial_size = strlen(serial) + 1U;
    allocation->rc.serial = malloc(serial_size);
    if (allocation->rc.serial == NULL) {
        fprintf(stderr, "Pico de Gallo registry: failed to copy serial selector\n");
        free(allocation);
        return -ENOMEM;
    }

    memcpy(allocation->rc.serial, serial, serial_size);
    allocation->rc.gallo = gallo;
    allocation->rc.rc = 1U;
    allocation->next = list;
    list = allocation;

    return 0;
}

/*  internal helper to find a gallo and remove it
*
*   this doesn't do anything to the underlying gallo, it
*   just finds the matching node pointer in the list and removes
*   it plus frees its allocation
*/
static int list_remove(const PicoDeGallo *gallo)
{
    RcNode **link = &list;

    while (*link != NULL) {
        RcNode *entry = *link;

        if (entry->rc.gallo == gallo) {
            *link = entry->next;
            free(entry->rc.serial);
            free(entry);
            return 0;
        }

        link = &entry->next;
    }

    return -ENOENT;
}

/*  opens a new pico de gallo board from a serial identifier.
*
*   if this is a totally new pico-de-gallo board, this function registers
*   that pico-de-gallo within the registry. if this pico-de-gallo board has
*   already been activated and is simply being opened by a new API/caller,
*   this function increments the board's reference count but does not re-initialize it.
*/
const PicoDeGallo *pdg_registry_open(const char *serial)
{
    const char *selector = normalize_serial(serial);
    const PicoDeGallo *gallo;
    Rc *found;
    int ret;

    pthread_mutex_lock(&list_lock);

    found = list_lookup_serial(selector);
    if (found != NULL) {
        found->rc++;
        gallo = found->gallo;
        pthread_mutex_unlock(&list_lock);
        return gallo;
    }

    /* The FFI can't report which serial the default selector chose. Therefore we reject
     * mixing default and explicit selectors so one physical board cannot be
     * registered under two keys and claimed twice.
     */
    if ((selector[0] == '\0' && list != NULL) ||
        (selector[0] != '\0' && list_lookup_serial("") != NULL)) {
        fprintf(stderr,
                "Pico de Gallo registry: cannot mix default and explicit selectors. You must "
                "use the same explicit serial for every API\n");
        pthread_mutex_unlock(&list_lock);
        return NULL;
    }

    if (selector[0] == '\0') {
        gallo = gallo_init_strict();
    } else {
        gallo = gallo_init_strict_with_serial_number(selector);
    }

    if (gallo == NULL) {
        pthread_mutex_unlock(&list_lock);
        return NULL;
    }

    ret = list_register(gallo, selector);
    if (ret < 0) {
        gallo_free(gallo);
        pthread_mutex_unlock(&list_lock);
        return NULL;
    }

    pthread_mutex_unlock(&list_lock);
    return gallo;
}

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
void pdg_registry_close(const PicoDeGallo *gallo)
{
    Rc *found;
    int ret;

    if (gallo == NULL) {
        fprintf(stderr, "Pico de Gallo registry: attempted to close a NULL pointer\n");
        return;
    }

    pthread_mutex_lock(&list_lock);
    found = list_lookup_gallo(gallo);

    if (found == NULL) {
        fprintf(stderr, "Pico de Gallo registry: attempted to close an unregistered pointer\n");
        pthread_mutex_unlock(&list_lock);
        return;
    }

    if (found->rc > 1U) {
        found->rc--;
        pthread_mutex_unlock(&list_lock);
        return;
    }

    if (found->rc == 0U) {
        fprintf(stderr, "Pico de Gallo registry: registered pointer has a zero reference count\n");
        pthread_mutex_unlock(&list_lock);
        return;
    }

    ret = list_remove(gallo);
    if (ret < 0) {
        fprintf(stderr, "Pico de Gallo registry: failed to remove registered pointer: %d\n", ret);
        pthread_mutex_unlock(&list_lock);
        return;
    }

    gallo_free(gallo);
    pthread_mutex_unlock(&list_lock);
}