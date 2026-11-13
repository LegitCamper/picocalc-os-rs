#include <assert.h>
#include <stdio.h>
#include "sound.c"

uintptr_t SYS_CALL_TABLE[18];
unsigned int numlumps = 2;
boolean screensaver_mode;
static uint8_t lumps[2][8192], packet[AUDIO_FRAMES * 2];
static int sizes[2], releases[2], reported_error, configure_error, send_error;
static bool pinned[2], busy;
static size_t sent;
static char requested_name[12];

int M_CheckParm(const char *name) { return strcmp(name, "-nomusic") == 0; }
int W_CheckNumForName(char *name)
{
    snprintf(requested_name, sizeof(requested_name), "%s", name);
    return strcmp(name, "dspistol") == 0 ? 0 : -1;
}
int W_LumpLength(unsigned int lump) { assert(lump < numlumps); return sizes[lump]; }
void *W_CacheLumpNum(int lump, int tag)
{
    assert(lump >= 0 && (unsigned int)lump < numlumps && tag == PU_STATIC);
    pinned[lump] = true;
    return lumps[lump];
}
void W_ReleaseLumpNum(int lump)
{
    assert(lump >= 0 && (unsigned int)lump < numlumps && pinned[lump]);
    pinned[lump] = false;
    ++releases[lump];
}
static void configure(uint32_t rate)
{
    assert(rate == SAMPLE_RATE);
    reported_error = configure_error;
}
static bool ready(void) { return !busy; }
static int status(void) { return reported_error; }
static void submit(const uint8_t *data, size_t length)
{
    assert(length == sizeof(packet));
    reported_error = send_error;
    if (!send_error) { memcpy(packet, data, length); ++sent; }
}
static void make_lump(int lump, uint32_t rate, uint32_t count, uint8_t value)
{
    assert(count + 40 <= sizeof(lumps[lump]));
    uint32_t length = count + 32;
    memset(lumps[lump], 0, sizeof(lumps[lump]));
    lumps[lump][0] = 3;
    lumps[lump][2] = rate;
    lumps[lump][3] = rate >> 8;
    for (size_t index = 0; index < 4; ++index) lumps[lump][4 + index] = length >> (8 * index);
    memset(lumps[lump] + 24, value, count);
    sizes[lump] = count + 40;
}
static void expect_stereo(uint8_t left, uint8_t right)
{
    for (size_t frame = 0; frame < AUDIO_FRAMES; ++frame) {
        assert(packet[frame * 2] == left);
        assert(packet[frame * 2 + 1] == right);
    }
}

int main(void)
{
    SYS_CALL_TABLE[12] = (uintptr_t)configure;
    SYS_CALL_TABLE[13] = (uintptr_t)ready;
    SYS_CALL_TABLE[14] = (uintptr_t)submit;
    SYS_CALL_TABLE[17] = (uintptr_t)status;
    sfxinfo_t sound = { .name = "pistol", .lumpnum = 0 };
    sfxinfo_t linked = { .name = "linked", .link = &sound };
    I_InitSound(true);
    assert(enabled);
    assert(I_GetSfxLumpNum(&sound) == 0);
    assert(I_GetSfxLumpNum(&linked) == 0 && strcmp(requested_name, "dspistol") == 0);

    make_lump(0, SAMPLE_RATE, 4096, 255);
    assert(I_StartSound(&sound, -1, 127, 0) == -1);
    assert(I_StartSound(&sound, SOUND_CHANNELS, 127, 0) == -1);
    assert(I_StartSound(&sound, 0, 127, 0) == 0);
    busy = true;
    I_UpdateSound();
    assert(sent == 0 && channels[0].position == 0);
    busy = false;
    send_error = 1;
    I_UpdateSound();
    assert(sent == 0 && channels[0].position == 0 && pinned[0]);
    send_error = 0;
    I_UpdateSound();
    expect_stereo(255, 128);
    assert(sent == 1 && channels[0].position == (uint64_t)AUDIO_FRAMES << 16);
    I_UpdateSoundParams(0, 127, 254);
    I_UpdateSound();
    expect_stereo(128, 255);
    assert(I_StartSound(&sound, 1, 127, 0) == 1);
    int previous = releases[0];
    I_StopSound(0);
    assert(pinned[0] && releases[0] == previous && I_SoundIsPlaying(1));
    I_StopSound(1);
    assert(!pinned[0] && releases[0] == previous + 1);

    make_lump(0, 11025, 64, 255);
    lumps[0][24] = 0;
    assert(I_StartSound(&sound, 0, 127, 0) == 0);
    I_UpdateSound();
    assert(packet[0] == 0 && packet[2] == 0 && packet[4] == 255 && packet[6] == 255);
    assert(packet[254] == 255 && packet[256] == 128);
    assert(!I_SoundIsPlaying(0) && !pinned[0]);

    make_lump(0, 44100, 4096, 255);
    lumps[0][24] = 0;
    lumps[0][25] = 128;
    assert(I_StartSound(&sound, 0, 127, 0) == 0);
    I_UpdateSound();
    assert(packet[0] == 0 && packet[2] == 255);
    assert(channels[0].position == (uint64_t)(AUDIO_FRAMES * 2) << 16);
    I_StopSound(0);

    for (int value = 0; value <= 255; value += 255) {
        make_lump(0, SAMPLE_RATE, 4096, value);
        for (int handle = 0; handle < SOUND_CHANNELS; ++handle)
            assert(I_StartSound(&sound, handle, 127, 0) == handle);
        I_UpdateSound();
        expect_stereo(value, 128);
        I_ShutdownSound();
        assert(!pinned[0]);
        I_InitSound(true);
    }
    make_lump(0, SAMPLE_RATE, 4096, 255);
    assert(I_StartSound(&sound, 0, -1, 999) == 0);
    I_UpdateSound();
    expect_stereo(128, 128);
    I_StopSound(0);

    for (int invalid = 0; invalid < 7; ++invalid) {
        make_lump(0, SAMPLE_RATE, 64, 255);
        sound.lumpnum = 0;
        switch (invalid) {
            case 0: sizes[0] = 7; break;
            case 1: lumps[0][0] = 0; break;
            case 2: lumps[0][2] = lumps[0][3] = 0; break;
            case 3: lumps[0][4] = 48; break;
            case 4: lumps[0][7] = 255; break;
            case 5: sound.lumpnum = -1; break;
            case 6: sound.lumpnum = numlumps; break;
        }
        assert(I_StartSound(&sound, 0, 127, 0) == -1);
        assert(!pinned[0] && !I_SoundIsPlaying(0));
    }
    sound.lumpnum = 0;
    I_UpdateSound();
    expect_stereo(128, 128);
    I_ShutdownSound();
    size_t before = sent;
    I_UpdateSound();
    assert(sent == before);
    configure_error = 1;
    I_InitSound(true);
    assert(!enabled && !I_SoundIsPlaying(0));
    puts("Doom sound: engine dispatch, PCM bounds, mixing, resampling, stereo, backpressure, cleanup pass");
}
