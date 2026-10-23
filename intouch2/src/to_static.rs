use std::{borrow::Cow, ops::Deref};

pub trait ToStatic {
    type Static: 'static;

    fn to_static(self) -> Self::Static;
}

pub trait ClonedStatic
where
    Self: Deref,
    <Self as Deref>::Target: ToStatic,
{
    fn cloned_static(self) -> <<Self as Deref>::Target as ToStatic>::Static;
}

impl<T: Clone + ToStatic> ClonedStatic for &T {
    fn cloned_static(self) -> <T as ToStatic>::Static {
        let cloned = self.clone();
        cloned.to_static()
    }
}

impl<'a, T: ToOwned + ?Sized> ToStatic for Cow<'a, T>
where
    <T as ToOwned>::Owned: ToStatic,
{
    type Static = <<T as ToOwned>::Owned as ToStatic>::Static;

    fn to_static(self) -> Self::Static {
        self.into_owned().to_static()
    }
}

impl<const N: usize, S, T: Clone> ToStatic for &[T; N]
where
    S: Clone + 'static,
    T: ToStatic<Static = S>,
{
    type Static = Cow<'static, [S; N]>;

    fn to_static(self) -> Self::Static {
        let mut new_list = Vec::with_capacity(self.len());
        for item in self {
            new_list.push(item.cloned_static());
        }
        let Ok(b): Result<Box<[S; N]>, _> = new_list.try_into() else {
            unreachable!("The size is compile time checked as well")
        };
        Cow::Owned(*b)
    }
}

impl<const N: usize, S, T: Clone> ToStatic for [T; N]
where
    S: Clone + 'static,
    T: ToStatic<Static = S>,
{
    type Static = Cow<'static, [S; N]>;

    fn to_static(self) -> Self::Static {
        let mut new_list = Vec::with_capacity(N);
        for item in self {
            new_list.push(item.to_static());
        }
        let Ok(b): Result<Box<[S; N]>, _> = new_list.try_into() else {
            unreachable!("The size is compile time checked as well")
        };
        Cow::Owned(*b)
    }
}

impl<T> ToStatic for Vec<T>
where
    T: ToStatic,
    [T::Static]: ToOwned,
    <[T::Static] as ToOwned>::Owned: FromIterator<T::Static>,
{
    type Static = Cow<'static, [T::Static]>;

    fn to_static(self) -> Self::Static {
        Cow::Owned(self.into_iter().map(ToStatic::to_static).collect())
    }
}

impl<T> ToStatic for &[T]
where
    T: ToStatic + Clone,
    T::Static: Clone,
{
    type Static = Cow<'static, [T::Static]>;

    fn to_static(self) -> Self::Static {
        self.iter().cloned().map(ToStatic::to_static).collect()
    }
}

#[cfg(test)]
mod to_static_tests {
    use super::*;
    #[derive(Clone)]
    struct SomeObject<'a> {
        link: Cow<'a, [u8]>,
    }

    impl ToStatic for SomeObject<'_> {
        type Static = SomeObject<'static>;

        fn to_static(self) -> Self::Static {
            Self::Static {
                link: self.link.to_static().into(),
            }
        }
    }

    #[test]
    fn convert_cow_lifetime() {
        let mut arr = [1, 2, 3];
        let link = SomeObject {
            link: Cow::Borrowed(&arr),
        };
        let static_copy = link.to_static();
        arr[0] = 5;
        assert_eq!(&*static_copy.link, [1, 2, 3]);
        assert_eq!(arr, [5, 2, 3]);
    }

    #[test]
    fn convert_complex_cow_lifetime() {
        let mut arr = [1, 2, 3];
        let mut arr2 = [2, 3, 4];
        let link1 = SomeObject {
            link: Cow::Borrowed(&arr),
        };
        let link2 = SomeObject {
            link: Cow::Borrowed(&arr2),
        };
        let link3 = SomeObject {
            link: Cow::Owned(vec![3, 4, 5]),
        };
        let list: Cow<[SomeObject]> = Cow::Owned(vec![link1, link2, link3]);
        let static_copy: Cow<[SomeObject]> = list.to_static().into();
        arr[0] = 5;
        arr2[1] = 5;
        assert_eq!(&*static_copy[0].link, [1, 2, 3]);
        assert_eq!(&*static_copy[1].link, [2, 3, 4]);
        assert_eq!(&*static_copy[2].link, [3, 4, 5]);
        assert_eq!(arr, [5, 2, 3]);
        assert_eq!(arr2, [2, 5, 4]);
    }
}
