#![feature(allocator_api)]

use std::{
    alloc::{AllocError, Allocator, Layout},
    cell::UnsafeCell,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct Arena {
    mem_pool: Box<[u8]>,
    offset: AtomicUsize,
}

impl Arena {
    pub fn new(capacity: usize) -> Self {
        Self {
            mem_pool: vec![0; capacity].into_boxed_slice(),
            offset: AtomicUsize::new(0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArenaAllocator<'a> {
    mem_pool: &'a UnsafeCell<[u8]>,
    offset: &'a AtomicUsize,
}

impl<'a> ArenaAllocator<'a> {
    pub fn new(arena: &'a mut Arena) -> Self {
        Self {
            mem_pool: unsafe { std::mem::transmute(arena.mem_pool.as_mut() as *mut [u8]) },
            offset: &arena.offset,
        }
    }

    fn capacity(&self) -> usize {
        unsafe { (*self.mem_pool.get()).len() }
    }

    fn print_arena(&self) {
        unsafe {
            println!("{:?}", &(*self.mem_pool.get()));
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
            let offset = self.offset.load(Ordering::Acquire);
            // make sure memory is aligned
            let start = offset.next_multiple_of(layout.align());
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

unsafe impl Allocator for ArenaAllocator<'_> {
    fn allocate(&self, layout: Layout) -> Result<std::ptr::NonNull<[u8]>, AllocError> {
        let (mem_start, mem_end) = self.get_aligned_memory_bounds(layout)?;

        unsafe {
            let mem_ptr = &mut (*self.mem_pool.get())[mem_start..mem_end];
            Ok(NonNull::<[u8]>::new_unchecked(mem_ptr))
        }
    }

    unsafe fn deallocate(&self, _ptr: std::ptr::NonNull<u8>, _layout: Layout) {}
}

#[test]
fn allocation() {
    let mut arena = Arena::new(8);
    let arena_alloc = ArenaAllocator::new(&mut arena);
    let mut vec1 = Vec::<u8, ArenaAllocator>::with_capacity_in(2, arena_alloc);
    let mut vec2 = Vec::<u32, ArenaAllocator>::with_capacity_in(1, arena_alloc);

    vec1.push(0xAA);
    vec1.push(0x55);
    vec2.push(0xFFFFFFFF);

    println!("{:?}", vec1);
    println!("{:?}", vec2);

    arena_alloc.print_arena();
}
