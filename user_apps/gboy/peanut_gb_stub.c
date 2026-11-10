#include <peanut_gb.h>

/* bindgen can't express the `direct` bitfields, so expose them here. */

void gb_set_joypad(struct gb_s *gb, uint8_t joypad)
{
	gb->direct.joypad = joypad;
}

uint8_t gb_get_joypad(const struct gb_s *gb)
{
	return gb->direct.joypad;
}

void gb_set_frame_skip(struct gb_s *gb, bool enable)
{
	gb->direct.frame_skip = enable;
}
