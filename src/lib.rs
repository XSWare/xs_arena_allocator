#![feature(allocator_api)]
#![feature(slice_ptr_get)]

use std::{
    alloc::{AllocError, Allocator, Layout},
    cell::UnsafeCell,
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

pub struct Arena {
    mem_pool: Box<[u8]>,
    offset: AtomicUsize,
}

unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    pub fn new(capacity: usize) -> Self {
        Self {
            mem_pool: vec![0; capacity].into_boxed_slice(),
            offset: AtomicUsize::new(0),
        }
    }
}

#[derive(Clone)]
pub struct ArenaAllocator {
    arena: Arc<Arena>,
}

impl ArenaAllocator {
    pub fn new(arena: Arena) -> Self {
        Self { arena: Arc::new(arena) }
    }

    fn get_mem_pool(&self) -> &UnsafeCell<[u8]> {
        unsafe { std::mem::transmute(self.arena.mem_pool.as_ref()) }
    }

    fn capacity(&self) -> usize {
        self.arena.mem_pool.len()
    }

    #[allow(unused)]
    fn print_arena(&self) {
        unsafe {
            println!("{:?}", &self.arena.mem_pool);
        }
    }

    /// returns the offset start and end for the allocated memory.
    #[must_use]
    fn get_aligned_memory_bounds(&self, layout: Layout) -> Result<(usize, usize), AllocError> {
        let requested_size = layout.size();

        loop {
            // get a copy of the current value of the offset.
            // it is important that the value used for the calculations can't be changed by another thread
            // since we need to very the result with a compare_exchange in the end.
            let offset = self.arena.offset.load(Ordering::Acquire);
            let align_offset = unsafe { self.get_mem_pool().get().as_mut_ptr().add(offset).align_offset(layout.align()) };

            // failed to find suitable alignment
            if align_offset == usize::MAX {
                return Err(AllocError);
            }

            // make sure memory is aligned
            let start = offset + align_offset;
            // end will always be aligned if start is aligned since the requested size can only be multiples
            let end = start + requested_size;

            if end > self.capacity() {
                return Err(AllocError);
            }

            // if there is enough space available and nobody else claimed that space in the meantime the result is returned.
            // otherwise the process needs to be retried.
            if self
                .arena
                .offset
                .compare_exchange(offset, end, Ordering::Release, Ordering::SeqCst)
                .is_ok()
            {
                return Ok((start, end));
            }
        }
    }
}

unsafe impl Allocator for ArenaAllocator {
    fn allocate(&self, layout: Layout) -> Result<std::ptr::NonNull<[u8]>, AllocError> {
        let (mem_start, mem_end) = self.get_aligned_memory_bounds(layout)?;

        unsafe {
            let mem_ptr = self.get_mem_pool().get().get_unchecked_mut(mem_start..mem_end);
            Ok(NonNull::<[u8]>::new_unchecked(mem_ptr))
        }
    }

    unsafe fn deallocate(&self, _ptr: std::ptr::NonNull<u8>, _layout: Layout) {}
}

#[cfg(test)]
mod test {
    use std::thread;

    use super::*;

    #[test]
    fn allocation() {
        let arena_alloc = ArenaAllocator::new(Arena::new(8));
        let mut vec1 = Vec::<u8, ArenaAllocator>::with_capacity_in(2, arena_alloc.clone());
        let mut vec2 = Vec::<u32, ArenaAllocator>::with_capacity_in(1, arena_alloc.clone());

        vec1.push(0xAA);
        vec1.push(0x55);
        vec2.push(0xFFFFFFFF);

        println!("{:?}", vec1);
        println!("{:?}", vec2);

        arena_alloc.print_arena();
    }

    fn _spawn_allocating_thread(arena_allocator: ArenaAllocator) {
        thread::spawn(move || {
            let _vec1 = Vec::<u8, ArenaAllocator>::with_capacity_in(2, arena_allocator);
        });
    }
}
