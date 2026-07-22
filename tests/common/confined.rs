macro_rules! assert_confined {
    ($ty:ty) => {
        const _: fn() = || {
            trait AmbiguousIfSend<A> {}
            impl<T: ?Sized> AmbiguousIfSend<()> for T {}
            impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}

            trait AmbiguousIfSync<A> {}
            impl<T: ?Sized> AmbiguousIfSync<()> for T {}
            impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}

            fn not_send<T: ?Sized + AmbiguousIfSend<A>, A>() {}
            fn not_sync<T: ?Sized + AmbiguousIfSync<A>, A>() {}

            not_send::<$ty, _>();
            not_sync::<$ty, _>();
        };
    };
}

pub(crate) use assert_confined;
