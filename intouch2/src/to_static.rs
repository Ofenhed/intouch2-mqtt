use std::borrow::Cow;

pub trait ToStatic {
    type Static: 'static;

    fn to_static(&self) -> Self::Static;
}

impl<O, S, T> ToStatic for Cow<'_, [T]>
where
    S: Clone + 'static,
    O: ToStatic<Static = S>,
    [T]: ToOwned<Owned = Vec<O>>,
{
    type Static = Cow<'static, [S]>;

    fn to_static(&self) -> Self::Static {
        match self {
            Cow::Owned(o) => o.to_static(),
            Cow::Borrowed(b) => (**b).to_owned().to_static(),
        }
    }
}

impl<const N: usize, S, T> ToStatic for [T; N]
where
    S: Clone + 'static,
    T: ToStatic<Static = S>,
{
    type Static = Cow<'static, [S; N]>;

    fn to_static(&self) -> Self::Static {
        let mut new_list = Vec::with_capacity(self.len());
        for item in self {
            new_list.push(item.to_static());
        }
        let Ok(b): Result<Box<[S; N]>, _> = new_list.try_into() else {
            unreachable!("The size is compile time checked as well")
        };
        Cow::Owned(*b)
    }
}

impl<S, T> ToStatic for Vec<T>
where
    S: Clone + 'static,
    T: ToStatic<Static = S>,
{
    type Static = Cow<'static, [S]>;

    fn to_static(&self) -> Self::Static {
        AsRef::<[T]>::as_ref(self).to_static()
    }
}

impl<S, T> ToStatic for [T]
where
    S: Clone + 'static,
    T: ToStatic<Static = S>,
{
    type Static = Cow<'static, [S]>;

    fn to_static(&self) -> Self::Static {
        self.iter().map(ToStatic::to_static).collect()
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

        fn to_static(&self) -> Self::Static {
            Self::Static {
                link: self.link.to_static(),
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
        drop(link);
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
        let static_copy: Cow<[SomeObject]> = list.to_static();
        arr[0] = 5;
        arr2[1] = 5;
        assert_eq!(&*static_copy[0].link, [1, 2, 3]);
        assert_eq!(&*static_copy[1].link, [2, 3, 4]);
        assert_eq!(&*static_copy[2].link, [3, 4, 5]);
        assert_eq!(arr, [5, 2, 3]);
        assert_eq!(arr2, [2, 5, 4]);
    }
}
