use core::marker::PhantomData;
use core::mem::transmute;
use core::ptr::{drop_in_place, read, NonNull};

extern crate alloc;

use alloc::alloc::{dealloc, Layout};

// Self referential structs are currently not supported with safe vanilla Rust.
// The only reasonable safe alternative is to expect the user to juggle 2 separate
// data structures which is a mess. The library solution rental is both no longer
// maintained and really heavy to compile. So begrudgingly I rolled my own version.
// These are some of the core invariants we require for this to be safe to use.
//
// 1. owner is initialized when UnsafeSelfCell is constructed.
// 2. owner is NEVER changed again.
// 3. The pointer to owner and dependent never changes, even when moved.
// 4. The only access to owner and dependent is as immutable reference.
// 5. owner lives longer than dependent.

#[doc(hidden)]
pub struct JoinedCell<Owner, Dependent> {
    pub owner: Owner,
    pub dependent: Dependent,
}

// Library controlled struct that marks all accesses as unsafe.
// Because the macro generated struct impl can be extended, could be unsafe.
#[doc(hidden)]
pub struct UnsafeSelfCell<Owner: 'static, DependentStatic: 'static> {
    joined_void_ptr: NonNull<u8>,

    owner_marker: PhantomData<Owner>,
    // DependentStatic is only used to correctly derive Send and Sync.
    dependent_marker: PhantomData<DependentStatic>,
}

impl<Owner, DependentStatic> UnsafeSelfCell<Owner, DependentStatic> {
    pub unsafe fn new(joined_void_ptr: NonNull<u8>) -> Self {
        Self {
            joined_void_ptr,
            owner_marker: PhantomData,
            dependent_marker: PhantomData,
        }
    }

    pub unsafe fn borrow_owner<'a, Dependent>(&'a self) -> &'a Owner {
        let joined_ptr =
            transmute::<NonNull<u8>, NonNull<JoinedCell<Owner, Dependent>>>(self.joined_void_ptr);

        &(*joined_ptr.as_ptr()).owner
    }

    pub unsafe fn borrow_dependent<'a, Dependent>(&'a self) -> &'a Dependent {
        let joined_ptr =
            transmute::<NonNull<u8>, NonNull<JoinedCell<Owner, Dependent>>>(self.joined_void_ptr);

        &(*joined_ptr.as_ptr()).dependent
    }

    pub unsafe fn borrow_mut<'a, Dependent>(&'a mut self) -> &'a mut JoinedCell<Owner, Dependent> {
        let joined_ptr =
            transmute::<NonNull<u8>, NonNull<JoinedCell<Owner, Dependent>>>(self.joined_void_ptr);

        &mut (*joined_ptr.as_ptr())
    }

    // Any subsequent use of this struct other than dropping it is UB.
    pub unsafe fn drop_joined<Dependent>(&mut self) {
        let joined_ptr =
            transmute::<NonNull<u8>, NonNull<JoinedCell<Owner, Dependent>>>(self.joined_void_ptr);

        drop_in_place(joined_ptr.as_ptr());

        let layout = Layout::new::<JoinedCell<Owner, Dependent>>();

        dealloc(self.joined_void_ptr.as_ptr(), layout);
    }

    pub unsafe fn into_owner<Dependent>(self) -> Owner {
        let joined_ptr =
            transmute::<NonNull<u8>, NonNull<JoinedCell<Owner, Dependent>>>(self.joined_void_ptr);

        let owner_ptr: *const Owner = &(*joined_ptr.as_ptr()).owner;

        // Move owner out so it can be returned.
        let owner = read(owner_ptr);

        // Clean up rest of JoinedCell
        drop_in_place(&mut (*joined_ptr.as_ptr()).dependent);

        let layout = Layout::new::<JoinedCell<Owner, Dependent>>();
        dealloc(self.joined_void_ptr.as_ptr(), layout);

        owner
    }
}

unsafe impl<Owner, DependentStatic> Send for UnsafeSelfCell<Owner, DependentStatic>
where
    // Only derive Send if Owner and DependentStatic is also Send
    Owner: Send,
    DependentStatic: Send,
{
}

unsafe impl<Owner, DependentStatic> Sync for UnsafeSelfCell<Owner, DependentStatic>
where
    // Only derive Sync if Owner and DependentStatic is also Sync
    Owner: Sync,
    DependentStatic: Sync,
{
}

// This struct is used to safely deallocate only the owner if dependent
// construction fails.
#[doc(hidden)]
pub struct OwnerAndCellDropGuard<Owner, Dependent> {
    fully_init: bool,
    joined_ptr: NonNull<JoinedCell<Owner, Dependent>>,
}

impl<Owner, Dependent> OwnerAndCellDropGuard<Owner, Dependent> {
    pub fn new(joined_ptr: NonNull<JoinedCell<Owner, Dependent>>) -> Self {
        Self {
            fully_init: false,
            joined_ptr,
        }
    }

    pub fn mark_fully_init(&mut self) {
        self.fully_init = true;
    }
}

impl<Owner, Dependent> Drop for OwnerAndCellDropGuard<Owner, Dependent> {
    fn drop(&mut self) {
        if self.fully_init {
            // We took over ownership and no cleanup should be done.
            return;
        }

        unsafe {
            // We must only drop owner and the struct itself,
            // The whole point of this drop guard is to clean up the partially
            // initialized struct should building the dependent fail.
            drop_in_place(&mut (*self.joined_ptr.as_ptr()).owner);

            let layout = Layout::new::<JoinedCell<Owner, Dependent>>();
            let joined_void_ptr =
                transmute::<*mut JoinedCell<Owner, Dependent>, *mut u8>(self.joined_ptr.as_ptr());
            dealloc(joined_void_ptr, layout);
        }
    }
}
