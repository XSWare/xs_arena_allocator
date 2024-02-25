#![feature(allocator_api)]
#![feature(slice_ptr_get)]

use std::{
    alloc::{AllocError, Allocator, Layout},
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// allocates a memory pool during construction and only de-allocates it during `drop()`.
/// chunks of the memory pool can be requested until nothing is left, which makes every consecutive call fail.
pub struct Arena {
    mem_pool: NonNull<[u8]>,
    offset: AtomicUsize,
}

unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    /// create a new arena with the passed capacity in bytes.
    pub fn new(capacity: usize) -> Self {
        Self {
            mem_pool: unsafe { NonNull::new_unchecked(Box::into_raw(vec![0; capacity].into_boxed_slice())) },
            offset: AtomicUsize::new(0),
        }
    }

    /// returns the maximum capacity of the arena, including the space thats already used.
    pub fn capacity(&self) -> usize {
        self.mem_pool.len()
    }

    /// returns the available space of the arena in bytes.
    pub fn available_space(&self) -> usize {
        self.capacity() - self.offset.load(Ordering::Relaxed)
    }

    /// returns a pointer to a memory slice with the size and alignment of the passed `Layout`.
    pub fn get_next_mem_slice(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let (start, end) = self.get_aligned_memory_bounds(layout)?;
        unsafe { Ok(self.mem_pool.get_unchecked_mut(start..end)) }
    }

    /// returns the offset start and end for the allocated memory.
    fn get_aligned_memory_bounds(&self, layout: Layout) -> Result<(usize, usize), AllocError> {
        let requested_size = layout.size();

        loop {
            // get a copy of the current value of the offset.
            // it is important that the value used for the calculations can't be changed by another thread
            // since we need to very the result with a compare_exchange in the end.
            let offset = self.offset.load(Ordering::Acquire);
            let align_offset = unsafe { self.mem_pool.as_mut_ptr().add(offset).align_offset(layout.align()) };

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
                .offset
                .compare_exchange(offset, end, Ordering::Release, Ordering::SeqCst)
                .is_ok()
            {
                return Ok((start, end));
            }
        }
    }

    /// SAFETY: must not be called while any &mut to the memory pool exist.
    /// this means ALL allocations were freed beforehand.
    #[cfg(test)]
    pub unsafe fn print(&self) {
        unsafe {
            println!("{:?}", self.mem_pool.as_ref());
        }
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe {
            let _: Box<_> = Box::from_raw(self.mem_pool.as_mut());
        }
    }
}

/// bump style arena allocator.
#[derive(Clone)]
pub struct ArenaAllocator {
    arena: Arc<Arena>,
}

impl ArenaAllocator {
    pub fn new(arena: Arena) -> Self {
        Self { arena: Arc::new(arena) }
    }

    /// SAFETY: the arena must not be used while there are still active allocations.
    #[cfg(test)]
    pub unsafe fn get_arena(&self) -> &Arena {
        &self.arena
    }
}

unsafe impl Allocator for ArenaAllocator {
    fn allocate(&self, layout: Layout) -> Result<std::ptr::NonNull<[u8]>, AllocError> {
        self.arena.get_next_mem_slice(layout)
    }

    unsafe fn deallocate(&self, _ptr: std::ptr::NonNull<u8>, _layout: Layout) {}
}

#[cfg(test)]
mod test {
    use std::thread::{self, JoinHandle};

    use super::*;

    #[test]
    fn allocation() {
        let thread_count = 100;
        // 2x1 byte + 1x4 byte allocation that can take up to 7 bytes with alignment = 9 bytes max per thread
        let arena = Arena::new(thread_count * 9);
        let arena_alloc = ArenaAllocator::new(arena);
        let mut join_handles = Vec::with_capacity(thread_count);

        for _ in 0..thread_count {
            let alloc = arena_alloc.clone();
            join_handles.push(spawn_allocating_thread(alloc));
        }

        join_handles.into_iter().for_each(|j| {
            j.join().unwrap();
        });

        unsafe {
            arena_alloc.get_arena().print();
            println!("available bytes: {}", arena_alloc.arena.available_space())
        }
    }

    fn spawn_allocating_thread(arena_allocator: ArenaAllocator) -> JoinHandle<()> {
        thread::spawn(move || {
            let mut vec1 = Vec::<u8, ArenaAllocator>::with_capacity_in(2, arena_allocator.clone());
            let mut vec2 = Vec::<u32, ArenaAllocator>::with_capacity_in(1, arena_allocator);
            vec1.push(0xAA);
            vec1.push(0x55);
            vec2.push(0xFFFFFFFF);
        })
    }
}
