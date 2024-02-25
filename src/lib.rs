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

    fn offset(&self) -> usize {
        self.offset.load(Ordering::Relaxed)
    }

    fn capacity(&self) -> usize {
        unsafe { (*self.mem_pool.get()).len() }
    }

    fn print_arena(&self) {
        unsafe {
            println!("{:?}", &(*self.mem_pool.get()));
        }
    }
}

unsafe impl Allocator for ArenaAllocator<'_> {
    fn allocate(&self, layout: Layout) -> Result<std::ptr::NonNull<[u8]>, AllocError> {
        unsafe {
            let requested_size = layout.size();
            let offset_aligned = self.offset().next_multiple_of(layout.align());
            let offset_end = offset_aligned + requested_size;

            if offset_end > self.capacity() {
                return Err(AllocError);
            }

            self.offset.store(offset_end, Ordering::Relaxed);

            let mem_ptr = &mut (*self.mem_pool.get())[offset_aligned..offset_end];
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
