#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <setjmp.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include "i_system.h"
#include "d_event.h"

void I_GetEvent(void);

#define open port_open
#define close port_close
#define read port_read
#define write port_write
#define lseek port_lseek
#define lseek64 port_lseek64
#define fstat port_fstat
#define isatty port_isatty
#define sbrk port_sbrk
#define getenv port_getenv
#define mkdir port_mkdir
#define unlink port_unlink
#define rename port_rename
#define system port_system
#define exit port_exit
#define _exit port__exit
#define _start port_start
void port_exit(int status) __attribute__((noreturn));
#include "picocalc.c"

int myargc;
char **myargv;
struct color colors[256];
static int quit_from_keyboard;
void D_DoomMain(void)
{
    assert(myargc == 6 && strcmp(myargv[3], "-nomusic") == 0);
    if (quit_from_keyboard) {
        int pressed;
        unsigned char key;
        DG_GetKey(&pressed, &key);
        assert(!"quit key returned to engine");
    }
    I_Quit();
    assert(!"I_Quit returned after engine cleanup");
}
void doomgeneric_Tick(void) { assert(0); }

static unsigned char wad[28] = { 'I', 'W', 'A', 'D', 1, 0, 0, 0, 12 };
static size_t mock_size = sizeof(wad);
static int read_error;
static int allocation_error;
static struct kernel_key events[5];
static size_t event_index, frame_rows, printed, allocations, deallocations;
static uint64_t slept;
static int engine_cleaned;
static unsigned char arena[HEAP_BYTES];

static void engine_cleanup(void) { ++engine_cleaned; }

static void *mock_alloc(struct kernel_layout layout)
{
    assert(layout.size == HEAP_BYTES && layout.alignment == 8);
    ++allocations;
    return allocation_error ? NULL : arena;
}

static void mock_free(void *pointer, struct kernel_layout layout)
{
    assert(pointer == arena && layout.size == HEAP_BYTES && layout.alignment == 8);
    assert((size_t)engine_cleaned == deallocations + 1);
    ++deallocations;
}

static size_t mock_length(const uint8_t *path, size_t length)
{
    assert(length == sizeof(WAD_PATH) - 1 && memcmp(path, WAD_PATH, length) == 0);
    return mock_size;
}

static int reported_error;

static int mock_last_error(void) { return reported_error; }

static size_t mock_read(const uint8_t *path, size_t name_length, size_t offset,
                        uint8_t *buffer, size_t length)
{
    mock_length(path, name_length);
    reported_error = read_error ? 4 : 0;
    if (read_error) return length;
    assert(offset <= sizeof(wad) && length <= sizeof(wad) - offset);
    memcpy(buffer, wad + offset, length);
    return length;
}

static void mock_print(const uint8_t *buffer, size_t length)
{
    assert(buffer != NULL);
    printed += length;
}

static void mock_key(struct kernel_key *output)
{
    assert(output != NULL);
    assert(event_index < sizeof(events) / sizeof(events[0]));
    struct kernel_key event = events[event_index++];
    const uint8_t wire[] = { event.key, event.state, event.mods };
    memcpy(output, wire, sizeof(wire));
}
static uint64_t mock_time(void) { return UINT64_C(0x10000002a); }
static void mock_sleep(uint64_t milliseconds) { slept = milliseconds; }
static void mock_fill(uint16_t left, uint16_t top, uint16_t width, uint16_t height, uint16_t color)
{
    assert(left == 0 && top == 0 && width == 320 && height == 320 && color == 0);
}

static void mock_blit(uint16_t left, uint16_t top, uint16_t width, uint16_t height,
                      const uint16_t *pixels, size_t length)
{
    static const uint16_t expected[] = {
        0x0000, 0xf800, 0x07e0, 0x001f, 0xffff, 0xffe0, 0xf81f, 0x07ff,
    };
    assert(left == 0 && top == frame_rows + 60 && width == 320 && height == 4);
    assert(length == 1280);
    for (size_t index = 0; index < length; ++index)
        assert(pixels[index] == expected[index % 8]);
    frame_rows += height;
}

int main(void)
{
    SYS_CALL_TABLE[0] = (uintptr_t)mock_alloc;
    SYS_CALL_TABLE[1] = (uintptr_t)mock_free;
    SYS_CALL_TABLE[2] = (uintptr_t)mock_print;
    SYS_CALL_TABLE[3] = (uintptr_t)mock_sleep;
    SYS_CALL_TABLE[4] = (uintptr_t)mock_time;
    SYS_CALL_TABLE[6] = (uintptr_t)mock_key;
    SYS_CALL_TABLE[9] = (uintptr_t)mock_read;
    SYS_CALL_TABLE[11] = (uintptr_t)mock_length;
    SYS_CALL_TABLE[15] = (uintptr_t)mock_fill;
    SYS_CALL_TABLE[16] = (uintptr_t)mock_blit;
    SYS_CALL_TABLE[17] = (uintptr_t)mock_last_error;

    assert(valid_wad());
    wad[0] = 'X'; assert(!valid_wad()); wad[0] = 'I';
    wad[4] = 2; assert(!valid_wad()); wad[4] = 1;
    wad[8] = 255; assert(!valid_wad()); wad[8] = 12;
    wad[12] = 255; assert(!valid_wad()); wad[12] = 0;
    wad[16] = 255; assert(!valid_wad()); wad[16] = 0;
    read_error = 1; assert(!valid_wad()); read_error = 0;
    mock_size = 0; port_start(); assert(printed && allocations == 0); mock_size = sizeof(wad);

    int descriptor = open(WAD_PATH, O_RDONLY);
    assert(descriptor == 3);
    unsigned char buffer[64];
    assert(read(descriptor, buffer, 4) == 4 && memcmp(buffer, "IWAD", 4) == 0);
    assert(lseek(descriptor, -1, SEEK_CUR) == 3);
    assert(lseek(descriptor, -1, SEEK_SET) == -1 && errno == EINVAL);
    assert(lseek64(descriptor, INT64_MAX, SEEK_SET) == -1);
    assert(lseek64(descriptor, INT64_MIN, SEEK_SET) == -1);
    assert(lseek(descriptor, -4, SEEK_END) == sizeof(wad) - 4);
    assert(read(descriptor, buffer, sizeof(buffer)) == 4);
    assert(read(descriptor, buffer, sizeof(buffer)) == 0);
    assert(lseek(descriptor, 100, SEEK_SET) == 100 && read(descriptor, buffer, 1) == 0);
    assert(lseek(descriptor, 0, SEEK_SET) == 0);
    read_error = 1; assert(read(descriptor, buffer, 1) == -1 && errno == EIO); read_error = 0;
    struct stat status;
    assert(fstat(descriptor, &status) == 0 && status.st_size == sizeof(wad));
    assert(open(WAD_PATH, O_WRONLY) == -1 && errno == EROFS);
    assert(open(WAD_PATH, O_RDONLY | O_TRUNC) == -1 && errno == EROFS);
    assert(open("/default.cfg", O_RDONLY) == -1 && errno == ENOENT);
    assert(write(descriptor, "no", 2) == -1 && errno == EROFS);
    assert(rename(WAD_PATH, "/other.wad") == -1 && errno == EROFS);
    assert(unlink(WAD_PATH) == -1 && errno == EROFS);
    for (int index = 1; index < FILE_COUNT; ++index) assert(open(WAD_PATH, O_RDONLY) == index + 3);
    assert(open(WAD_PATH, O_RDONLY) == -1 && errno == EMFILE);
    for (int index = 0; index < FILE_COUNT; ++index) assert(close(index + 3) == 0);
    assert(read(descriptor, buffer, 1) == -1 && errno == EBADF);
    assert(close(descriptor) == -1 && errno == EBADF);

    heap_base = arena;
    assert(sbrk(16) == arena);
    assert(sbrk(-1) == (void *)-1 && errno == ENOMEM);
    assert(sbrk(HEAP_BYTES) == (void *)-1 && errno == ENOMEM);
    assert(sbrk(HEAP_BYTES - 16) == arena + 16);
    assert(sbrk(1) == (void *)-1 && errno == ENOMEM);
    heap_used = 0;

    static const unsigned char bindings[][2] = {
        {'w', KEY_UPARROW}, {'W', KEY_UPARROW}, {'s', KEY_DOWNARROW},
        {'a', KEY_LEFTARROW}, {'d', KEY_RIGHTARROW},
        {0x01, KEY_UPARROW}, {0x02, KEY_DOWNARROW},
        {0x03, KEY_LEFTARROW}, {0x04, KEY_RIGHTARROW},
        {0xb5, KEY_UPARROW}, {0xb6, KEY_DOWNARROW},
        {0xb4, KEY_LEFTARROW}, {0xb7, KEY_RIGHTARROW},
        {0x05, KEY_ENTER}, {0x0a, KEY_ENTER}, {0x0d, KEY_ENTER},
        {'Z', KEY_FIRE}, {0xa5, KEY_FIRE}, {0x06, KEY_FIRE},
        {' ', KEY_USE}, {0x07, KEY_USE}, {'q', KEY_STRAFE_L},
        {',', KEY_STRAFE_L}, {0x11, KEY_STRAFE_L}, {'e', KEY_STRAFE_R},
        {'.', KEY_STRAFE_R}, {0x12, KEY_STRAFE_R},
        {0xa1, KEY_RALT}, {0xa2, KEY_RSHIFT}, {0xa3, KEY_RSHIFT},
        {'m', KEY_ESCAPE}, {'M', KEY_ESCAPE}, {0x08, KEY_BACKSPACE},
        {0x09, KEY_TAB}, {'1', '1'}, {'7', '7'},
        {0x81, KEY_F1}, {0x90, KEY_F10},
    };
    int pressed;
    unsigned char key;
    for (size_t index = 0; index < sizeof(bindings) / sizeof(bindings[0]); ++index) {
        event_index = 0;
        events[0] = (struct kernel_key){ bindings[index][0], 2, 0 };
        events[1] = (struct kernel_key){ bindings[index][0], 1, 0 };
        events[2] = (struct kernel_key){ bindings[index][0], 3, 0 };
        assert(DG_GetKey(&pressed, &key) == 1 && pressed == 1 && key == bindings[index][1]);
        assert(DG_GetKey(&pressed, &key) == 1 && pressed == 0 && key == bindings[index][1]);
        assert(DG_GetKey(&pressed, &key) == 0);
    }
    assert(DG_GetTicksMs() == 42);
    event_index = 0;
    events[0] = (struct kernel_key){ 0x0a, 1, 0 };
    events[1] = (struct kernel_key){ 0x0a, 3, 0 };
    events[2] = (struct kernel_key){ 0xb5, 1, 0 };
    events[3] = (struct kernel_key){ 0xb5, 3, 0 };
    I_GetEvent();
    event_t *event = D_PopEvent();
    assert(event && event->type == ev_keydown && event->data1 == KEY_ENTER);
    event = D_PopEvent();
    assert(event && event->type == ev_keyup && event->data1 == KEY_ENTER);
    I_GetEvent();
    event = D_PopEvent();
    assert(event && event->type == ev_keydown && event->data1 == KEY_UPARROW);
    event = D_PopEvent();
    assert(event && event->type == ev_keyup && event->data1 == KEY_UPARROW);
    assert(D_PopEvent() == NULL);
    DG_SleepMs(17); assert(slept == 17);
    DG_Init();
    DG_ScreenBuffer = calloc(320 * 200, 1);
    assert(DG_ScreenBuffer);
    colors[1].r = 255;
    colors[2].g = 255;
    colors[3].b = 255;
    colors[4].r = colors[4].g = colors[4].b = 255;
    colors[5].r = colors[5].g = 255;
    colors[6].r = colors[6].b = 255;
    colors[7].g = colors[7].b = 255;
    for (size_t index = 0; index < 320 * 200; ++index) DG_ScreenBuffer[index] = index % 8;
    DG_DrawFrame(); assert(frame_rows == 200);
    free(DG_ScreenBuffer);
    allocation_error = 1;
    size_t previous_printed = printed;
    port_start();
    assert(allocations == 1 && deallocations == 0 && heap_base == NULL);
    assert(printed > previous_printed && engine_cleaned == 0);
    allocation_error = 0;
    I_AtExit(engine_cleanup, false);
    port_start();
    assert(allocations == 2 && deallocations == 1 && heap_base == NULL && engine_cleaned == 1);
    quit_from_keyboard = 1;
    static const unsigned char quit_keys[] = { 0xb1, 0x1b, 0xd0 };
    for (size_t index = 0; index < sizeof(quit_keys); ++index) {
        memset(events, 0, sizeof(events));
        events[0] = (struct kernel_key){ quit_keys[index], 3, 0 };
        event_index = 0;
        assert(DG_GetKey(&pressed, &key) == 0);
        events[0] = (struct kernel_key){ quit_keys[index], 1, 0 };
        event_index = 0;
        port_start();
        assert(allocations == index + 3 && deallocations == index + 2 && heap_base == NULL);
        assert((size_t)engine_cleaned == index + 2);
    }
    puts("Doom bridge: file I/O, WAD bounds, keys, RGB565, timing, heap bounds, exit cleanup pass");
}
