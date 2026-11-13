#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "i_sound.h"
#include "w_wad.h"
#include "z_zone.h"

#define SOUND_CHANNELS 8
#define SAMPLE_RATE 22050
#define AUDIO_FRAMES 1024
#define SYS_AUDIO_CONFIGURE 12
#define SYS_AUDIO_READY 13
#define SYS_AUDIO_SEND 14
#define SYS_GET_LAST_ERROR 17

extern uintptr_t SYS_CALL_TABLE[];

/* ponytail: eight PCM effect channels, no music; add a synth for MUS playback. */
static struct sound_channel {
    const uint8_t *samples;
    uint32_t length, step;
    uint64_t position;
    int lump, left, right;
} channels[SOUND_CHANNELS];
static bool enabled, prefix;

static int audio_error(void)
{
    return ((int (*)(void))SYS_CALL_TABLE[SYS_GET_LAST_ERROR])();
}

static void release_lump(int lump)
{
    for (size_t index = 0; index < SOUND_CHANNELS; ++index)
        if (channels[index].samples && channels[index].lump == lump) return;
    W_ReleaseLumpNum(lump);
}

static void stop_sound(int handle)
{
    if (handle < 0 || handle >= SOUND_CHANNELS || !channels[handle].samples) return;
    int lump = channels[handle].lump;
    channels[handle].samples = NULL;
    release_lump(lump);
}

static void shutdown_sound(void)
{
    enabled = false;
    for (int handle = 0; handle < SOUND_CHANNELS; ++handle) stop_sound(handle);
}

static boolean init_sound(boolean use_sfx_prefix)
{
    shutdown_sound();
    prefix = use_sfx_prefix;
    ((void (*)(uint32_t))SYS_CALL_TABLE[SYS_AUDIO_CONFIGURE])(SAMPLE_RATE);
    enabled = audio_error() == 0;
    if (!enabled) fprintf(stderr, "Doom: audio configuration failed\n");
    return enabled;
}

static int sound_lump(sfxinfo_t *sound)
{
    if (!sound) return -1;
    if (sound->link) sound = sound->link;
    char name[12];
    snprintf(name, sizeof(name), "%s%.8s", prefix ? "ds" : "", sound->name);
    return W_CheckNumForName(name);
}

static void update_params(int handle, int volume, int separation)
{
    if (handle < 0 || handle >= SOUND_CHANNELS) return;
    if (volume < 0) volume = 0;
    if (volume > 127) volume = 127;
    if (separation < 0) separation = 0;
    if (separation > 254) separation = 254;
    channels[handle].left = volume * (254 - separation) / 254;
    channels[handle].right = volume * separation / 254;
}

static int start_sound(sfxinfo_t *sound, int handle, int volume, int separation)
{
    if (!enabled || !sound || handle < 0 || handle >= SOUND_CHANNELS) return -1;
    stop_sound(handle);
    int lump = sound->lumpnum;
    if (lump < 0 || (unsigned int)lump >= numlumps) return -1;
    int size = W_LumpLength(lump);
    if (size < 8) return -1;
    const uint8_t *data = W_CacheLumpNum(lump, PU_STATIC);
    uint32_t rate = (uint32_t)data[2] | (uint32_t)data[3] << 8;
    uint32_t length = (uint32_t)data[4] | (uint32_t)data[5] << 8
                    | (uint32_t)data[6] << 16 | (uint32_t)data[7] << 24;
    if (data[0] != 3 || data[1] != 0 || rate == 0 || length <= 48
        || length > (uint32_t)size - 8) {
        release_lump(lump);
        return -1;
    }
    channels[handle] = (struct sound_channel){
        .samples = data + 24, .length = length - 32,
        .step = (uint32_t)(((uint64_t)rate << 16) / SAMPLE_RATE), .lump = lump,
    };
    update_params(handle, volume, separation);
    return handle;
}

static boolean sound_playing(int handle)
{
    return handle >= 0 && handle < SOUND_CHANNELS && channels[handle].samples != NULL;
}

static uint8_t pcm_sample(int mixed)
{
    int sample = 128 + mixed / 127;
    return sample < 0 ? 0 : sample > 255 ? 255 : (uint8_t)sample;
}

static void update_sound(void)
{
    if (!enabled || !((bool (*)(void))SYS_CALL_TABLE[SYS_AUDIO_READY])()) return;
    uint8_t output[AUDIO_FRAMES * 2];
    uint64_t positions[SOUND_CHANNELS];
    for (size_t handle = 0; handle < SOUND_CHANNELS; ++handle)
        positions[handle] = channels[handle].position;
    for (size_t frame = 0; frame < AUDIO_FRAMES; ++frame) {
        int left = 0, right = 0;
        for (size_t handle = 0; handle < SOUND_CHANNELS; ++handle) {
            const struct sound_channel *channel = &channels[handle];
            if (!channel->samples || (positions[handle] >> 16) >= channel->length) continue;
            int sample = (int)channel->samples[positions[handle] >> 16] - 128;
            left += sample * channel->left;
            right += sample * channel->right;
            positions[handle] += channel->step;
        }
        output[frame * 2] = pcm_sample(left);
        output[frame * 2 + 1] = pcm_sample(right);
    }
    ((void (*)(const uint8_t *, size_t))SYS_CALL_TABLE[SYS_AUDIO_SEND])(output, sizeof(output));
    if (audio_error()) return;
    for (int handle = 0; handle < SOUND_CHANNELS; ++handle) {
        channels[handle].position = positions[handle];
        if ((positions[handle] >> 16) >= channels[handle].length) stop_sound(handle);
    }
}

static snddevice_t devices[] = { SNDDEVICE_SB };
sound_module_t DG_sound_module = {
    .sound_devices = devices,
    .num_sound_devices = 1,
    .Init = init_sound,
    .Shutdown = shutdown_sound,
    .GetSfxLumpNum = sound_lump,
    .Update = update_sound,
    .UpdateSoundParams = update_params,
    .StartSound = start_sound,
    .StopSound = stop_sound,
    .SoundIsPlaying = sound_playing,
};
