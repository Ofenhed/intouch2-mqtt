use std::{
    error::Error,
    fmt::Display,
    ops::{Deref, DerefMut},
    sync::Mutex,
};

#[derive(Debug)]
enum MaybeEntered {
    Span(tracing::span::Span),
    Entered { _guard: tracing::span::EnteredSpan },
}

#[derive(Debug)]
pub struct SpannedError<E> {
    err: E,
    guard: Mutex<MaybeEntered>,
}

impl<E: Display> Display for SpannedError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.err.fmt(f)
    }
}

impl<E: Error> Error for SpannedError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.err.source()
    }

    fn cause(&self) -> Option<&dyn Error> {
        self.err.source()
    }

    fn provide<'a>(&'a self, request: &mut std::error::Request<'a>) {
        self.err.provide(request);
    }
}
unsafe impl<E: Send> Send for SpannedError<E> {}
unsafe impl<E: Sync> Sync for SpannedError<E> {}

impl<T> Deref for SpannedError<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        if let Ok(mut g) = self.guard.try_lock() {
            if let MaybeEntered::Span(s) = &(*g) {
                *g = MaybeEntered::Entered {
                    _guard: s.clone().entered(),
                }
            }
        }
        &self.err
    }
}

impl<T> DerefMut for SpannedError<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.err
    }
}

pub trait ResultSpan<'a>
where
    Self: Sized + 'a,
{
    type Spanned;
    #[track_caller]
    fn in_span(self, span: &'a tracing::Span) -> Self;
    #[track_caller]
    fn into_span(self, span: &'a tracing::Span) -> Self::Spanned;
}
impl<'a, T: 'a, E: std::fmt::Display + 'a> ResultSpan<'a> for Result<T, E> {
    type Spanned = Result<T, SpannedError<E>>;
    #[track_caller]
    fn in_span(self, span: &tracing::Span) -> Self {
        if let Err(err) = &self {
            tracing::event!(parent: span, tracing::Level::TRACE, err = tracing::field::display(err));
        }
        self
    }
    fn into_span(self, span: &'a tracing::Span) -> Self::Spanned {
        self.map_err(|e| SpannedError {
            err: e,
            guard: MaybeEntered::Span(span.clone()).into(),
        })
    }
}
