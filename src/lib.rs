#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(non_null_convenience)]

use std::{
    alloc::{AllocError, Allocator, Layout},
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

pub struct Arena {
    mem_pool: NonNull<[u8]>,
    offset: AtomicUsize,
}

unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    pub fn new(capacity: usize) -> Self {
        Self {
            mem_pool: unsafe { NonNull::new_unchecked(Box::into_raw(vec![0; capacity].into_boxed_slice())) },
            offset: AtomicUsize::new(0),
        }
    }

    fn capacity(&self) -> usize {
        self.mem_pool.len()
    }

    fn get_next_mem_slice(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
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
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe {
            let _: Box<_> = Box::from_raw(self.mem_pool.as_mut());
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

    #[allow(unused)]
    fn print_arena(&self) {
        unsafe {
            println!("{:?}", &self.arena.mem_pool);
        }
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
