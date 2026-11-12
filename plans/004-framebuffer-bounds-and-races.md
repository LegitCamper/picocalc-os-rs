# Plan 004: Fix out-of-bounds and cross-core races in the drawing syscalls

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat b619dfa..HEAD -- kernel/src/syscalls.rs kernel/src/framebuffer.rs kernel/src/display.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/001-verification-baseline.md
- **Category**: bug
- **Planned at**: commit `b619dfa`

## Why this matters

This is a bare-metal Rust OS for the Clockwork PicoCalc (RP2350, dual-core
Cortex-M33, no MMU). The 320×320 RGB565 framebuffer is a `static mut` shared
between **both cores**: core0's `display_handler` task pushes it to the SPI
display, core1's user app writes into it through the `draw_iter` / `fill_rect`
/ `blit` syscalls. The coordination is a single `AtomicBool` (`FB_PAUSED`) that
does not actually exclude anything, and three of the drawing paths index the
buffer with arithmetic that can overflow or go out of bounds on
user-controlled coordinates. The result is the class of bug the README is
hunting: corrupted frames, hangs, and watchdog resets that are hard to
reproduce because they depend on core timing. Every finding below is reachable
from safe user code.

## Current state

**The shared framebuffer**

```rust
// kernel/src/display.rs:34
pub static mut FRAMEBUFFER: Option<AtomicFrameBuffer> = None;
```

Written by core0 in `display_handler` (`kernel/src/display.rs:86-120`) and by
core1 in the drawing syscalls (`kernel/src/syscalls.rs:96-146`). Both reach it
through `unsafe { FRAMEBUFFER.as_mut().unwrap() }`.

**1. `fill_rect` and `blit` never clip — out-of-bounds panic**

`kernel/src/syscalls.rs:111-146`:

```rust
const _: FillRect = fill_rect;
pub extern "C" fn fill_rect(x: u16, y: u16, w: u16, h: u16, color: u16) {
    let area = Rectangle::new(
        Point::new(x as i32, y as i32),
        Size::new(w as u32, h as u32),
    );
    let color: Rgb565 = RawU16::new(color).into();

    FB_PAUSED.store(true, Ordering::Release);
    unsafe {
        FRAMEBUFFER
            .as_mut()
            .unwrap()
            .fill_solid(&area, color)
            .unwrap()
    }
    FB_PAUSED.store(false, Ordering::Release);
}
```

`blit` (`:132-146`) is the same shape, plus
`slice::from_raw_parts(colors, len)` with no null check and no check that
`len` matches `w * h`.

These land in `AtomicFrameBuffer::fill_contiguous`
(`kernel/src/framebuffer.rs:362-403`), which *does* clip pixel writes via
`drawable_area.contains(p)` — but then calls:

```rust
// kernel/src/framebuffer.rs:398-400
if changed {
    self.mark_tiles_dirty(*area);
}
```

It marks the **unclipped** `area` dirty. And `mark_tiles_dirty`
(`:48-61`) has no clamping at all:

```rust
fn mark_tiles_dirty(&mut self, rect: Rectangle) {
    let tiles_x = SCREEN_WIDTH.div_ceil(TILE_SIZE);
    let start_tx = (rect.top_left.x as usize) / TILE_SIZE;
    let end_tx = ((rect.top_left.x + rect.size.width as i32 - 1) as usize) / TILE_SIZE;
    ...
            let tile_idx = ty * tiles_x + tx;
            self.dirty_tiles[tile_idx].store(true, Ordering::Release);
```

`dirty_tiles` is `[AtomicBool; 400]`. A user calling
`fill_rect(0, 0, 1000, 1000, 0)` indexes past it and panics the kernel. A
negative `top_left` (reachable from `draw_iter`, whose `CPixel` fields are
`i32` — `userlib_sys/src/lib.rs:109-114`) casts to a huge `usize` and does the
same. Also note `TILE_COUNT` uses `SCREEN_WIDTH` for both dimensions
(`kernel/src/framebuffer.rs:20-23`) — correct only because the screen is
square; `NUM_TILE_ROWS` is likewise `SCREEN_WIDTH / TILE_SIZE`.

**2. `fill_solid` builds a wrong-length iterator**

`kernel/src/framebuffer.rs:406-411`:

```rust
fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
    self.fill_contiguous(
        area,
        core::iter::repeat_n(color, (self.size().width * self.size().height) as usize),
    )
}
```

The iterator length is the **whole screen** (102 400) regardless of `area`.
`fill_contiguous` iterates `area_width * area_height` times, so for any area
smaller than the screen this over-allocates the iterator (harmless) but for an
area *larger* than the screen it silently runs out and `break`s mid-row,
leaving a partial fill. The correct length is `area.size.width *
area.size.height`.

**3. `FB_PAUSED` is not a lock**

`kernel/src/display.rs:103-113` on core0:

```rust
if !FB_PAUSED.load(Ordering::Acquire) {
    unsafe {
        FRAMEBUFFER
            .as_mut()
            .unwrap()
            .partial_draw(&mut display)
            .await
            .unwrap();
    }
}
```

Core1 sets `FB_PAUSED = true`, draws, sets it `false`. Core0 checks it *once*
and then runs an `async` SPI transfer that awaits — during which core1 can and
does mutate the same buffer. There is no mutual exclusion, only a hint. Two
`&mut AtomicFrameBuffer` aliases exist simultaneously across cores, which is
UB in addition to producing torn frames. The `unwrap()` on `partial_draw` also
means any display error resets the device.

**4. The dirty-tile run scan reads the wrong index**

`kernel/src/framebuffer.rs:260-298`:

```rust
if self.dirty_tiles[row_start_idx + col].swap(false, Ordering::Acquire) {
    let run_start = col;
    let mut scan_col = col;
    let mut run_len = 1;

    while scan_col + 1 < NUM_TILE_COLS
        && self.dirty_tiles[row_start_idx + col + 1].load(Ordering::Acquire)
        && run_len < MAX_BATCH_TILES
    {
        scan_col += 1;
        run_len += 1;
    }
```

The loop advances `scan_col` but keeps testing `col + 1` — a fixed index. If
tile `col+1` is dirty, the condition stays true until `run_len` hits
`MAX_BATCH_TILES` (40), so the run is extended over **clean** tiles; if it is
clean, runs never extend past 1 tile. Either way the tiles swept into the run
are never cleared (only the first one's flag was swapped to `false`), so they
are redrawn every frame. This is a correctness and a throughput bug. Note
`MAX_BATCH_TILES` is `(SCREEN_WIDTH / TILE_SIZE) * 2` = 40, which is *twice*
`NUM_TILE_COLS` (20), so `run_len < MAX_BATCH_TILES` never binds — the run
length must be capped by `NUM_TILE_COLS` instead, and
`batch_tile_buf` is sized for `MAX_BATCH_TILES` tiles so it cannot overflow.

**5. `should_full_draw` miscounts**

`kernel/src/framebuffer.rs:99-109`:

```rust
fn should_full_draw(&self) -> bool {
    let threshold_pixels = SIZE * 80 / 100;
    let mut dirty_pixels = 0;

    self.dirty_tiles.iter().any(|tile| {
        if tile.load(Ordering::Acquire) {
            dirty_pixels += TILE_SIZE * TILE_SIZE;
        }
        dirty_pixels >= threshold_pixels
    })
}
```

`any` short-circuits, which is fine, but the closure returns the accumulated
comparison on *every* element including clean ones — it works, but it is
fragile. Low severity; fix only if free.

**6. `set_pixels` is dead and has an off-by-one**

`kernel/src/framebuffer.rs:63-97` — `set_pixels` is never called (the struct
is `#[allow(dead_code)]` at `:31`). Its bounds check rejects `ex == width`
rather than `ex > width - 1` inconsistently and it is unreachable. Delete it
rather than fix it.

### Conventions to match

- Syscalls validate arguments and return quietly; they do not panic. Exemplar
  (after plan 002): the early-return + `#[cfg(feature = "defmt")]
  defmt::warn!` pattern.
- `// SAFETY:` comment above every `unsafe` block —
  `kernel/src/syscalls.rs:64-66` is the exemplar.
- Compile-time guards use `const _: () = assert!(...)` —
  `kernel/src/audio.rs:18`, `userlib_sys/src/lib.rs:16`.
- Pure logic that should be unit-tested goes in `userlib_sys` (the only
  host-testable crate — see `plans/001-verification-baseline.md`).

## Commands you will need

| Purpose       | Command                                                                 | Expected on success |
|---------------|-------------------------------------------------------------------------|---------------------|
| Host tests    | `just test`                                                             | exit 0, tests pass  |
| Kernel builds | `cargo build --bin kernel --release --no-default-features --features rp235xa` | exit 0        |
| Kernel + fps  | `cargo build --bin kernel --release --features rp235xa --features fps`  | exit 0              |
| Apps build    | `just userapps`                                                         | exit 0              |
| Full gate     | `just check`                                                            | exit 0              |

The `fps` feature gates a second code path through `append_tiles_to_batch`
(`kernel/src/framebuffer.rs:163-223`) — it must keep compiling.

## Scope

**In scope**:
- `kernel/src/syscalls.rs` — only `draw_iter`, `fill_rect`, `blit`
  (lines ~96-146)
- `kernel/src/framebuffer.rs`
- `kernel/src/display.rs` — only the `display_handler` guard (lines ~103-113)
- `userlib_sys/src/lib.rs` — only to add the clipping helper + host tests
  (step 1)

**Out of scope** (do NOT touch):
- The filesystem/audio syscalls in `kernel/src/syscalls.rs` — plan 002 owns
  them.
- `kernel/src/elf.rs` — plan 003 owns it.
- `userlib/src/lib.rs`'s `DrawTarget` impl (`fill_contiguous` chunking at
  lines 146-224) — the user-side batching is correct and changing it is a
  separate concern; the kernel must be safe regardless of what it sends.
- `SCREEN_WIDTH` / `SCREEN_HEIGHT` / `TILE_SIZE` values, and the
  `SyscallTable` enum order. No ABI change in this plan.
- Replacing the `static mut FRAMEBUFFER` with a different ownership model —
  that is a larger redesign; see plan 006.

## Git workflow

- Branch: `advisor/004-framebuffer-bounds-and-races`
- Commit per step; short imperative lowercase messages matching `git log`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Add a tested rectangle-clipping helper to `userlib_sys`

Add to `userlib_sys/src/lib.rs`:

```rust
/// Clips a rectangle given as `(x, y, w, h)` in signed screen coordinates to
/// a `width` × `height` screen, returning `(x, y, w, h)` with every component
/// in range, or `None` if the rectangle is empty or entirely off-screen.
pub fn clip_rect(
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if w == 0 || h == 0 {
        return None;
    }
    let x1 = (x as i64) + (w as i64);
    let y1 = (y as i64) + (h as i64);
    let cx0 = x.max(0) as i64;
    let cy0 = y.max(0) as i64;
    let cx1 = x1.min(width as i64);
    let cy1 = y1.min(height as i64);
    if cx1 <= cx0 || cy1 <= cy0 {
        return None;
    }
    Some((
        cx0 as u32,
        cy0 as u32,
        (cx1 - cx0) as u32,
        (cy1 - cy0) as u32,
    ))
}
```

`i64` intermediates are deliberate: `x + w` must not overflow for
`x = i32::MAX`.

Host tests in the `mod tests` block created by plan 001:

```rust
#[test]
fn clip_rect_passes_through_fully_visible_rect() {
    assert_eq!(clip_rect(10, 10, 20, 20, 320, 320), Some((10, 10, 20, 20)));
}

#[test]
fn clip_rect_clips_negative_origin() {
    assert_eq!(clip_rect(-5, -5, 20, 20, 320, 320), Some((0, 0, 15, 15)));
}

#[test]
fn clip_rect_clips_past_right_edge() {
    assert_eq!(clip_rect(310, 0, 20, 10, 320, 320), Some((310, 0, 10, 10)));
}

#[test]
fn clip_rect_rejects_offscreen_rect() {
    assert_eq!(clip_rect(400, 400, 10, 10, 320, 320), None);
}

#[test]
fn clip_rect_rejects_zero_size() {
    assert_eq!(clip_rect(0, 0, 0, 10, 320, 320), None);
}

#[test]
fn clip_rect_does_not_overflow_on_extreme_origin() {
    assert_eq!(clip_rect(i32::MAX, 0, 16, 16, 320, 320), None);
    assert_eq!(clip_rect(i32::MIN, 0, 16, 16, 320, 320), None);
}
```

**Verify**: `just test` → exit 0, 6 new tests pass.

### Step 2: Clamp `mark_tiles_dirty` — the actual panic site

In `kernel/src/framebuffer.rs:48-61`, clip the rect before deriving tile
indices, and derive the column count from the real screen dimensions:

```rust
const NUM_TILE_COLS: usize = SCREEN_WIDTH / TILE_SIZE;
const NUM_TILE_ROWS: usize = SCREEN_HEIGHT / TILE_SIZE;
const _: () = assert!(NUM_TILE_COLS * NUM_TILE_ROWS == TILE_COUNT);

fn mark_tiles_dirty(&mut self, rect: Rectangle) {
    let Some((x, y, w, h)) = userlib_sys::clip_rect(
        rect.top_left.x,
        rect.top_left.y,
        rect.size.width,
        rect.size.height,
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    ) else {
        return;
    };

    let start_tx = x as usize / TILE_SIZE;
    let end_tx = (x + w - 1) as usize / TILE_SIZE;
    let start_ty = y as usize / TILE_SIZE;
    let end_ty = (y + h - 1) as usize / TILE_SIZE;

    for ty in start_ty..=end_ty {
        for tx in start_tx..=end_tx {
            self.dirty_tiles[ty * NUM_TILE_COLS + tx].store(true, Ordering::Release);
        }
    }
}
```

Note `NUM_TILE_ROWS` currently uses `SCREEN_WIDTH` (`:21`); change it to
`SCREEN_HEIGHT` and add the `const _` assertion so a non-square screen fails
the build instead of corrupting memory. Also update `TILE_COUNT` (`:20`) to
use `SCREEN_HEIGHT` for the vertical term if it does not already.

**Verify**: `cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0, and the `const _` assertion holds.

### Step 3: Validate the three drawing syscalls at the boundary

In `kernel/src/syscalls.rs`:

- `draw_iter` (`:96-110`): return immediately if `cpixels.is_null() || len == 0`.
  Cap `len` against a sane maximum — the user-side buffer is 1024 `CPixel`s
  (`userlib/src/lib.rs:66-68`) but a hostile app can pass anything; reject
  `len > SCREEN_WIDTH * SCREEN_HEIGHT`.
- `fill_rect` (`:111-131`): clip with `userlib_sys::clip_rect` before building
  the `Rectangle`; return if `None`.
- `blit` (`:132-146`): return if `colors.is_null()`; reject
  `len != w as usize * h as usize` (a mismatch means the user's buffer and the
  rectangle disagree — `fill_contiguous` would read past the slice or
  under-fill); then clip. Because clipping changes the rectangle but the
  color buffer is laid out for the *unclipped* rectangle, pass the original
  `area` to `fill_contiguous` — it clips internally per-pixel
  (`kernel/src/framebuffer.rs:376-396`) and consumes the iterator correctly.
  Use the clip result only to decide whether to call at all.
- Replace the three `.unwrap()` calls on the `DrawTarget` results
  (`:104-106`, `:123-125`, `:142-144`) with `let _ =` — the error type is
  `()` and there is nothing to report.

Keep the `const _: DrawIter = draw_iter;`-style ABI guards and the
`FB_PAUSED` store/clear pairs exactly where they are.

**Verify**: `just userapps && cargo build --bin kernel --release --no-default-features --features rp235xa`
→ both exit 0.

### Step 4: Fix `fill_solid`'s iterator length and `fill_contiguous`'s dirty rect

`kernel/src/framebuffer.rs:406-411` — use the area's own size:

```rust
fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
    let count = (area.size.width as usize).saturating_mul(area.size.height as usize);
    self.fill_contiguous(area, core::iter::repeat_n(color, count))
}
```

`kernel/src/framebuffer.rs:398-400` — mark the **clipped** area dirty, not the
caller's:

```rust
if changed {
    self.mark_tiles_dirty(drawable_area);
}
```

`drawable_area` is already computed at `:366`. After step 2 this is belt and
braces, but passing the unclipped rect was the original defect and it should
not be relied on being clamped downstream.

**Verify**: `cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0.

### Step 5: Fix the dirty-tile run scan

In `kernel/src/framebuffer.rs:260-298`, test the tile the scan has actually
reached, clear each tile it consumes, and cap the run at the row width:

```rust
while col < NUM_TILE_COLS {
    if self.dirty_tiles[row_start_idx + col].swap(false, Ordering::Acquire) {
        let run_start = col;
        let mut run_len = 1;

        // Extend the run over contiguous dirty tiles, clearing each as it
        // joins the batch, bounded by the row width and the batch buffer.
        while run_start + run_len < NUM_TILE_COLS
            && run_len < MAX_BATCH_TILES
            && self.dirty_tiles[row_start_idx + run_start + run_len]
                .swap(false, Ordering::Acquire)
        {
            run_len += 1;
        }

        self.append_tiles_to_batch(run_start as u16, tile_row as u16, run_len as u16);

        let start_x = run_start * TILE_SIZE;
        let end_x = start_x + run_len * TILE_SIZE - 1;
        let start_y = tile_row * TILE_SIZE;
        let end_y = start_y + TILE_SIZE - 1;

        display
            .set_pixels_buffered(
                start_x as u16,
                start_y as u16,
                end_x as u16,
                end_y as u16,
                &self.batch_tile_buf[..run_len * TILE_SIZE * TILE_SIZE],
            )
            .await?;

        col = run_start + run_len;
    } else {
        col += 1;
    }
}
```

`append_tiles_to_batch` already `debug_assert!`s `total_tiles <=
NUM_TILE_COLS` (`:162`); the `run_start + run_len < NUM_TILE_COLS` bound
satisfies it. Keep that assertion.

**Verify**: `cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0. `cargo build --bin kernel --release --features rp235xa --features fps`
→ exit 0 (the `fps` path shares `append_tiles_to_batch`).

### Step 6: Close the core0/core1 framebuffer race

The minimal correct fix that does not redesign ownership: make core0 skip the
frame if core1 is drawing, and make core1's pause flag a counter so nested or
back-to-back syscalls cannot clear it early.

Replace `FB_PAUSED: AtomicBool` (`kernel/src/framebuffer.rs:29`) with:

```rust
/// Number of user-space draw calls currently touching `FRAMEBUFFER`.
/// Core0's `display_handler` skips a frame while this is non-zero, so a
/// partially-written frame is never pushed to the display.
pub static FB_WRITERS: AtomicU32 = AtomicU32::new(0);
```

In each of the three drawing syscalls, replace
`FB_PAUSED.store(true, Ordering::Release)` with
`FB_WRITERS.fetch_add(1, Ordering::Acquire)` and
`FB_PAUSED.store(false, ...)` with
`FB_WRITERS.fetch_sub(1, Ordering::Release)`.

In `kernel/src/display.rs:103-113`, check the counter and re-check after the
draw; and stop panicking on display errors:

```rust
if FB_WRITERS.load(Ordering::Acquire) == 0 {
    // SAFETY: `FB_WRITERS == 0` means no user-space draw call is in
    // flight, so core1 is not aliasing `FRAMEBUFFER`.
    unsafe {
        if let Some(fb) = FRAMEBUFFER.as_mut() {
            let _ = fb.partial_draw(&mut display).await;
        }
    }
}
```

This is still cooperative, not a hard lock — a user app that draws
continuously can starve the refresh, and a draw that starts mid-transfer is
not blocked. Say so in a comment and in the maintenance notes; the proper fix
is plan 006.

Keep the `fps` block above it (`kernel/src/display.rs:96-101`) unchanged.

**Verify**: `grep -c 'FB_PAUSED' kernel/src/` (recursive) → `0`.
`just check` → exit 0.

### Step 7: Delete the dead `set_pixels`

Remove `AtomicFrameBuffer::set_pixels` (`kernel/src/framebuffer.rs:63-97`). It
has no callers. If the build then warns that `#[allow(dead_code)]` on the
struct (`:31`) is unnecessary, remove that too.

**Verify**: `grep -c 'fn set_pixels' kernel/src/framebuffer.rs` → `0`.
`cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0 with no new warnings.

## Test plan

- New tests: the six `clip_rect` cases from step 1, in
  `userlib_sys/src/lib.rs`'s `mod tests` — fully visible, negative origin,
  past-right-edge, fully off-screen, zero size, and `i32::MAX`/`i32::MIN`
  overflow. These are the logic that prevents the `dirty_tiles` out-of-bounds
  index.
- The framebuffer itself cannot be unit-tested on the host (it needs
  `st7365p_lcd` and an SPI device). Its gate is: compiles with and without
  `fps`, plus the manual device check below.
- Model after the test block plan 001 creates in the same file.
- Verification: `just test` → exit 0, 6 new tests pass.

## Done criteria

ALL must hold:

- [ ] `just test` exits 0; the 6 `clip_rect` tests exist and pass
- [ ] `just check` exits 0
- [ ] `cargo build --bin kernel --release --features rp235xa --features fps`
      exits 0
- [ ] `grep -rc 'FB_PAUSED' kernel/src/` → 0
- [ ] `grep -c 'fn set_pixels' kernel/src/framebuffer.rs` → 0
- [ ] `mark_tiles_dirty` calls `clip_rect` and contains no unclamped
      `as usize` index arithmetic
- [ ] `draw_iter`, `fill_rect` and `blit` each reject null pointers, and
      `blit` rejects `len != w * h`
- [ ] The `const _: () = assert!(NUM_TILE_COLS * NUM_TILE_ROWS == TILE_COUNT)`
      guard exists
- [ ] `SYS_CALL_TABLE_COUNT` is still `17`
- [ ] `git status` shows only the four in-scope files modified
- [ ] `plans/README.md` status row updated

**Manual device check** (operator; record as pending if no hardware):
`just kernel-release rp235xa`, `just copy-userapps`. Launch the gallery app
(exercises `blit` via `fill_contiguous`) and the snake app (exercises
`fill_rect`). Expected: no tearing artifacts worse than before, no reset, and
frames update when only part of the screen changes.

## STOP conditions

- `userlib_sys` cannot be referenced from `kernel/src/framebuffer.rs` — it
  can (`kernel/Cargo.toml` already has `userlib_sys = { path = "../userlib_sys" }`),
  but if the import fails, report rather than duplicating `clip_rect`.
- The `const _` tile-count assertion fails: that means `SCREEN_WIDTH` and
  `SCREEN_HEIGHT` are no longer both 320 — stop and report, do not "fix" it by
  deleting the assertion.
- Step 5's rewrite changes what the display shows (e.g. whole screen redraws
  every frame). Report the observed behavior; do not revert silently.
- Any step appears to require changing `userlib/src/lib.rs` or a user app.

## Maintenance notes

- `FB_WRITERS` is a cooperative hint, not mutual exclusion: core0 can still
  begin a transfer immediately before core1 increments the counter. The real
  fix is to stop sharing `static mut FRAMEBUFFER` across cores — see
  plan 006 (ownership redesign). Do not add more logic on top of this
  counter; it is a stopgap.
- `blit`'s `len != w * h` rejection is a behavior change for user apps that
  previously passed a short buffer and got a partial fill. `userlib`'s
  `fill_contiguous` (`userlib/src/lib.rs:146-224`) always passes
  `rows_buffered * clip_w`, which satisfies the check — verified by reading
  the call sites at `:189-198` and `:215-223`.
- Reviewer should scrutinize: that `blit` still passes the *unclipped*
  rectangle to `fill_contiguous` (the color buffer is laid out for it), and
  that every `swap(false, ...)` in step 5 corresponds to a tile actually
  included in the batch.
- Deferred on purpose: `should_full_draw`'s fragile accumulator
  (`kernel/src/framebuffer.rs:99-109`), and the `TILE_COUNT` hardcoding of a
  square screen beyond the new assertion.
