use core::alloc::Layout;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub len: usize,
}

impl Region {
    pub fn contains(self, other: Self) -> bool {
        other.start >= self.start
            && other
                .start
                .checked_sub(self.start)
                .and_then(|offset| offset.checked_add(other.len))
                .is_some_and(|end| end <= self.len)
            && self.start.checked_add(self.len).is_some()
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.len != 0
            && other.len != 0
            && self.start < other.start.saturating_add(other.len)
            && other.start < self.start.saturating_add(self.len)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub address: usize,
    pub layout: Layout,
}

pub struct AppMemory<const COUNT: usize> {
    pub image: Region,
    pub stack: Region,
    allocations: [Option<Allocation>; COUNT],
}

impl<const COUNT: usize> AppMemory<COUNT> {
    pub const fn new() -> Self {
        Self {
            image: Region { start: 0, len: 0 },
            stack: Region { start: 0, len: 0 },
            allocations: [None; COUNT],
        }
    }

    pub fn permits(&self, address: usize, count: usize, size: usize, alignment: usize) -> bool {
        if !crate::buffer_shape_valid(address, count, size, alignment) {
            return false;
        }
        if count == 0 {
            return true;
        }
        let region = Region {
            start: address,
            len: count * size,
        };
        self.image.contains(region)
            || self.stack.contains(region)
            || self.allocations.iter().flatten().any(|allocation| {
                Region {
                    start: allocation.address,
                    len: allocation.layout.size(),
                }
                .contains(region)
            })
    }

    pub fn track(&mut self, allocation: Allocation) -> bool {
        if allocation.address == 0 || allocation.layout.size() == 0 {
            return false;
        }
        let Some(slot) = self.allocations.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some(allocation);
        true
    }

    pub fn release(&mut self, allocation: Allocation) -> bool {
        let Some(slot) = self
            .allocations
            .iter_mut()
            .find(|slot| **slot == Some(allocation))
        else {
            return false;
        };
        *slot = None;
        true
    }

    pub fn pop(&mut self) -> Option<Allocation> {
        self.allocations
            .iter_mut()
            .find(|slot| slot.is_some())?
            .take()
    }
}

impl<const COUNT: usize> Default for AppMemory<COUNT> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_only_complete_owned_ranges() {
        let mut memory = AppMemory::<1>::new();
        memory.image = Region {
            start: 0x1000,
            len: 0x100,
        };
        memory.stack = Region {
            start: 0x3000,
            len: 0x100,
        };
        assert!(memory.permits(0x1000, 64, 4, 4));
        assert!(memory.permits(0x3000, 256, 1, 1));
        for (address, len) in [
            (0, 1),
            (0x1001, 4),
            (0x10fc, 8),
            (0x2000, 4),
            (usize::MAX - 3, 8),
        ] {
            assert!(!memory.permits(address, len, 1, 4));
        }
        assert!(!memory.permits(0x1000, usize::MAX, 4, 4));
        assert!(memory.permits(0, 0, 1, 1));
        let allocation = Allocation {
            address: 0x2000,
            layout: Layout::from_size_align(32, 8).unwrap(),
        };
        assert!(memory.track(allocation));
        assert!(!memory.track(allocation));
        assert!(memory.permits(0x2008, 24, 1, 1));
        assert!(!memory.release(Allocation {
            address: 0x2008,
            ..allocation
        }));
        assert!(!memory.release(Allocation {
            layout: Layout::from_size_align(16, 8).unwrap(),
            ..allocation
        }));
        assert!(memory.release(allocation));
        assert!(!memory.release(allocation));
        assert!(!memory.permits(0x2000, 1, 1, 1));
        assert!(memory.track(allocation));
        assert_eq!(memory.pop(), Some(allocation));
        assert_eq!(memory.pop(), None);
    }

    #[test]
    fn alias_checks_allow_adjacent_and_empty_buffers() {
        let region = Region {
            start: 100,
            len: 10,
        };
        assert!(region.overlaps(Region { start: 99, len: 2 }));
        assert!(region.overlaps(Region { start: 109, len: 2 }));
        assert!(!region.overlaps(Region { start: 110, len: 1 }));
        assert!(!region.overlaps(Region { start: 105, len: 0 }));
        assert!(
            !Region {
                start: usize::MAX,
                len: 2
            }
            .contains(Region {
                start: usize::MAX,
                len: 1
            })
        );
    }
}
