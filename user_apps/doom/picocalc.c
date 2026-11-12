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

#include "doomgeneric.h"
#include "doomkeys.h"
#include "i_video.h"
#include "i_system.h"
#include "m_argv.h"

#define HEAP_BYTES (3 * 1024 * 1024)
#define SYS_CALL_COUNT 18
#define SYS_ALLOC 0
#define SYS_DEALLOC 1
#define SYS_PRINT 2
#define SYS_SLEEP_MS 3
#define SYS_GET_MS 4
#define SYS_GET_KEY 6
#define SYS_READ_FILE 9
#define SYS_FILE_LEN 11
#define SYS_FILL_RECT 15
#define SYS_BLIT 16
#define SYS_GET_LAST_ERROR 17
#define WAD_PATH "/doom1.wad"
#define FILE_COUNT 4

struct kernel_layout { size_t size, alignment; };
struct kernel_key { uint8_t key, state, mods; };

__attribute__((section(".syscall_table"), used))
uintptr_t SYS_CALL_TABLE[SYS_CALL_COUNT];

_Static_assert(offsetof(struct kernel_key, state) == 1, "KeyEventC state ABI");
_Static_assert(offsetof(struct kernel_key, mods) == 2, "KeyEventC mods ABI");
_Static_assert(sizeof(struct kernel_key) == 3, "KeyEventC size ABI");
_Static_assert(DOOMGENERIC_RESX == 320 && DOOMGENERIC_RESY == 200, "panel size");

static jmp_buf exit_context;
static unsigned char *heap_base;
static size_t heap_used;
static struct { int active; size_t offset, length; } files[FILE_COUNT];
pixel_t *DG_ScreenBuffer;

/* Status of the last status-reporting syscall; 0 is success. */
static int last_error(void)
{
    return ((int (*)(void))SYS_CALL_TABLE[SYS_GET_LAST_ERROR])();
}

static size_t wad_length(void)
{
    return ((size_t (*)(const uint8_t *, size_t))SYS_CALL_TABLE[SYS_FILE_LEN])(
        (const uint8_t *)WAD_PATH, sizeof(WAD_PATH) - 1);
}

static size_t wad_read(size_t offset, void *buffer, size_t length)
{
    size_t count = ((size_t (*)(const uint8_t *, size_t, size_t, uint8_t *, size_t))
        SYS_CALL_TABLE[SYS_READ_FILE])((const uint8_t *)WAD_PATH, sizeof(WAD_PATH) - 1,
                                       offset, buffer, length);
    return last_error() ? 0 : count;
}

void *sbrk(ptrdiff_t increment)
{
    if (!heap_base || increment < 0 || (size_t)increment > HEAP_BYTES - heap_used) {
        errno = ENOMEM;
        return (void *)-1;
    }
    void *previous = heap_base + heap_used;
    heap_used += (size_t)increment;
    return previous;
}

/* ponytail: one read-only IWAD; add saves after checked kernel write/rename support. */
int open(const char *path, int flags, ...)
{
    if ((flags & O_ACCMODE) != O_RDONLY || (flags & (O_CREAT | O_TRUNC | O_APPEND))) {
        errno = EROFS;
        return -1;
    }
    if (strcmp(path, WAD_PATH) != 0) {
        errno = ENOENT;
        return -1;
    }
    size_t length = wad_length();
    if (!length || length > INT_MAX) {
        errno = length ? EFBIG : ENOENT;
        return -1;
    }
    for (int index = 0; index < FILE_COUNT; ++index) {
        if (!files[index].active) {
            files[index].active = 1;
            files[index].offset = 0;
            files[index].length = length;
            return index + 3;
        }
    }
    errno = EMFILE;
    return -1;
}

static int file_index(int descriptor)
{
    if (descriptor < 3 || descriptor >= FILE_COUNT + 3 || !files[descriptor - 3].active) {
        errno = EBADF;
        return -1;
    }
    return descriptor - 3;
}

int close(int descriptor)
{
    int index = file_index(descriptor);
    if (index < 0) return -1;
    files[index].active = 0;
    return 0;
}

ssize_t read(int descriptor, void *buffer, size_t length)
{
    int index = file_index(descriptor);
    if (index < 0) return -1;
    if (!length || files[index].offset >= files[index].length) return 0;
    size_t available = files[index].length - files[index].offset;
    if (length > available) length = available;
    size_t count = wad_read(files[index].offset, buffer, length);
    if (!count || count > length) {
        errno = EIO;
        return -1;
    }
    files[index].offset += count;
    return (ssize_t)count;
}

off_t lseek(int descriptor, off_t offset, int whence)
{
    int index = file_index(descriptor);
    if (index < 0) return -1;
    int64_t origin;
    switch (whence) {
        case SEEK_SET: origin = 0; break;
        case SEEK_CUR: origin = files[index].offset; break;
        case SEEK_END: origin = files[index].length; break;
        default: errno = EINVAL; return -1;
    }
    if (offset < -origin || offset > INT_MAX - origin) {
        errno = EINVAL;
        return -1;
    }
    files[index].offset = (size_t)(origin + offset);
    return (off_t)files[index].offset;
}

int64_t lseek64(int descriptor, int64_t offset, int whence)
{
    if (offset < INT_MIN || offset > INT_MAX) { errno = EINVAL; return -1; }
    return lseek(descriptor, (off_t)offset, whence);
}

ssize_t write(int descriptor, const void *buffer, size_t length)
{
    if (descriptor != 1 && descriptor != 2) {
        errno = EROFS;
        return -1;
    }
    if (length > INT_MAX) { errno = EOVERFLOW; return -1; }
    ((void (*)(const uint8_t *, size_t))SYS_CALL_TABLE[SYS_PRINT])(buffer, length);
    return (ssize_t)length;
}

int fstat(int descriptor, struct stat *status)
{
    if (!status) { errno = EFAULT; return -1; }
    memset(status, 0, sizeof(*status));
    if (descriptor == 1 || descriptor == 2) {
        status->st_mode = S_IFCHR;
        return 0;
    }
    int index = file_index(descriptor);
    if (index < 0) return -1;
    status->st_mode = S_IFREG | S_IRUSR;
    status->st_size = (off_t)files[index].length;
    return 0;
}

int isatty(int descriptor) { return descriptor == 1 || descriptor == 2; }
char *getenv(const char *name) { (void)name; return NULL; }
int mkdir(const char *path, mode_t mode) { (void)path; (void)mode; errno = EROFS; return -1; }
int unlink(const char *path) { (void)path; errno = EROFS; return -1; }
int rename(const char *old_path, const char *new_path)
{
    (void)old_path; (void)new_path; errno = EROFS; return -1;
}
int system(const char *command) { (void)command; errno = ENOSYS; return -1; }

void exit(int status) { longjmp(exit_context, status ? status : 1); }
void _exit(int status) { exit(status); }

void DG_Init(void)
{
    ((void (*)(uint16_t, uint16_t, uint16_t, uint16_t, uint16_t))SYS_CALL_TABLE[SYS_FILL_RECT])(
        0, 0, 320, 320, 0);
}

void DG_DrawFrame(void)
{
    uint16_t palette[256];
    uint16_t rows[320 * 4];
    for (size_t index = 0; index < 256; ++index) {
        palette[index] = ((colors[index].r >> 3) << 11)
                       | ((colors[index].g >> 2) << 5) | (colors[index].b >> 3);
    }
    for (size_t row = 0; row < 200; row += 4) {
        for (size_t index = 0; index < 320 * 4; ++index)
            rows[index] = palette[DG_ScreenBuffer[row * 320 + index]];
        ((void (*)(uint16_t, uint16_t, uint16_t, uint16_t, const uint16_t *, size_t))
            SYS_CALL_TABLE[SYS_BLIT])(0, (uint16_t)(row + 60), 320, 4, rows, 320 * 4);
    }
}

uint32_t DG_GetTicksMs(void)
{
    return (uint32_t)((uint64_t (*)(void))SYS_CALL_TABLE[SYS_GET_MS])();
}

void DG_SleepMs(uint32_t milliseconds)
{
    ((void (*)(uint64_t))SYS_CALL_TABLE[SYS_SLEEP_MS])(milliseconds);
}

static unsigned char doom_key(unsigned char key)
{
    if (key >= 'A' && key <= 'Z') key += 'a' - 'A';
    if (key >= 0x81 && key <= 0x89) return KEY_F1 + key - 0x81;
    switch (key) {
        case 0x01: case 0xb5: case 'w': return KEY_UPARROW;
        case 0x02: case 0xb6: case 's': return KEY_DOWNARROW;
        case 0x03: case 0xb4: case 'a': return KEY_LEFTARROW;
        case 0x04: case 0xb7: case 'd': return KEY_RIGHTARROW;
        case 0x05: case 0x0a: case 0x0d: return KEY_ENTER;
        case 0x08: return KEY_BACKSPACE;
        case 'm': return KEY_ESCAPE;
        case 0x90: return KEY_F10;
        case 0x06: case 0xa5: case 'z': return KEY_FIRE;
        case 0x07: case ' ': return KEY_USE;
        case 0x11: case 'q': case ',': return KEY_STRAFE_L;
        case 0x12: case 'e': case '.': return KEY_STRAFE_R;
        case 0xa1: return KEY_RALT;
        case 0xa2: case 0xa3: return KEY_RSHIFT;
        default: return key < 0x80 ? key : 0;
    }
}

int DG_GetKey(int *pressed, unsigned char *key)
{
    for (;;) {
        struct kernel_key event = { 0, 0, 0 };
        ((void (*)(struct kernel_key *))SYS_CALL_TABLE[SYS_GET_KEY])(&event);
        if (event.state == 0) return 0;
        if (event.state != 1 && event.state != 3) continue;
        if (event.key == 0xb1 || event.key == 0x1b || event.key == 0xd0) {
            if (event.state == 1) I_Quit();
            continue;
        }
        *key = doom_key(event.key);
        if (!*key) continue;
        *pressed = event.state == 1;
        return 1;
    }
}

void DG_SetWindowTitle(const char *title) { (void)title; }

static uint32_t little_u32(const unsigned char *bytes)
{
    return (uint32_t)bytes[0] | (uint32_t)bytes[1] << 8
         | (uint32_t)bytes[2] << 16 | (uint32_t)bytes[3] << 24;
}

static int valid_wad(void)
{
    unsigned char header[12], entries[512];
    size_t length = wad_length();
    if (length < sizeof(header) || length > INT_MAX
        || wad_read(0, header, sizeof(header)) != sizeof(header)
        || memcmp(header, "IWAD", 4) != 0) return 0;
    size_t count = little_u32(header + 4), directory = little_u32(header + 8);
    if (!count || directory > length || count > (length - directory) / 16) return 0;
    for (size_t index = 0; index < count; index += 32) {
        size_t batch = count - index < 32 ? count - index : 32;
        if (wad_read(directory + index * 16, entries, batch * 16) != batch * 16) return 0;
        for (size_t entry = 0; entry < batch; ++entry) {
            size_t start = little_u32(entries + entry * 16);
            size_t size = little_u32(entries + entry * 16 + 4);
            if (start > length || size > length - start) return 0;
        }
    }
    return 1;
}

void D_DoomMain(void);

void _start(void)
{
    if (!valid_wad()) {
        static const char message[] = "Doom: missing or invalid /doom1.wad\n";
        write(2, message, sizeof(message) - 1);
        return;
    }
    struct kernel_layout layout = { HEAP_BYTES, 8 };
    heap_base = ((void *(*)(struct kernel_layout))SYS_CALL_TABLE[SYS_ALLOC])(layout);
    if (!heap_base) {
        static const char message[] = "Doom: cannot allocate 3 MiB heap\n";
        write(2, message, sizeof(message) - 1);
        return;
    }
    if (setjmp(exit_context) == 0) {
        char *arguments[] = { "doom", "-iwad", WAD_PATH, "-nomusic", "-mb", "2" };
        myargc = sizeof(arguments) / sizeof(arguments[0]);
        myargv = arguments;
        DG_Init();
        D_DoomMain();
        for (;;) doomgeneric_Tick();
    }
    ((void (*)(void *, struct kernel_layout))SYS_CALL_TABLE[SYS_DEALLOC])(heap_base, layout);
    heap_base = NULL;
}
