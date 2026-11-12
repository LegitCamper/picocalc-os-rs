extern crate alloc;

use alloc::{string::String, vec::Vec};

// ponytail: root-only, at most 512 live objects; extend identity to paths for subdirectories.
pub const MAX_OBJECTS: usize = 512;
pub const MAX_NAME: usize = 64;

struct Object {
    handle: u32,
    name: String,
    seen: bool,
}

pub struct ObjectHandles {
    objects: Vec<Object>,
    next: u32,
}

impl ObjectHandles {
    pub const fn new() -> Self {
        Self {
            objects: Vec::new(),
            next: 1,
        }
    }

    pub fn clear(&mut self) {
        self.objects.clear();
    }

    pub fn begin_scan(&mut self) {
        for object in &mut self.objects {
            object.seen = false;
        }
    }

    pub fn observe(&mut self, name: &str) -> Result<u32, ()> {
        if name.is_empty() || name.len() > MAX_NAME {
            return Err(());
        }
        if let Some(object) = self
            .objects
            .iter_mut()
            .find(|object| object.name.eq_ignore_ascii_case(name))
        {
            object.seen = true;
            return Ok(object.handle);
        }
        if self.objects.len() == MAX_OBJECTS || self.next == u32::MAX {
            return Err(());
        }
        let mut stored = String::new();
        stored.try_reserve_exact(name.len()).map_err(|_| ())?;
        stored.push_str(name);
        self.objects.try_reserve(1).map_err(|_| ())?;
        let handle = self.next;
        self.objects.push(Object {
            handle,
            name: stored,
            seen: true,
        });
        self.next += 1;
        Ok(handle)
    }

    pub fn finish_scan(&mut self) {
        self.objects.retain(|object| object.seen);
    }

    pub fn name(&self, handle: u32) -> Option<&str> {
        self.objects
            .iter()
            .find(|object| object.handle == handle)
            .map(|object| object.name.as_str())
    }

    pub fn remove(&mut self, handle: u32) {
        self.objects.retain(|object| object.handle != handle);
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.objects.iter().map(|object| object.handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_then_create_preserves_cached_neighbor_handles() {
        let mut objects = ObjectHandles::new();
        let gallery = objects.observe("GALLERY.BIN").unwrap();
        let gif = objects.observe("GIF.BIN").unwrap();
        let snake = objects.observe("SNAKE.BIN").unwrap();
        objects.remove(gallery);
        objects.begin_scan();
        assert_eq!(objects.observe("GIF.BIN"), Ok(gif));
        assert_eq!(objects.observe("SNAKE.BIN"), Ok(snake));
        objects.finish_scan();
        assert_eq!(objects.name(gallery), None);

        let replacement = objects.observe("GALLERY.BIN").unwrap();
        assert_ne!(replacement, gallery);
        assert_ne!(replacement, gif);
        objects.begin_scan();
        assert_eq!(objects.observe("GALLERY.BIN"), Ok(replacement));
        assert_eq!(objects.observe("GIF.BIN"), Ok(gif));
        assert_eq!(objects.observe("SNAKE.BIN"), Ok(snake));
        objects.finish_scan();
        assert_eq!(objects.name(gif), Some("GIF.BIN"));
        assert_eq!(objects.name(snake), Some("SNAKE.BIN"));
        assert_eq!(objects.name(replacement), Some("GALLERY.BIN"));
        objects.remove(gallery);
        assert_eq!(objects.iter().len(), 3);
    }

    #[test]
    fn repeated_replacements_and_session_resets_never_reuse_handles() {
        let mut objects = ObjectHandles::new();
        let neighbor = objects.observe("DOOM.BIN").unwrap();
        let mut previous = 0;
        for _ in 0..1024 {
            let handle = objects.observe("GALLERY.BIN").unwrap();
            assert!(handle > previous);
            assert_eq!(objects.observe("gallery.bin"), Ok(handle));
            objects.remove(handle);
            assert_eq!(objects.name(handle), None);
            assert_eq!(objects.name(neighbor), Some("DOOM.BIN"));
            previous = handle;
        }
        objects.clear();
        assert_eq!(objects.name(neighbor), None);
        assert!(objects.observe("DOOM.BIN").unwrap() > previous);
    }

    #[test]
    fn scans_retire_missing_objects_only_on_completion() {
        let mut objects = ObjectHandles::new();
        let deleted = objects.observe("OLD.BIN").unwrap();
        let kept = objects.observe("KEEP.BIN").unwrap();
        objects.begin_scan();
        objects.observe("KEEP.BIN").unwrap();
        assert_eq!(objects.name(deleted), Some("OLD.BIN"));
        objects.finish_scan();
        assert_eq!(objects.name(deleted), None);
        assert_eq!(objects.name(kept), Some("KEEP.BIN"));
    }

    #[test]
    fn bounds_and_handle_exhaustion_fail_without_aliasing() {
        let mut objects = ObjectHandles::new();
        assert!(objects.observe("").is_err());
        assert!(objects.observe(&"A".repeat(MAX_NAME + 1)).is_err());
        for index in 0..MAX_OBJECTS {
            objects.observe(&alloc::format!("{index}.BIN")).unwrap();
        }
        assert!(objects.observe("EXTRA.BIN").is_err());
        assert_eq!(objects.iter().len(), MAX_OBJECTS);
        objects.remove(1);
        assert!(objects.observe("EXTRA.BIN").unwrap() > MAX_OBJECTS as u32);
        objects.clear();
        objects.next = u32::MAX - 1;
        assert_eq!(objects.observe("LAST.BIN"), Ok(u32::MAX - 1));
        assert!(objects.observe("WRAP.BIN").is_err());
        assert_eq!(objects.name(0), None);
        assert_eq!(objects.name(u32::MAX), None);
    }
}
