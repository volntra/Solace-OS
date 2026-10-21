/* fern-icons: Fern's adaptive icon engine, C API (de/icons/src/capi.rs).
 * docs/formats/icon.md describes the icons and the five modes.
 * Strings are UTF-8 and NUL-terminated. An engine may be shared between
 * threads. */
#ifndef FERN_ICONS_H
#define FERN_ICONS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FernIcons FernIcons;

enum {
    FERN_ICONS_DEFAULT = 0,
    FERN_ICONS_DARK = 1,
    FERN_ICONS_TINTED = 2,
    FERN_ICONS_WALLPAPER = 3,
    FERN_ICONS_CLEAR = 4,
};

enum {
    FERN_ICONS_OK = 0,
    FERN_ICONS_NOT_FOUND = 1,
    FERN_ICONS_BAD_ICON = 2,
    FERN_ICONS_BAD_REQUEST = -1,
};

/* A new engine searching `dirs` in order (the standard places when ndirs
 * is 0). cache_dir: NULL for no disk cache, "" for ~/.cache/fern/icons.
 * NULL on failure. */
FernIcons *fern_icons_new(const char *const *dirs, size_t ndirs, const char *cache_dir);
void fern_icons_free(FernIcons *engine);

/* Draws icon `id` (else the legacy file `legacy`, may be NULL) in `mode`
 * with `accent` (0xRRGGBB) at size x size pixels into `out`, which holds
 * size * size * 4 bytes of premultiplied RGBA. */
int fern_icons_render(const FernIcons *engine, const char *id, const char *legacy,
                      uint32_t mode, uint32_t accent, uint32_t size,
                      uint8_t *out, size_t out_len);

/* The accent (0xRRGGBB) a wallpaper (PNG) suggests. 0 on success. */
int fern_icons_wallpaper_accent(const char *path, uint32_t *out);

/* rgb (0xRRGGBB) at another HCT tone, 0 (black) to 100 (white). */
uint32_t fern_icons_tone(uint32_t rgb, double tone);

#ifdef __cplusplus
}
#endif

#endif
