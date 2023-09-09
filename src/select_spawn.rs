use core::{future::Future, pin::Pin, task::{Context, Poll}};

use embassy_futures::select::{Either, select};


pub struct SelectSpawn<F>(F);

impl<F> SelectSpawn<F> {
    pub fn run(fut: F) -> Self {
        Self(fut)
    }

    pub fn chain<F2>(self, fut: F2) -> SelectSpawn<impl Future<Output = F::Output>>
    where
        F: Future,
        F2: Future<Output = F::Output>,
    {
        SelectSpawn(async move {
            match select(self.0, fut).await {
                Either::First(res) => res,
                Either::Second(res) => res,
            }
        })
    }
}

impl<F> Future for SelectSpawn<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };

        let fut = unsafe { Pin::new_unchecked(&mut this.0) };

        fut.poll(cx)
    }
}
